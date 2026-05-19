use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SandboxId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxState {
    Stopped,
    Starting,
    Ready,
    Running,
    Degraded,
    Stopping,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxInstance {
    pub id: SandboxId,
    pub user_sid: String,
    pub desktop_name: String,
    pub profile_root: PathBuf,
    pub state: SandboxState,
    pub policy_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxStatus {
    pub instance: Option<SandboxInstance>,
    pub health: ServiceHealth,
    pub active_policy_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceHealth {
    Healthy,
    Degraded(String),
    FailClosed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub process_id: u32,
    pub app_id: Option<String>,
    pub executable: PathBuf,
    pub state: ProcessState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessState {
    Starting,
    Running,
    Exited { exit_code: Option<u32> },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub sandbox_id: Option<SandboxId>,
    pub user_sid: String,
    pub desktop_name: Option<String>,
    pub profile_root: Option<PathBuf>,
    pub policy_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSessionResponse {
    pub instance: SandboxInstance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSessionStateRequest {
    pub sandbox_id: SandboxId,
    pub state: SandboxState,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachSessionRequest {
    pub sandbox_id: SandboxId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseSessionRequest {
    pub sandbox_id: SandboxId,
    pub mode: CloseSessionMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseSessionMode {
    KeepProfile,
    ResetProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchAppRequest {
    pub sandbox_id: SandboxId,
    pub app_id: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    pub desktop_name: String,
    pub profile_root: PathBuf,
    #[serde(default)]
    pub environment_overrides: Vec<EnvironmentVariable>,
    pub policy_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentVariable {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchAppResponse {
    pub process: ProcessInfo,
    pub restricted_token_applied: bool,
    pub profile_applied: bool,
    pub job_assigned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSummary {
    pub id: String,
    pub name: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    pub auto_start: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicySummary {
    pub version: String,
    pub app_count: usize,
    pub network_mode: NetworkPolicyMode,
    pub clipboard_mode: ClipboardPolicyMode,
    pub import_allowed: bool,
    pub export_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkPolicyMode {
    Blocked,
    IntranetOnly,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClipboardPolicyMode {
    Blocked,
    PlainTextOnly,
    Prompt,
    Allowed,
}
