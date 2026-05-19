use crate::{Result, SandboxError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxPolicy {
    pub version: String,
    #[serde(default)]
    pub apps: Vec<SandboxApp>,
    #[serde(default)]
    pub network: NetworkPolicy,
    #[serde(default)]
    pub filesystem: FilesystemPolicy,
    #[serde(default)]
    pub clipboard: ClipboardPolicy,
    #[serde(default)]
    pub printing: PrintingPolicy,
    #[serde(default)]
    pub audit: AuditPolicy,
}

impl SandboxPolicy {
    pub fn from_json(input: &str) -> Result<Self> {
        let policy: Self = serde_json::from_str(input)?;
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version.trim().is_empty() {
            return Err(SandboxError::PolicyValidation(
                "policy version must not be empty".to_string(),
            ));
        }

        for app in &self.apps {
            app.validate()?;
        }

        if !self.network.block_public_internet {
            return Err(SandboxError::PolicyValidation(
                "network.block_public_internet must be true by default".to_string(),
            ));
        }

        if self.filesystem.allow_export {
            return Err(SandboxError::PolicyValidation(
                "filesystem.allow_export must remain false for TASK-0002".to_string(),
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxApp {
    pub id: String,
    pub name: String,
    pub exe_path: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default)]
    pub icon_path: Option<PathBuf>,
    #[serde(default)]
    pub hash_sha256: Option<String>,
    #[serde(default)]
    pub signer_thumbprint: Option<String>,
    pub network_profile: NetworkProfile,
    #[serde(default)]
    pub auto_start: bool,
}

impl SandboxApp {
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            return Err(SandboxError::PolicyValidation(
                "app id must not be empty".to_string(),
            ));
        }

        if self.name.trim().is_empty() {
            return Err(SandboxError::PolicyValidation(
                "app name must not be empty".to_string(),
            ));
        }

        if path_is_empty(&self.exe_path) {
            return Err(SandboxError::PolicyValidation(format!(
                "app '{}' exe_path must not be empty",
                self.id
            )));
        }

        if self.hash_sha256.is_none() && self.signer_thumbprint.is_none() {
            return Err(SandboxError::PolicyValidation(format!(
                "app '{}' must define hash_sha256 or signer_thumbprint",
                self.id
            )));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkProfile {
    IntranetOnly,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkPolicy {
    #[serde(default)]
    pub intranet_cidrs: Vec<String>,
    #[serde(default)]
    pub dns_servers: Vec<String>,
    pub block_ipv6: bool,
    pub block_public_internet: bool,
    pub block_unapproved_proxy: bool,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            intranet_cidrs: Vec::new(),
            dns_servers: Vec::new(),
            block_ipv6: true,
            block_public_internet: true,
            block_unapproved_proxy: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    pub sandbox_root: PathBuf,
    pub allow_import: bool,
    pub allow_export: bool,
    #[serde(default)]
    pub denied_host_paths: Vec<PathBuf>,
}

impl Default for FilesystemPolicy {
    fn default() -> Self {
        Self {
            sandbox_root: PathBuf::from(r"C:\SandboxData"),
            allow_import: false,
            allow_export: false,
            denied_host_paths: vec![PathBuf::from(r"C:\Users"), PathBuf::from(r"C:\ProgramData")],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardPolicy {
    pub allow_cross_boundary: bool,
    pub allow_plain_text_only: bool,
}

impl Default for ClipboardPolicy {
    fn default() -> Self {
        Self {
            allow_cross_boundary: false,
            allow_plain_text_only: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintingPolicy {
    pub allow_printing: bool,
    pub require_watermark: bool,
}

impl Default for PrintingPolicy {
    fn default() -> Self {
        Self {
            allow_printing: false,
            require_watermark: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditPolicy {
    pub enabled: bool,
    pub fail_closed_on_write_error: bool,
    pub local_retention_days: u32,
}

impl Default for AuditPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            fail_closed_on_write_error: true,
            local_retention_days: 30,
        }
    }
}

fn path_is_empty(path: &Path) -> bool {
    path.as_os_str().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_fail_closed() {
        let json = r#"{"version":"2026.05.18"}"#;

        let policy = SandboxPolicy::from_json(json).expect("policy should be valid");

        assert!(policy.network.block_public_internet);
        assert!(policy.network.block_ipv6);
        assert!(policy.network.block_unapproved_proxy);
        assert!(!policy.filesystem.allow_export);
        assert!(!policy.clipboard.allow_cross_boundary);
        assert!(!policy.printing.allow_printing);
        assert!(policy.audit.fail_closed_on_write_error);
    }

    #[test]
    fn policy_rejects_empty_version() {
        let json = r#"{"version":" "}"#;

        let err = SandboxPolicy::from_json(json).expect_err("policy should fail");

        assert!(matches!(err, SandboxError::PolicyValidation(_)));
    }

    #[test]
    fn policy_rejects_public_internet_enabled() {
        let json = r#"{
            "version":"2026.05.18",
            "network": {
                "intranet_cidrs": [],
                "dns_servers": [],
                "block_ipv6": true,
                "block_public_internet": false,
                "block_unapproved_proxy": true
            }
        }"#;

        let err = SandboxPolicy::from_json(json).expect_err("policy should fail closed");

        assert!(matches!(err, SandboxError::PolicyValidation(_)));
    }

    #[test]
    fn policy_deserializes_valid_app() {
        let json = r#"{
            "version":"2026.05.18",
            "apps":[{
                "id":"oa",
                "name":"OA",
                "exe_path":"C:\\Program Files\\OA\\oa.exe",
                "hash_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "network_profile":"IntranetOnly"
            }]
        }"#;

        let policy = SandboxPolicy::from_json(json).expect("policy should be valid");

        assert_eq!(policy.apps.len(), 1);
        assert_eq!(policy.apps[0].id, "oa");
        assert_eq!(policy.apps[0].network_profile, NetworkProfile::IntranetOnly);
    }

    #[test]
    fn app_requires_hash_or_signer() {
        let json = r#"{
            "version":"2026.05.18",
            "apps":[{
                "id":"oa",
                "name":"OA",
                "exe_path":"C:\\Program Files\\OA\\oa.exe",
                "network_profile":"IntranetOnly"
            }]
        }"#;

        let err = SandboxPolicy::from_json(json).expect_err("app should require identity proof");

        assert!(matches!(err, SandboxError::PolicyValidation(_)));
    }
}
