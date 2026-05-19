use std::env;
use std::net::{TcpStream, ToSocketAddrs, UdpSocket};
use std::process::{Command, ExitCode};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommandMode {
    Probe { assert_blocked: bool },
    FirewallWrap { program: String },
    Cleanup { rule_prefix: String },
}

fn main() -> ExitCode {
    let mode = match parse_args(env::args().skip(1)) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    match run(mode) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("POC-0103 failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<CommandMode, String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        None | Some("probe") => Ok(CommandMode::Probe {
            assert_blocked: false,
        }),
        Some("assert-blocked") => Ok(CommandMode::Probe {
            assert_blocked: true,
        }),
        Some("firewall-wrap") => {
            let program = args
                .next()
                .filter(|value| !value.trim().is_empty())
                .ok_or("firewall-wrap requires a program path")?;
            Ok(CommandMode::FirewallWrap { program })
        }
        Some("cleanup") => {
            let rule_prefix = args
                .next()
                .unwrap_or_else(|| "SandboxPlusPoc0103".to_string());
            Ok(CommandMode::Cleanup { rule_prefix })
        }
        Some("--help") | Some("-h") => Err("POC-0103 firewall/network probe".into()),
        Some(other) => Err(format!("unknown command: {other}")),
    }
}

fn print_usage() {
    eprintln!(
        "Usage:\n  sandbox-network-poc probe\n  sandbox-network-poc assert-blocked\n  sandbox-network-poc firewall-wrap <program-path>\n  sandbox-network-poc cleanup [rule-prefix]"
    );
}

fn run(mode: CommandMode) -> Result<(), String> {
    match mode {
        CommandMode::Probe { assert_blocked } => run_probe(assert_blocked),
        CommandMode::FirewallWrap { program } => firewall_wrap(&program),
        CommandMode::Cleanup { rule_prefix } => cleanup_rules(&rule_prefix),
    }
}

fn run_probe(assert_blocked: bool) -> Result<(), String> {
    println!("POC-0103 network probe");
    let results = [
        ("tcp 1.1.1.1:443", tcp_probe("1.1.1.1:443")),
        ("tcp example.com:80", tcp_probe("example.com:80")),
        ("udp dns 1.1.1.1:53", udp_dns_probe("1.1.1.1:53")),
        (
            "tcp ipv6 [2606:4700:4700::1111]:443",
            tcp_probe("[2606:4700:4700::1111]:443"),
        ),
    ];

    let mut reachable = Vec::new();
    for (label, result) in results {
        if result.is_ok() {
            reachable.push(label);
        }
        report(label, result);
    }

    if assert_blocked && !reachable.is_empty() {
        return Err(format!(
            "expected public probes to be blocked, but reachable probes were: {}",
            reachable.join(", ")
        ));
    }

    Ok(())
}

fn firewall_wrap(program: &str) -> Result<(), String> {
    let prefix = "SandboxPlusPoc0103";
    let rules = [format!("{prefix}-block-tcp"), format!("{prefix}-block-udp")];

    add_firewall_rule(&rules[0], program, "TCP")?;
    add_firewall_rule(&rules[1], program, "UDP")?;
    println!("added outbound block rules for {program}");
    println!("run the target program's network probe now, then execute cleanup.");
    Ok(())
}

fn cleanup_rules(prefix: &str) -> Result<(), String> {
    delete_firewall_rule(&format!("{prefix}-block-tcp"))?;
    delete_firewall_rule(&format!("{prefix}-block-udp"))?;
    println!("cleanup requested for rules with prefix {prefix}");
    Ok(())
}

fn add_firewall_rule(name: &str, program: &str, protocol: &str) -> Result<(), String> {
    run_netsh([
        "advfirewall",
        "firewall",
        "add",
        "rule",
        &format!("name={name}"),
        "dir=out",
        "action=block",
        &format!("program={program}"),
        &format!("protocol={protocol}"),
        "enable=yes",
    ])
}

fn delete_firewall_rule(name: &str) -> Result<(), String> {
    run_netsh([
        "advfirewall",
        "firewall",
        "delete",
        "rule",
        &format!("name={name}"),
    ])
}

fn run_netsh<const N: usize>(args: [&str; N]) -> Result<(), String> {
    let status = Command::new("netsh")
        .args(args)
        .status()
        .map_err(|error| format!("failed to start netsh: {error}"))?;
    if !status.success() {
        return Err(format!("netsh exited with {status}"));
    }
    Ok(())
}

fn tcp_probe(address: &str) -> Result<(), String> {
    let socket = address
        .to_socket_addrs()
        .map_err(|error| format!("resolve failed: {error}"))?
        .next()
        .ok_or_else(|| "no socket address resolved".to_string())?;
    TcpStream::connect_timeout(&socket, Duration::from_secs(3))
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn udp_dns_probe(address: &str) -> Result<(), String> {
    let socket = address
        .to_socket_addrs()
        .map_err(|error| format!("resolve failed: {error}"))?
        .next()
        .ok_or_else(|| "no socket address resolved".to_string())?;
    let bind_addr = if socket.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let udp = UdpSocket::bind(bind_addr).map_err(|error| error.to_string())?;
    udp.set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| error.to_string())?;
    udp.set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|error| error.to_string())?;

    let payload = dns_query_payload();
    udp.send_to(&payload, socket)
        .map_err(|error| error.to_string())?;

    let mut response = [0u8; 512];
    udp.recv_from(&mut response)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn report(label: &str, result: Result<(), String>) {
    match result {
        Ok(()) => println!("{label}: reachable"),
        Err(error) => println!("{label}: blocked_or_failed ({error})"),
    }
}

fn dns_query_payload() -> [u8; 29] {
    [
        0x12, 0x34, // transaction id
        0x01, 0x00, // standard recursive query
        0x00, 0x01, // qdcount
        0x00, 0x00, // ancount
        0x00, 0x00, // nscount
        0x00, 0x00, // arcount
        0x07, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm', 0x00, // root
        0x00, 0x01, // A
        0x00, 0x01, // IN
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_command_is_probe() {
        let result = parse_args(Vec::<String>::new());
        assert!(result.is_ok(), "{result:?}");
        let Ok(mode) = result else { return };
        assert_eq!(
            mode,
            CommandMode::Probe {
                assert_blocked: false
            }
        );
    }

    #[test]
    fn parses_assert_blocked_probe() {
        let result = parse_args(["assert-blocked".into()]);
        assert!(result.is_ok(), "{result:?}");
        let Ok(mode) = result else { return };
        assert_eq!(
            mode,
            CommandMode::Probe {
                assert_blocked: true
            }
        );
    }

    #[test]
    fn parses_cleanup_default_prefix() {
        let result = parse_args(["cleanup".into()]);
        assert!(result.is_ok(), "{result:?}");
        let Ok(mode) = result else { return };
        assert_eq!(
            mode,
            CommandMode::Cleanup {
                rule_prefix: "SandboxPlusPoc0103".into()
            }
        );
    }
}
