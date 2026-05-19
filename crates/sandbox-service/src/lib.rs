use sandbox_audit::{AuditSink, JsonlAuditSink};
use sandbox_common::{
    AppSummary, AuditEvent, AuditEventType, AuditResult, CloseSessionMode, CloseSessionRequest,
    CreateSessionRequest, CreateSessionResponse, LaunchAppRequest, LaunchAppResponse,
    NetworkPolicyMode, PolicySummary, ProcessInfo, ProcessState, Result, SandboxError, SandboxId,
    SandboxInstance, SandboxPolicy, SandboxState, SandboxStatus, ServiceHealth,
    UpdateSessionStateRequest,
};
use sandbox_ipc::{IpcRequest, IpcResponse};
use sandbox_launcher::{LaunchCredentials, LauncherOptions};
use sandbox_net::NetworkEnforcer;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const DEFAULT_POLICY_VERSION: &str = "development";

pub struct SandboxService {
    policy: SandboxPolicy,
    instance: Option<SandboxInstance>,
    processes: HashMap<u32, ProcessInfo>,
    job: Option<JobHandle>,
    audit: JsonlAuditSink,
    network: Box<dyn NetworkEnforcer>,
    dedicated_user: Option<DedicatedSandboxUser>,
}

impl SandboxService {
    pub fn new(policy: SandboxPolicy) -> Result<Self> {
        Self::with_network_enforcer(policy, default_network_enforcer())
    }

    pub fn with_network_enforcer(
        policy: SandboxPolicy,
        network: Box<dyn NetworkEnforcer>,
    ) -> Result<Self> {
        policy.validate()?;

        Ok(Self {
            policy,
            instance: None,
            processes: HashMap::new(),
            job: None,
            audit: JsonlAuditSink::new(r"C:\ProgramData\SandboxPlus\Logs\audit.jsonl"),
            network,
            dedicated_user: None,
        })
    }

    pub fn development() -> Result<Self> {
        Self::new(SandboxPolicy {
            version: DEFAULT_POLICY_VERSION.to_string(),
            apps: Vec::new(),
            network: Default::default(),
            filesystem: Default::default(),
            clipboard: Default::default(),
            printing: Default::default(),
            audit: Default::default(),
        })
    }

    pub fn from_policy_file(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let path = path.as_ref();
        let input = std::fs::read_to_string(path).map_err(|error| {
            SandboxError::Configuration(format!(
                "failed to read policy file '{}': {error}",
                path.display()
            ))
        })?;
        let policy = SandboxPolicy::from_json(&input)?;
        Self::new(policy)
    }

    pub fn get_status(&self) -> SandboxStatus {
        SandboxStatus {
            instance: self.instance.clone(),
            health: match &self.instance {
                Some(instance) if instance.state == SandboxState::Degraded => {
                    ServiceHealth::Degraded("sandbox session is degraded".to_string())
                }
                Some(_) => ServiceHealth::Healthy,
                None => ServiceHealth::FailClosed("no active sandbox session".to_string()),
            },
            active_policy_version: Some(self.policy.version.clone()),
        }
    }

    pub fn handle_request(&mut self, request: IpcRequest) -> IpcResponse {
        match self.try_handle_request(request) {
            Ok(response) => response,
            Err(error) => IpcResponse::from_error(error),
        }
    }

    fn try_handle_request(&mut self, request: IpcRequest) -> Result<IpcResponse> {
        match request {
            IpcRequest::CreateSession(request) => {
                Ok(IpcResponse::SessionCreated(self.create_session(request)?))
            }
            IpcRequest::AttachSession(request) => {
                self.require_instance(&request.sandbox_id)?;
                Ok(IpcResponse::Status(self.get_status()))
            }
            IpcRequest::UpdateSessionState(request) => {
                self.update_session_state(request)?;
                Ok(IpcResponse::Status(self.get_status()))
            }
            IpcRequest::CloseSession(request) => {
                self.close_session(request)?;
                Ok(IpcResponse::Ok)
            }
            IpcRequest::ResetSession(sandbox_id) => {
                self.close_session(CloseSessionRequest {
                    sandbox_id,
                    mode: CloseSessionMode::ResetProfile,
                })?;
                Ok(IpcResponse::Ok)
            }
            IpcRequest::GetStatus => Ok(IpcResponse::Status(self.get_status())),
            IpcRequest::ListApps { sandbox_id } => {
                self.require_instance(&sandbox_id)?;
                Ok(IpcResponse::Apps(self.list_apps()))
            }
            IpcRequest::LaunchApp(request) => Ok(IpcResponse::LaunchApp(self.launch_app(request)?)),
            IpcRequest::RecordProcess {
                sandbox_id,
                process,
            } => {
                self.record_process(&sandbox_id, process)?;
                Ok(IpcResponse::Ok)
            }
            IpcRequest::ListProcesses { sandbox_id } => {
                Ok(IpcResponse::Processes(self.list_processes(&sandbox_id)?))
            }
            IpcRequest::GetPolicySummary { sandbox_id } => {
                self.require_instance(&sandbox_id)?;
                Ok(IpcResponse::PolicySummary(self.get_policy_summary()))
            }
            IpcRequest::ReturnToHost { sandbox_id } => {
                self.require_instance(&sandbox_id)?;
                sandbox_desktop::switch_to_default_desktop()?;
                Ok(IpcResponse::Ok)
            }
            IpcRequest::ExportDiagnosticBundle { sandbox_id } => {
                self.require_instance(&sandbox_id)?;
                Err(SandboxError::UnsupportedPlatform(
                    "diagnostic bundle export is not implemented yet".to_string(),
                ))
            }
        }
    }

    pub fn create_session(
        &mut self,
        request: CreateSessionRequest,
    ) -> Result<CreateSessionResponse> {
        if let Some(instance) = &self.instance {
            if !matches!(instance.state, SandboxState::Stopped | SandboxState::Error) {
                return Err(SandboxError::Denied(format!(
                    "sandbox session '{}' is already active",
                    instance.id.0
                )));
            }
        }

        let id = request
            .sandbox_id
            .unwrap_or_else(|| SandboxId(format!("sandbox-{}", Uuid::new_v4())));
        let desktop_name = request
            .desktop_name
            .unwrap_or_else(|| format!("Sandbox-{}", id.0));
        let profile_root = request
            .profile_root
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData\SandboxPlus\Profiles").join(&id.0));
        let policy_version = request
            .policy_version
            .unwrap_or_else(|| self.policy.version.clone());

        if request.user_sid.trim().is_empty() {
            return Err(SandboxError::Configuration(
                "create session requires user_sid".to_string(),
            ));
        }

        let dedicated_user = if dedicated_user_requested(&request.user_sid) {
            Some(ensure_dedicated_sandbox_user()?)
        } else {
            None
        };
        let user_sid = dedicated_user
            .as_ref()
            .map(|user| user.sid.clone())
            .unwrap_or(request.user_sid);

        prepare_profile(&profile_root)?;
        prepare_explorer_workspace(&profile_root, &self.policy.apps)?;
        write_explorer_policy_reg(&profile_root)?;
        apply_profile_acl(&profile_root, &user_sid)?;
        sandbox_desktop::ensure_desktop(&desktop_name)?;
        sandbox_desktop::grant_desktop_access(&desktop_name, &user_sid)?;
        self.network
            .apply_policy(&id, &self.policy.network, &self.policy.apps)?;
        let job = JobHandle::create(&id.0)?;

        let instance = SandboxInstance {
            id,
            user_sid,
            desktop_name,
            profile_root,
            state: SandboxState::Ready,
            policy_version,
        };

        self.instance = Some(instance.clone());
        self.dedicated_user = dedicated_user;
        self.processes.clear();
        self.job = Some(job);
        self.write_audit(
            AuditEventType::SandboxStarted,
            AuditResult::Success,
            Some(&instance),
            serde_json::json!({
                "desktop": instance.desktop_name,
                "profile_root": instance.profile_root,
            }),
        )?;

        Ok(CreateSessionResponse { instance })
    }

    pub fn close_session(&mut self, request: CloseSessionRequest) -> Result<()> {
        let instance = self.require_instance(&request.sandbox_id)?.clone();

        if matches!(request.mode, CloseSessionMode::ResetProfile) {
            // Profile deletion is intentionally deferred to the filesystem broker.
            // The state transition is recorded here so callers can build the flow.
        }

        self.cleanup_active_resources(&instance);
        self.instance = Some(SandboxInstance {
            state: SandboxState::Stopped,
            ..instance.clone()
        });
        if matches!(request.mode, CloseSessionMode::ResetProfile) {
            remove_profile_dir(&instance.profile_root)?;
        }
        self.write_audit(
            AuditEventType::SandboxStopped,
            AuditResult::Success,
            Some(&instance),
            serde_json::json!({ "mode": format!("{:?}", request.mode) }),
        )?;
        Ok(())
    }

    pub fn update_session_state(&mut self, request: UpdateSessionStateRequest) -> Result<()> {
        let instance = self.require_instance(&request.sandbox_id)?;

        if matches!(request.state, SandboxState::Stopped | SandboxState::Error)
            && request.reason.as_deref().unwrap_or("").trim().is_empty()
        {
            return Err(SandboxError::Configuration(
                "terminal session states require a reason".to_string(),
            ));
        }

        self.instance = Some(SandboxInstance {
            state: request.state,
            ..instance.clone()
        });
        self.write_audit(
            AuditEventType::DesktopSwitched,
            AuditResult::Success,
            self.instance.as_ref(),
            serde_json::json!({ "reason": request.reason }),
        )?;
        Ok(())
    }

    pub fn list_apps(&self) -> Vec<AppSummary> {
        self.policy
            .apps
            .iter()
            .map(|app| AppSummary {
                id: app.id.clone(),
                name: app.name.clone(),
                executable: app.exe_path.clone(),
                arguments: app.args.clone(),
                working_directory: app.working_dir.clone(),
                auto_start: app.auto_start,
            })
            .collect()
    }

    pub fn list_processes(&mut self, sandbox_id: &SandboxId) -> Result<Vec<ProcessInfo>> {
        self.require_instance(sandbox_id)?;
        self.refresh_process_states();
        Ok(self.processes.values().cloned().collect())
    }

    pub fn record_process(&mut self, sandbox_id: &SandboxId, process: ProcessInfo) -> Result<()> {
        self.require_instance(sandbox_id)?;
        if process.process_id == 0 {
            return Err(SandboxError::Configuration(
                "cannot record process with pid 0".to_string(),
            ));
        }
        if let Some(job) = &self.job {
            job.assign_process(process.process_id)?;
        }
        self.processes.insert(process.process_id, process);
        Ok(())
    }

    pub fn get_policy_summary(&self) -> PolicySummary {
        PolicySummary {
            version: self.policy.version.clone(),
            app_count: self.policy.apps.len(),
            network_mode: NetworkPolicyMode::IntranetOnly,
            clipboard_mode: if self.policy.clipboard.allow_cross_boundary {
                sandbox_common::ClipboardPolicyMode::Allowed
            } else {
                sandbox_common::ClipboardPolicyMode::Blocked
            },
            import_allowed: self.policy.filesystem.allow_import,
            export_allowed: self.policy.filesystem.allow_export,
        }
    }

    pub fn launch_app(&mut self, request: LaunchAppRequest) -> Result<LaunchAppResponse> {
        let instance = self.require_instance(&request.sandbox_id)?;

        if request.desktop_name != instance.desktop_name {
            return Err(SandboxError::Denied(format!(
                "launch desktop '{}' does not match active desktop '{}'",
                request.desktop_name, instance.desktop_name
            )));
        }

        let allowed = self.policy.apps.iter().any(|app| {
            app.id == request.app_id
                && app.exe_path == request.executable
                && app.network_profile == sandbox_common::NetworkProfile::IntranetOnly
        });

        if !allowed {
            return Err(SandboxError::Denied(format!(
                "app '{}' is not allowed by active policy",
                request.app_id
            )));
        }

        let response = sandbox_launcher::launch_app(request, self.launcher_options_for_instance())?;

        if response.process.process_id != 0 {
            self.processes
                .insert(response.process.process_id, response.process.clone());
        }
        self.write_audit(
            AuditEventType::AppLaunched,
            AuditResult::Success,
            self.instance.as_ref(),
            serde_json::json!({
                "pid": response.process.process_id,
                "app_id": response.process.app_id,
                "executable": response.process.executable,
            }),
        )?;

        Ok(response)
    }

    pub fn shutdown(&mut self) {
        if let Some(instance) = self.instance.clone() {
            if !matches!(instance.state, SandboxState::Stopped | SandboxState::Error) {
                self.cleanup_active_resources(&instance);
                self.instance = Some(SandboxInstance {
                    state: SandboxState::Stopped,
                    ..instance.clone()
                });
                let _ = self.write_audit(
                    AuditEventType::SandboxStopped,
                    AuditResult::Success,
                    Some(&instance),
                    serde_json::json!({ "mode": "ServiceShutdown" }),
                );
            }
        }
    }

    fn launcher_options_for_instance(&self) -> LauncherOptions {
        LauncherOptions {
            dry_run: false,
            credentials: self.dedicated_user.as_ref().map(|user| LaunchCredentials {
                username: user.name.clone(),
                domain: Some(".".to_string()),
                password: user.password.clone(),
            }),
            job_handle: self.job.as_ref().map(|job| job.raw),
        }
    }

    fn cleanup_active_resources(&mut self, instance: &SandboxInstance) {
        self.refresh_process_states();
        for process in self.processes.values() {
            if process.process_id != 0 && !matches!(process.state, ProcessState::Exited { .. }) {
                let _ = terminate_process(process.process_id);
            }
        }
        if let Some(job) = &self.job {
            let _ = job.terminate();
        }
        let _ = self.network.clear_policy(&instance.id);
        let _ = cleanup_profile_temporary_state(&instance.profile_root);
        self.dedicated_user = None;
        self.processes.clear();
        self.job = None;
    }

    fn require_instance(&self, sandbox_id: &SandboxId) -> Result<&SandboxInstance> {
        let instance = self
            .instance
            .as_ref()
            .ok_or_else(|| SandboxError::Denied("no active sandbox session".to_string()))?;

        if &instance.id != sandbox_id {
            return Err(SandboxError::Denied(format!(
                "sandbox session '{}' is not active",
                sandbox_id.0
            )));
        }

        Ok(instance)
    }

    fn refresh_process_states(&mut self) {
        for process in self.processes.values_mut() {
            if process.process_id == 0 || matches!(process.state, ProcessState::Exited { .. }) {
                continue;
            }
            if let Some(exit_code) = process_exit_code(process.process_id) {
                process.state = ProcessState::Exited { exit_code };
            }
        }
    }

    fn write_audit(
        &self,
        event_type: AuditEventType,
        result: AuditResult,
        instance: Option<&SandboxInstance>,
        details: serde_json::Value,
    ) -> Result<()> {
        let event = AuditEvent {
            event_id: Uuid::new_v4(),
            event_type,
            timestamp_utc: unix_timestamp_string(),
            user_sid: instance
                .map(|instance| instance.user_sid.clone())
                .unwrap_or_else(|| "unknown".to_string()),
            device_id: "local-device".to_string(),
            sandbox_id: instance.map(|instance| instance.id.0.clone()),
            policy_version: Some(self.policy.version.clone()),
            result,
            details,
        };
        self.audit.write_event(event)
    }
}

impl Drop for SandboxService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn default_network_enforcer() -> Box<dyn NetworkEnforcer> {
    #[cfg(test)]
    {
        Box::new(sandbox_net::NoopNetworkEnforcer)
    }
    #[cfg(not(test))]
    {
        Box::new(sandbox_net::WfpNetworkEnforcer)
    }
}

fn unix_timestamp_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

fn prepare_profile(profile_root: &std::path::Path) -> Result<()> {
    for relative in [
        "",
        "AppData",
        "AppData\\Local",
        "AppData\\Local\\Temp",
        "AppData\\Roaming",
        "AppData\\Roaming\\Microsoft",
        "AppData\\Roaming\\Microsoft\\Windows",
        "AppData\\Roaming\\Microsoft\\Windows\\Start Menu",
        "AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs",
        "Temp",
        "Desktop",
        "Documents",
        "Downloads",
    ] {
        std::fs::create_dir_all(profile_root.join(relative)).map_err(|error| {
            SandboxError::System(format!(
                "failed to create profile directory '{}': {error}",
                profile_root.join(relative).display()
            ))
        })?;
    }
    Ok(())
}

fn prepare_explorer_workspace(
    profile_root: &std::path::Path,
    apps: &[sandbox_common::SandboxApp],
) -> Result<()> {
    let desktop_dir = profile_root.join("Desktop");
    let start_menu_dir = profile_root
        .join("AppData")
        .join("Roaming")
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");

    for app in apps {
        let filename = format!("{}.cmd", sanitize_shortcut_name(&app.name));
        let launcher = build_cmd_launcher(&app.exe_path, &app.args);
        for dir in [&desktop_dir, &start_menu_dir] {
            let path = dir.join(&filename);
            std::fs::write(&path, &launcher).map_err(|error| {
                SandboxError::System(format!(
                    "failed to create sandbox app launcher '{}': {error}",
                    path.display()
                ))
            })?;
        }
    }

    Ok(())
}

fn write_explorer_policy_reg(profile_root: &std::path::Path) -> Result<()> {
    let path = profile_root.join("sandbox-explorer-policies.reg");
    let content = r#"Windows Registry Editor Version 5.00

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer]
"NoRun"=dword:00000001
"NoControlPanel"=dword:00000001
"NoWindowsUpdate"=dword:00000001
"NoClose"=dword:00000001
"NoStartMenuSubFolders"=dword:00000001
"NoCommonGroups"=dword:00000001

[HKEY_CURRENT_USER\Software\Policies\Microsoft\Windows\System]
"DisableCMD"=dword:00000001
"DisableRegistryTools"=dword:00000001

[HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Policies\System]
"DisableTaskMgr"=dword:00000001
"#;
    std::fs::write(&path, content).map_err(|error| {
        SandboxError::System(format!(
            "failed to write explorer policy template '{}': {error}",
            path.display()
        ))
    })
}

fn build_cmd_launcher(executable: &std::path::Path, args: &[String]) -> String {
    let mut command = format!(
        "start \"\" {}",
        quote_cmd_arg(&executable.display().to_string())
    );
    for arg in args {
        command.push(' ');
        command.push_str(&quote_cmd_arg(arg));
    }
    format!("@echo off\r\n{command}\r\n")
}

fn quote_cmd_arg(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn sanitize_shortcut_name(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect();
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        "Sandbox App".to_string()
    } else {
        trimmed.to_string()
    }
}

fn dedicated_user_requested(value: &str) -> bool {
    value.eq_ignore_ascii_case("auto")
        || value.eq_ignore_ascii_case("sandbox-plus")
        || value.eq_ignore_ascii_case("SandboxPlusUser")
        || value.eq_ignore_ascii_case("SandboxPlusUsr")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedicatedSandboxUser {
    pub name: String,
    pub sid: String,
    pub password: String,
}

pub fn ensure_dedicated_sandbox_user() -> Result<DedicatedSandboxUser> {
    platform_identity::ensure_dedicated_sandbox_user()
}

pub fn apply_profile_acl(profile_root: &std::path::Path, user_sid: &str) -> Result<()> {
    platform_identity::apply_profile_acl(profile_root, user_sid)
}

#[cfg(all(windows, not(test)))]
mod platform_identity {
    use super::{DedicatedSandboxUser, Result, SandboxError};
    use std::path::Path;
    use std::process::Command;

    const USER_NAME: &str = "SandboxPlusUsr";

    pub fn ensure_dedicated_sandbox_user() -> Result<DedicatedSandboxUser> {
        let random = uuid::Uuid::new_v4().simple().to_string();
        let password = format!("Sp!{}", &random[..10]);
        if !command_success("net", &["user", USER_NAME])? {
            run(
                "net",
                &[
                    "user",
                    USER_NAME,
                    &password,
                    "/add",
                    "/active:yes",
                    "/expires:never",
                    "/passwordchg:no",
                ],
            )?;
        } else {
            run("net", &["user", USER_NAME, &password])?;
        }

        let sid = query_user_sid(USER_NAME)?;
        Ok(DedicatedSandboxUser {
            name: USER_NAME.to_string(),
            sid,
            password,
        })
    }

    pub fn apply_profile_acl(profile_root: &Path, user_sid: &str) -> Result<()> {
        let root = profile_root.display().to_string();
        run("icacls", &[&root, "/inheritance:r"])?;
        run(
            "icacls",
            &[
                &root,
                "/grant:r",
                "SYSTEM:(OI)(CI)(F)",
                "Administrators:(OI)(CI)(F)",
                &format!("*{user_sid}:(OI)(CI)(F)"),
            ],
        )?;
        Ok(())
    }

    fn query_user_sid(name: &str) -> Result<String> {
        let output = Command::new("wmic")
            .args([
                "useraccount",
                "where",
                &format!("name='{name}'"),
                "get",
                "sid",
                "/value",
            ])
            .output()
            .map_err(|error| SandboxError::System(format!("failed to run wmic: {error}")))?;

        if !output.status.success() {
            return Err(SandboxError::System(format!(
                "wmic user sid query failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("SID=").map(str::trim))
            .filter(|sid| !sid.is_empty())
            .map(str::to_string)
            .ok_or_else(|| SandboxError::System(format!("failed to resolve SID for {name}")))
    }

    fn command_success(program: &str, args: &[&str]) -> Result<bool> {
        let status = Command::new(program)
            .args(args)
            .status()
            .map_err(|error| SandboxError::System(format!("failed to run {program}: {error}")))?;
        Ok(status.success())
    }

    fn run(program: &str, args: &[&str]) -> Result<()> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|error| SandboxError::System(format!("failed to run {program}: {error}")))?;

        if !output.status.success() {
            return Err(SandboxError::System(format!(
                "{program} failed with status {}: {}{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }
}

#[cfg(any(not(windows), test))]
mod platform_identity {
    use super::{DedicatedSandboxUser, Result};
    use std::path::Path;

    pub fn ensure_dedicated_sandbox_user() -> Result<DedicatedSandboxUser> {
        Ok(DedicatedSandboxUser {
            name: "SandboxPlusUsr".to_string(),
            sid: "S-1-5-21-SandboxPlusUsr".to_string(),
            password: "test-only".to_string(),
        })
    }

    pub fn apply_profile_acl(_profile_root: &Path, _user_sid: &str) -> Result<()> {
        Ok(())
    }
}

pub fn validate_profile_boundary(instance: &SandboxInstance) -> Result<()> {
    if !instance.profile_root.exists() {
        return Err(SandboxError::System(format!(
            "profile root does not exist: {}",
            instance.profile_root.display()
        )));
    }
    for required in ["AppData\\Local", "AppData\\Roaming", "Temp"] {
        let path = instance.profile_root.join(required);
        if !path.exists() {
            return Err(SandboxError::System(format!(
                "profile subdirectory missing: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn remove_profile_dir(profile_root: &std::path::Path) -> Result<()> {
    if profile_root.exists() {
        std::fs::remove_dir_all(profile_root).map_err(|error| {
            SandboxError::System(format!(
                "failed to remove profile directory '{}': {error}",
                profile_root.display()
            ))
        })?;
    }
    Ok(())
}

fn cleanup_profile_temporary_state(profile_root: &std::path::Path) -> Result<()> {
    for relative in ["Temp", r"AppData\Local\Temp"] {
        let path = profile_root.join(relative);
        if path.exists() {
            remove_dir_contents(&path)?;
        }
        std::fs::create_dir_all(&path).map_err(|error| {
            SandboxError::System(format!(
                "failed to recreate profile temp directory '{}': {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

fn remove_dir_contents(path: &std::path::Path) -> Result<()> {
    for entry in std::fs::read_dir(path).map_err(|error| {
        SandboxError::System(format!(
            "failed to enumerate directory '{}': {error}",
            path.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            SandboxError::System(format!(
                "failed to read directory entry under '{}': {error}",
                path.display()
            ))
        })?;
        let child = entry.path();
        let metadata = entry.metadata().map_err(|error| {
            SandboxError::System(format!(
                "failed to inspect profile temp path '{}': {error}",
                child.display()
            ))
        })?;
        if metadata.is_dir() {
            std::fs::remove_dir_all(&child).map_err(|error| {
                SandboxError::System(format!(
                    "failed to remove profile temp directory '{}': {error}",
                    child.display()
                ))
            })?;
        } else {
            std::fs::remove_file(&child).map_err(|error| {
                SandboxError::System(format!(
                    "failed to remove profile temp file '{}': {error}",
                    child.display()
                ))
            })?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn terminate_process(process_id: u32) -> Result<()> {
    use sandbox_common::SandboxError;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, process_id) };
    if process.is_null() {
        return Err(SandboxError::System(format!(
            "OpenProcess(PROCESS_TERMINATE) failed with Win32 error {}",
            unsafe { GetLastError() }
        )));
    }

    let ok = unsafe { TerminateProcess(process, 0) };
    unsafe {
        CloseHandle(process);
    }
    if ok == 0 {
        return Err(SandboxError::System(format!(
            "TerminateProcess failed with Win32 error {}",
            unsafe { GetLastError() }
        )));
    }

    Ok(())
}

#[cfg(windows)]
fn process_exit_code(process_id: u32) -> Option<Option<u32>> {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Some(None);
    }

    let mut exit_code = 0u32;
    let ok = unsafe { GetExitCodeProcess(process, &mut exit_code) };
    unsafe {
        CloseHandle(process);
    }
    if ok == 0 {
        return Some(None);
    }
    if exit_code == STILL_ACTIVE as u32 {
        None
    } else {
        Some(Some(exit_code))
    }
}

#[cfg(not(windows))]
fn process_exit_code(_process_id: u32) -> Option<Option<u32>> {
    None
}

#[cfg(not(windows))]
fn terminate_process(_process_id: u32) -> Result<()> {
    Ok(())
}

#[derive(Debug)]
struct JobHandle {
    raw: isize,
}

impl JobHandle {
    #[cfg(windows)]
    fn create(name: &str) -> Result<Self> {
        use std::mem::size_of;
        use std::ptr::null;
        use windows_sys::Win32::Foundation::GetLastError;
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        let name = wide_null(&format!("SandboxPlus-{name}"));
        let raw = unsafe { CreateJobObjectW(null(), name.as_ptr()) };
        if raw.is_null() {
            return Err(SandboxError::System(format!(
                "CreateJobObjectW failed with Win32 error {}",
                unsafe { GetLastError() }
            )));
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                raw,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(raw);
            }
            return Err(SandboxError::System(format!(
                "SetInformationJobObject(KILL_ON_JOB_CLOSE) failed with Win32 error {code}"
            )));
        }
        Ok(Self { raw: raw as isize })
    }

    #[cfg(not(windows))]
    fn create(_name: &str) -> Result<Self> {
        Ok(Self { raw: 0 })
    }

    #[cfg(windows)]
    fn assign_process(&self, process_id: u32) -> Result<()> {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, process_id) };
        if process.is_null() {
            return Err(SandboxError::System(format!(
                "OpenProcess(PROCESS_SET_QUOTA) failed with Win32 error {}",
                unsafe { GetLastError() }
            )));
        }
        let ok = unsafe { AssignProcessToJobObject(self.raw as _, process) };
        unsafe {
            CloseHandle(process);
        }
        if ok == 0 {
            return Err(SandboxError::System(format!(
                "AssignProcessToJobObject failed with Win32 error {}",
                unsafe { GetLastError() }
            )));
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn assign_process(&self, _process_id: u32) -> Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn terminate(&self) -> Result<()> {
        use windows_sys::Win32::Foundation::GetLastError;
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        let ok = unsafe { TerminateJobObject(self.raw as _, 0) };
        if ok == 0 {
            return Err(SandboxError::System(format!(
                "TerminateJobObject failed with Win32 error {}",
                unsafe { GetLastError() }
            )));
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn terminate(&self) -> Result<()> {
        Ok(())
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.raw != 0 {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.raw as _);
            }
        }
    }
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn get_status() -> Result<SandboxStatus> {
    Ok(SandboxService::development()?.get_status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sandbox_common::{NetworkProfile, SandboxApp};
    use sandbox_ipc::IpcResponse;

    #[test]
    fn development_service_fails_closed_without_session() {
        let service = SandboxService::development().expect("service should initialize");
        let status = service.get_status();

        assert!(status.instance.is_none());
        assert!(matches!(status.health, ServiceHealth::FailClosed(_)));
    }

    #[test]
    fn create_session_returns_ready_instance() {
        let mut service = SandboxService::development().expect("service should initialize");

        let response = service
            .create_session(CreateSessionRequest {
                sandbox_id: Some(SandboxId("test".to_string())),
                user_sid: "S-1-5-21-test".to_string(),
                desktop_name: Some("Sandbox-Test".to_string()),
                profile_root: Some(PathBuf::from(r"C:\SandboxPlus\Test")),
                policy_version: None,
            })
            .expect("session should be created");

        assert_eq!(response.instance.id.0, "test");
        assert_eq!(response.instance.desktop_name, "Sandbox-Test");
        assert_eq!(response.instance.state, SandboxState::Ready);
    }

    #[test]
    fn handle_request_dispatches_get_status() {
        let mut service = SandboxService::development().expect("service should initialize");

        let response = service.handle_request(IpcRequest::GetStatus);

        assert!(matches!(response, IpcResponse::Status(_)));
    }

    #[test]
    fn handle_request_maps_errors_to_ipc_response() {
        let mut service = SandboxService::development().expect("service should initialize");

        let response = service.handle_request(IpcRequest::ListProcesses {
            sandbox_id: SandboxId("missing".to_string()),
        });

        assert!(matches!(response, IpcResponse::Error(_)));
    }

    #[test]
    fn create_session_rejects_empty_user_sid() {
        let mut service = SandboxService::development().expect("service should initialize");

        let err = service
            .create_session(CreateSessionRequest {
                sandbox_id: None,
                user_sid: " ".to_string(),
                desktop_name: None,
                profile_root: None,
                policy_version: None,
            })
            .expect_err("empty user sid should be rejected");

        assert!(matches!(err, SandboxError::Configuration(_)));
    }

    #[test]
    fn create_session_rejects_duplicate_active_session() {
        let mut service = SandboxService::development().expect("service should initialize");

        service
            .create_session(CreateSessionRequest {
                sandbox_id: Some(SandboxId("one".to_string())),
                user_sid: "S-1-5-21-test".to_string(),
                desktop_name: None,
                profile_root: None,
                policy_version: None,
            })
            .expect("first session should be created");

        let err = service
            .create_session(CreateSessionRequest {
                sandbox_id: Some(SandboxId("two".to_string())),
                user_sid: "S-1-5-21-test".to_string(),
                desktop_name: None,
                profile_root: None,
                policy_version: None,
            })
            .expect_err("second active session should be rejected");

        assert!(matches!(err, SandboxError::Denied(_)));
    }

    #[test]
    fn launch_app_rejects_unlisted_app() {
        let mut service = SandboxService::development().expect("service should initialize");
        let session = service
            .create_session(CreateSessionRequest {
                sandbox_id: Some(SandboxId("test".to_string())),
                user_sid: "S-1-5-21-test".to_string(),
                desktop_name: Some("Sandbox-Test".to_string()),
                profile_root: Some(PathBuf::from(r"C:\SandboxPlus\Test")),
                policy_version: None,
            })
            .expect("session should be created")
            .instance;

        let err = service
            .launch_app(LaunchAppRequest {
                sandbox_id: session.id,
                app_id: "notepad".to_string(),
                executable: PathBuf::from("notepad.exe"),
                arguments: Vec::new(),
                working_directory: None,
                desktop_name: session.desktop_name,
                profile_root: session.profile_root,
                environment_overrides: Vec::new(),
                policy_version: session.policy_version,
            })
            .expect_err("unlisted app should be rejected");

        assert!(matches!(err, SandboxError::Denied(_)));
    }

    #[test]
    fn launch_app_accepts_policy_app_with_restricted_token() {
        let app = SandboxApp {
            id: "cmd".to_string(),
            name: "Command".to_string(),
            exe_path: PathBuf::from("cmd.exe"),
            args: vec!["/c".to_string(), "exit 0".to_string()],
            working_dir: None,
            icon_path: None,
            hash_sha256: Some(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            ),
            signer_thumbprint: None,
            network_profile: NetworkProfile::IntranetOnly,
            auto_start: false,
        };
        let policy = SandboxPolicy {
            version: "test".to_string(),
            apps: vec![app],
            network: Default::default(),
            filesystem: Default::default(),
            clipboard: Default::default(),
            printing: Default::default(),
            audit: Default::default(),
        };
        let mut service = SandboxService::new(policy).expect("policy should be valid");
        let session = service
            .create_session(CreateSessionRequest {
                sandbox_id: Some(SandboxId("test".to_string())),
                user_sid: "S-1-5-21-test".to_string(),
                desktop_name: Some("Sandbox-Test".to_string()),
                profile_root: Some(PathBuf::from(r"C:\SandboxPlus\Test")),
                policy_version: None,
            })
            .expect("session should be created")
            .instance;

        let response = service
            .launch_app(LaunchAppRequest {
                sandbox_id: session.id,
                app_id: "cmd".to_string(),
                executable: PathBuf::from("cmd.exe"),
                arguments: vec!["/c".to_string(), "exit 0".to_string()],
                working_directory: None,
                desktop_name: session.desktop_name,
                profile_root: session.profile_root,
                environment_overrides: Vec::new(),
                policy_version: session.policy_version,
            })
            .expect("policy app should pass service boundary checks");

        assert_eq!(response.process.app_id.as_deref(), Some("cmd"));
        assert!(response.restricted_token_applied);
        assert!(response.profile_applied);
    }

    #[test]
    fn close_session_keep_profile_clears_temporary_profile_state() {
        let profile_root =
            std::env::temp_dir().join(format!("sandbox-plus-test-{}", uuid::Uuid::new_v4()));
        let temp_file = profile_root.join("Temp").join("scratch.txt");
        let local_temp_file = profile_root
            .join(r"AppData\Local\Temp")
            .join("local-scratch.txt");

        let mut service = SandboxService::development().expect("service should initialize");
        let session = service
            .create_session(CreateSessionRequest {
                sandbox_id: Some(SandboxId("cleanup-test".to_string())),
                user_sid: "S-1-5-21-test".to_string(),
                desktop_name: Some("Sandbox-Cleanup-Test".to_string()),
                profile_root: Some(profile_root.clone()),
                policy_version: None,
            })
            .expect("session should be created")
            .instance;

        std::fs::write(&temp_file, "temporary").expect("temp file should be writable");
        std::fs::write(&local_temp_file, "temporary").expect("local temp file should be writable");

        service
            .close_session(CloseSessionRequest {
                sandbox_id: session.id,
                mode: CloseSessionMode::KeepProfile,
            })
            .expect("session close should succeed");

        assert!(profile_root.exists());
        assert!(profile_root.join("Temp").exists());
        assert!(profile_root.join(r"AppData\Local\Temp").exists());
        assert!(!temp_file.exists());
        assert!(!local_temp_file.exists());

        let _ = std::fs::remove_dir_all(profile_root);
    }
}
