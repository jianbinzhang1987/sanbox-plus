use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditEventType {
    ServiceStarted,
    PolicyApplied,
    SandboxStarted,
    SandboxStopped,
    DesktopSwitched,
    AppLaunched,
    AppBlocked,
    NetworkAllowed,
    NetworkBlocked,
    FileImported,
    FileExportRequested,
    ClipboardBlocked,
    PrintBlocked,
    TunnelDisconnected,
    DegradedModeEnabled,
    SecurityCheckFailed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: Uuid,
    pub event_type: AuditEventType,
    pub timestamp_utc: String,
    pub user_sid: String,
    pub device_id: String,
    pub sandbox_id: Option<String>,
    pub policy_version: Option<String>,
    pub result: AuditResult,
    pub details: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditResult {
    Success,
    Failure,
    Blocked,
    Degraded,
}
