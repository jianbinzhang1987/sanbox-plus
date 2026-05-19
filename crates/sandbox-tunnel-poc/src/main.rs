use std::env;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    CheckTools,
    Install { config: PathBuf },
    Uninstall { tunnel_name: String },
    Probe { target: String },
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
            eprintln!("POC-0104 failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Mode, String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        None | Some("check-tools") => Ok(Mode::CheckTools),
        Some("install") => Ok(Mode::Install {
            config: PathBuf::from(next_value(&mut args, "install")?),
        }),
        Some("uninstall") => Ok(Mode::Uninstall {
            tunnel_name: next_value(&mut args, "uninstall")?,
        }),
        Some("probe") => Ok(Mode::Probe {
            target: next_value(&mut args, "probe")?,
        }),
        Some("--help") | Some("-h") => Err("POC-0104 WireGuard tunnel probe".into()),
        Some(other) => Err(format!("unknown command: {other}")),
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, command: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{command} requires a value"))
}

fn print_usage() {
    eprintln!(
        "Usage:\n  sandbox-tunnel-poc check-tools\n  sandbox-tunnel-poc install <wg-config-path>\n  sandbox-tunnel-poc uninstall <tunnel-name>\n  sandbox-tunnel-poc probe <host:port>"
    );
}

fn run(mode: Mode) -> Result<(), String> {
    match mode {
        Mode::CheckTools => check_tools(),
        Mode::Install { config } => wireguard(["/installtunnelservice", path_to_str(&config)?]),
        Mode::Uninstall { tunnel_name } => wireguard(["/uninstalltunnelservice", &tunnel_name]),
        Mode::Probe { target } => probe_target(&target),
    }
}

fn check_tools() -> Result<(), String> {
    println!("POC-0104 checking WireGuard CLI");
    match Command::new("wireguard.exe").arg("/?").status() {
        Ok(status) => {
            println!("wireguard.exe found, status={status}");
            Ok(())
        }
        Err(error) => Err(format!("wireguard.exe not found or not runnable: {error}")),
    }
}

fn wireguard<const N: usize>(args: [&str; N]) -> Result<(), String> {
    let status = Command::new("wireguard.exe")
        .args(args)
        .status()
        .map_err(|error| format!("failed to start wireguard.exe: {error}"))?;
    if !status.success() {
        return Err(format!("wireguard.exe exited with {status}"));
    }
    Ok(())
}

fn probe_target(target: &str) -> Result<(), String> {
    let socket = target
        .to_socket_addrs()
        .map_err(|error| format!("resolve failed: {error}"))?
        .next()
        .ok_or_else(|| "no socket address resolved".to_string())?;
    TcpStream::connect_timeout(&socket, Duration::from_secs(5))
        .map_err(|error| format!("target not reachable: {error}"))?;
    println!("{target} reachable");
    Ok(())
}

fn path_to_str(path: &PathBuf) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| "path contains non-unicode characters".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_check_tools() {
        let result = parse_args(Vec::<String>::new());
        assert!(result.is_ok(), "{result:?}");
        let Ok(mode) = result else { return };
        assert_eq!(mode, Mode::CheckTools);
    }

    #[test]
    fn parses_probe_target() {
        let result = parse_args(["probe".into(), "10.0.0.1:443".into()]);
        assert!(result.is_ok(), "{result:?}");
        let Ok(mode) = result else { return };
        assert_eq!(
            mode,
            Mode::Probe {
                target: "10.0.0.1:443".into()
            }
        );
    }
}
