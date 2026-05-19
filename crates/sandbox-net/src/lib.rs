use sandbox_common::{NetworkPolicy, Result, SandboxApp, SandboxError, SandboxId};
use std::process::Command;

pub fn validate_network_policy(policy: &NetworkPolicy) -> Result<()> {
    if !policy.block_public_internet {
        return Err(SandboxError::PolicyValidation(
            "public internet must remain blocked".to_string(),
        ));
    }

    Ok(())
}

pub trait NetworkEnforcer: Send + Sync {
    fn apply_policy(
        &self,
        sandbox_id: &SandboxId,
        policy: &NetworkPolicy,
        apps: &[SandboxApp],
    ) -> Result<()>;
    fn clear_policy(&self, sandbox_id: &SandboxId) -> Result<()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct WfpNetworkEnforcer;

impl NetworkEnforcer for WfpNetworkEnforcer {
    fn apply_policy(
        &self,
        sandbox_id: &SandboxId,
        policy: &NetworkPolicy,
        apps: &[SandboxApp],
    ) -> Result<()> {
        validate_network_policy(policy)?;
        platform::apply_policy(sandbox_id, policy, apps)
    }

    fn clear_policy(&self, sandbox_id: &SandboxId) -> Result<()> {
        platform::clear_policy(sandbox_id)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopNetworkEnforcer;

impl NetworkEnforcer for NoopNetworkEnforcer {
    fn apply_policy(
        &self,
        _sandbox_id: &SandboxId,
        policy: &NetworkPolicy,
        _apps: &[SandboxApp],
    ) -> Result<()> {
        validate_network_policy(policy)
    }

    fn clear_policy(&self, _sandbox_id: &SandboxId) -> Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use super::*;

    pub fn apply_policy(
        sandbox_id: &SandboxId,
        policy: &NetworkPolicy,
        apps: &[SandboxApp],
    ) -> Result<()> {
        clear_policy(sandbox_id)?;

        for app in apps {
            let program = app.exe_path.display().to_string();

            if policy.block_public_internet {
                for (index, range) in public_ipv4_ranges().iter().enumerate() {
                    add_rule(
                        rule_name(
                            sandbox_id,
                            &format!("{}-block-public-ipv4-{index}", sanitize(&app.id)),
                        ),
                        "out",
                        "block",
                        Some(&program),
                        range,
                    )?;
                }
            }

            if policy.block_ipv6 {
                add_rule(
                    rule_name(
                        sandbox_id,
                        &format!("{}-block-public-ipv6", sanitize(&app.id)),
                    ),
                    "out",
                    "block",
                    Some(&program),
                    "2000::/3",
                )?;
            }

            for cidr in &policy.intranet_cidrs {
                add_rule(
                    rule_name(
                        sandbox_id,
                        &format!("{}-allow-intranet-{}", sanitize(&app.id), sanitize(cidr)),
                    ),
                    "out",
                    "allow",
                    Some(&program),
                    cidr,
                )?;
            }

            for dns in &policy.dns_servers {
                add_rule(
                    rule_name(
                        sandbox_id,
                        &format!("{}-allow-dns-{}", sanitize(&app.id), sanitize(dns)),
                    ),
                    "out",
                    "allow",
                    Some(&program),
                    dns,
                )?;
            }
        }

        Ok(())
    }

    pub fn clear_policy(sandbox_id: &SandboxId) -> Result<()> {
        let prefix = rule_name(sandbox_id, "");
        run_netsh_allow_failure([
            "advfirewall".to_string(),
            "firewall".to_string(),
            "delete".to_string(),
            "rule".to_string(),
            format!("name={prefix}*"),
        ])
    }

    fn add_rule(
        name: String,
        dir: &str,
        action: &str,
        program: Option<&str>,
        remote_ip: &str,
    ) -> Result<()> {
        let mut args = vec![
            "advfirewall".to_string(),
            "firewall".to_string(),
            "add".to_string(),
            "rule".to_string(),
            format!("name={name}"),
            format!("dir={dir}"),
            format!("action={action}"),
            "enable=yes".to_string(),
            "profile=any".to_string(),
            format!("remoteip={remote_ip}"),
        ];
        if let Some(program) = program {
            args.push(format!("program={program}"));
        }
        run_netsh(args)
    }

    fn run_netsh(args: impl IntoIterator<Item = String>) -> Result<()> {
        let output = Command::new("netsh")
            .args(args)
            .output()
            .map_err(|error| SandboxError::System(format!("failed to run netsh: {error}")))?;

        if !output.status.success() {
            return Err(SandboxError::System(format!(
                "netsh failed with status {}: {}{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    fn run_netsh_allow_failure(args: impl IntoIterator<Item = String>) -> Result<()> {
        Command::new("netsh")
            .args(args)
            .output()
            .map_err(|error| SandboxError::System(format!("failed to run netsh: {error}")))?;
        Ok(())
    }

    fn rule_name(sandbox_id: &SandboxId, suffix: &str) -> String {
        if suffix.is_empty() {
            format!("SandboxPlus-{}", sanitize(&sandbox_id.0))
        } else {
            format!("SandboxPlus-{}-{suffix}", sanitize(&sandbox_id.0))
        }
    }

    fn sanitize(value: &str) -> String {
        value
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
            .collect()
    }

    fn public_ipv4_ranges() -> &'static [&'static str] {
        &[
            "0.0.0.0-9.255.255.255",
            "11.0.0.0-126.255.255.255",
            "128.0.0.0-169.253.255.255",
            "169.255.0.0-172.15.255.255",
            "172.32.0.0-191.255.255.255",
            "192.0.0.0-192.167.255.255",
            "192.169.0.0-223.255.255.255",
        ]
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn apply_policy(
        _sandbox_id: &SandboxId,
        _policy: &NetworkPolicy,
        _apps: &[SandboxApp],
    ) -> Result<()> {
        Err(SandboxError::UnsupportedPlatform(
            "WFP firewall rule enforcement is only available on Windows".to_string(),
        ))
    }

    pub fn clear_policy(_sandbox_id: &SandboxId) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_enforcer_validates_policy() {
        let enforcer = NoopNetworkEnforcer;
        let result = enforcer.apply_policy(
            &SandboxId("test".to_string()),
            &NetworkPolicy::default(),
            &[],
        );

        assert!(result.is_ok());
    }

    #[test]
    #[cfg(not(windows))]
    fn wfp_enforcer_is_windows_only() {
        let enforcer = WfpNetworkEnforcer;
        let err = enforcer
            .apply_policy(
                &SandboxId("test".to_string()),
                &NetworkPolicy::default(),
                &[],
            )
            .expect_err("WFP rules are Windows-only");

        assert!(matches!(err, SandboxError::UnsupportedPlatform(_)));
    }
}
