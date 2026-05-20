use sandbox_common::{
    LaunchAppRequest, LaunchAppResponse, ProcessInfo, ProcessState, Result, SandboxError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherOptions {
    pub dry_run: bool,
    pub credentials: Option<LaunchCredentials>,
    pub job_handle: Option<isize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCredentials {
    pub username: String,
    pub domain: Option<String>,
    pub password: String,
}

impl Default for LauncherOptions {
    fn default() -> Self {
        Self {
            dry_run: true,
            credentials: None,
            job_handle: None,
        }
    }
}

pub fn launch_app(
    request: LaunchAppRequest,
    options: LauncherOptions,
) -> Result<LaunchAppResponse> {
    validate_launch_request(&request)?;

    if !options.dry_run {
        return platform::launch_restricted(request, options.credentials, options.job_handle);
    }

    Ok(LaunchAppResponse {
        process: ProcessInfo {
            process_id: 0,
            app_id: Some(request.app_id),
            executable: request.executable,
            state: ProcessState::Starting,
        },
        restricted_token_applied: false,
        profile_applied: false,
        job_assigned: false,
    })
}

pub fn validate_launch_request(request: &LaunchAppRequest) -> Result<()> {
    if request.app_id.trim().is_empty() {
        return Err(SandboxError::Configuration(
            "launch request requires app_id".to_string(),
        ));
    }

    if request.desktop_name.trim().is_empty() {
        return Err(SandboxError::Configuration(
            "launch request requires desktop_name".to_string(),
        ));
    }

    if request.executable.as_os_str().is_empty() {
        return Err(SandboxError::Configuration(
            "launch request requires executable".to_string(),
        ));
    }

    if request.profile_root.as_os_str().is_empty() {
        return Err(SandboxError::Configuration(
            "launch request requires profile_root".to_string(),
        ));
    }

    if request.policy_version.trim().is_empty() {
        return Err(SandboxError::Configuration(
            "launch request requires policy_version".to_string(),
        ));
    }

    Ok(())
}

#[cfg(windows)]
mod platform {
    use sandbox_common::{
        LaunchAppRequest, LaunchAppResponse, ProcessInfo, ProcessState, Result, SandboxError,
    };
    use std::mem::size_of;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE, HLOCAL};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        CreateRestrictedToken, LogonUserW, DISABLE_MAX_PRIVILEGE, LOGON32_LOGON_INTERACTIVE,
        LOGON32_PROVIDER_DEFAULT, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_ALL_ACCESS,
    };
    use windows_sys::Win32::System::Environment::{
        CreateEnvironmentBlock, DestroyEnvironmentBlock,
    };
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, CreateDesktopW, OpenDesktopW, DESKTOP_CREATEWINDOW, DESKTOP_READOBJECTS,
        DESKTOP_SWITCHDESKTOP, DESKTOP_WRITEOBJECTS, HDESK,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessAsUserW, CreateProcessWithTokenW, GetCurrentProcess, OpenProcess,
        OpenProcessToken, ResumeThread, WaitForInputIdle, CREATE_BREAKAWAY_FROM_JOB,
        CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, LOGON_WITH_PROFILE, PROCESS_INFORMATION,
        PROCESS_SET_QUOTA, PROCESS_TERMINATE, STARTUPINFOW,
    };
    use windows_sys::Win32::UI::Shell::{LoadUserProfileW, PROFILEINFOW};

    use crate::LaunchCredentials;

    const DESKTOP_ACCESS: u32 =
        DESKTOP_CREATEWINDOW | DESKTOP_READOBJECTS | DESKTOP_SWITCHDESKTOP | DESKTOP_WRITEOBJECTS;
    const PI_NOUI: u32 = 0x0000_0001;

    pub fn launch_restricted(
        request: LaunchAppRequest,
        credentials: Option<LaunchCredentials>,
        job_handle: Option<isize>,
    ) -> Result<LaunchAppResponse> {
        if let Some(credentials) = credentials {
            return launch_with_credentials(request, credentials, job_handle);
        }

        let current_token = ProcessToken::open_current()?;
        let restricted_token = current_token.create_restricted()?;
        let _desktop_handle = DesktopHandle::open_or_create(&request.desktop_name)?;
        let command_line = build_command_line(&request);
        let environment = build_environment_block(&request);
        let child = ChildProcess::create_as_user(
            restricted_token.raw,
            &command_line,
            &request.desktop_name,
            request.working_directory.as_ref(),
            environment.as_ptr() as *mut _,
        )?;
        let job_assigned = child.assign_to_job(job_handle)?;

        Ok(LaunchAppResponse {
            process: ProcessInfo {
                process_id: child.process_id,
                app_id: Some(request.app_id),
                executable: request.executable,
                state: ProcessState::Running,
            },
            restricted_token_applied: true,
            profile_applied: true,
            job_assigned,
        })
    }

    fn launch_with_credentials(
        request: LaunchAppRequest,
        credentials: LaunchCredentials,
        job_handle: Option<isize>,
    ) -> Result<LaunchAppResponse> {
        let _desktop_handle = DesktopHandle::open_or_create(&request.desktop_name)?;
        let logon = LogonSession::interactive(&credentials)?;
        let profile = LoadedUserProfile::load(logon.token, &credentials.username)?;
        let command_line = build_command_line(&request);
        let environment = EnvironmentBlock::for_token(logon.token, &request)?;
        let child = ChildProcess::create_with_token_suspended(
            logon.token,
            &command_line,
            &request.desktop_name,
            request.working_directory.as_ref(),
            environment.as_ptr(),
        )?;
        let job_assigned = child.assign_to_job(job_handle)?;
        child.resume()?;
        profile.keep_loaded();

        Ok(LaunchAppResponse {
            process: ProcessInfo {
                process_id: child.process_id,
                app_id: Some(request.app_id),
                executable: request.executable,
                state: ProcessState::Running,
            },
            restricted_token_applied: false,
            profile_applied: true,
            job_assigned,
        })
    }

    struct ProcessToken {
        raw: HANDLE,
    }

    impl ProcessToken {
        fn open_current() -> Result<Self> {
            let mut token = null_mut();
            let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut token) };
            if ok == 0 {
                return Err(last_error("OpenProcessToken"));
            }
            Ok(Self { raw: token })
        }

        fn create_restricted(&self) -> Result<Self> {
            let mut restricted = null_mut();
            let ok = unsafe {
                CreateRestrictedToken(
                    self.raw,
                    DISABLE_MAX_PRIVILEGE,
                    0,
                    null(),
                    0,
                    null(),
                    0,
                    null(),
                    &mut restricted,
                )
            };
            if ok == 0 {
                return Err(last_error("CreateRestrictedToken"));
            }
            Ok(Self { raw: restricted })
        }
    }

    impl Drop for ProcessToken {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                unsafe {
                    CloseHandle(self.raw);
                }
            }
        }
    }

    struct DesktopHandle {
        raw: HDESK,
    }

    impl DesktopHandle {
        fn open_or_create(name: &str) -> Result<Self> {
            match Self::open(name) {
                Ok(handle) => Ok(handle),
                Err(_) => Self::create(name),
            }
        }

        fn open(name: &str) -> Result<Self> {
            let name = wide_null(name);
            let raw = unsafe { OpenDesktopW(name.as_ptr(), 0, 0, DESKTOP_ACCESS) };
            if raw.is_null() {
                return Err(last_error("OpenDesktopW"));
            }
            Ok(Self { raw })
        }

        fn create(name: &str) -> Result<Self> {
            let name = wide_null(name);
            let mut security = DesktopSecurity::new()?;
            let raw = unsafe {
                CreateDesktopW(
                    name.as_ptr(),
                    null_mut(),
                    null(),
                    0,
                    DESKTOP_ACCESS,
                    security.attributes_mut(),
                )
            };
            if raw.is_null() {
                return Err(last_error("CreateDesktopW"));
            }
            Ok(Self { raw })
        }
    }

    impl Drop for DesktopHandle {
        fn drop(&mut self) {
            if !self.raw.is_null() {
                unsafe {
                    CloseDesktop(self.raw);
                }
            }
        }
    }

    struct ChildProcess {
        process: HANDLE,
        thread: HANDLE,
        process_id: u32,
    }

    impl ChildProcess {
        fn assign_to_job(&self, job_handle: Option<isize>) -> Result<bool> {
            let Some(job_handle) = job_handle else {
                return Ok(false);
            };

            use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
            let process =
                unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, self.process_id) };
            if process.is_null() {
                return Err(last_error("OpenProcess(PROCESS_SET_QUOTA)"));
            }
            let ok = unsafe { AssignProcessToJobObject(job_handle as _, process) };
            unsafe {
                CloseHandle(process);
            }
            if ok == 0 {
                if self.is_already_in_job() {
                    return Ok(false);
                }
                return Err(last_error("AssignProcessToJobObject"));
            }
            Ok(true)
        }

        fn is_already_in_job(&self) -> bool {
            use windows_sys::Win32::System::JobObjects::IsProcessInJob;
            let mut result = 0;
            let ok = unsafe { IsProcessInJob(self.process, null_mut(), &mut result) };
            ok != 0 && result != 0
        }

        fn create_as_user(
            token: HANDLE,
            command_line: &str,
            desktop_name: &str,
            working_directory: Option<&std::path::PathBuf>,
            environment: *mut std::ffi::c_void,
        ) -> Result<Self> {
            let mut command_line = wide_null(command_line);
            let mut desktop = wide_null(&format!("WinSta0\\{desktop_name}"));
            let working_directory_wide = working_directory
                .map(|path| wide_null(&path.display().to_string()))
                .unwrap_or_default();

            let mut startup_info: STARTUPINFOW = unsafe { std::mem::zeroed() };
            startup_info.cb = size_of::<STARTUPINFOW>() as u32;
            startup_info.lpDesktop = desktop.as_mut_ptr();
            let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

            let ok = unsafe {
                CreateProcessAsUserW(
                    token,
                    null(),
                    command_line.as_mut_ptr(),
                    null(),
                    null(),
                    0,
                    CREATE_UNICODE_ENVIRONMENT,
                    environment,
                    if working_directory_wide.is_empty() {
                        null()
                    } else {
                        working_directory_wide.as_ptr()
                    },
                    &mut startup_info,
                    &mut process_info,
                )
            };
            if ok == 0 {
                return Err(last_error("CreateProcessAsUserW"));
            }
            unsafe {
                WaitForInputIdle(process_info.hProcess, 5000);
            }

            Ok(Self {
                process: process_info.hProcess,
                thread: process_info.hThread,
                process_id: process_info.dwProcessId,
            })
        }

        fn create_with_token_suspended(
            token: HANDLE,
            command_line: &str,
            desktop_name: &str,
            working_directory: Option<&std::path::PathBuf>,
            environment: *mut std::ffi::c_void,
        ) -> Result<Self> {
            let mut command_line = wide_null(command_line);
            let mut desktop = wide_null(&format!("WinSta0\\{desktop_name}"));
            let working_directory_wide = working_directory
                .map(|path| wide_null(&path.display().to_string()))
                .unwrap_or_default();

            let mut startup_info: STARTUPINFOW = unsafe { std::mem::zeroed() };
            startup_info.cb = size_of::<STARTUPINFOW>() as u32;
            startup_info.lpDesktop = desktop.as_mut_ptr();
            let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

            let ok = unsafe {
                CreateProcessWithTokenW(
                    token,
                    LOGON_WITH_PROFILE,
                    null(),
                    command_line.as_mut_ptr(),
                    CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED | CREATE_BREAKAWAY_FROM_JOB,
                    environment,
                    if working_directory_wide.is_empty() {
                        null()
                    } else {
                        working_directory_wide.as_ptr()
                    },
                    &startup_info,
                    &mut process_info,
                )
            };
            if ok == 0 {
                return Err(last_error("CreateProcessWithTokenW(dedicated)"));
            }

            Ok(Self {
                process: process_info.hProcess,
                thread: process_info.hThread,
                process_id: process_info.dwProcessId,
            })
        }

        fn resume(&self) -> Result<()> {
            let result = unsafe { ResumeThread(self.thread) };
            if result == u32::MAX {
                return Err(last_error("ResumeThread"));
            }
            unsafe {
                WaitForInputIdle(self.process, 5000);
            }
            Ok(())
        }
    }

    impl Drop for ChildProcess {
        fn drop(&mut self) {
            unsafe {
                if !self.thread.is_null() {
                    CloseHandle(self.thread);
                }
                if !self.process.is_null() {
                    CloseHandle(self.process);
                }
            }
        }
    }

    fn build_command_line(request: &LaunchAppRequest) -> String {
        let mut parts = vec![quote_arg(&request.executable.display().to_string())];
        parts.extend(request.arguments.iter().map(|arg| quote_arg(arg)));
        parts.join(" ")
    }

    fn build_environment_block(request: &LaunchAppRequest) -> Vec<u16> {
        let mut values: Vec<(String, String)> = std::env::vars().collect();
        upsert_env(
            &mut values,
            "USERPROFILE",
            &request.profile_root.display().to_string(),
        );
        upsert_env(
            &mut values,
            "APPDATA",
            &request
                .profile_root
                .join("AppData\\Roaming")
                .display()
                .to_string(),
        );
        upsert_env(
            &mut values,
            "LOCALAPPDATA",
            &request
                .profile_root
                .join("AppData\\Local")
                .display()
                .to_string(),
        );
        upsert_env(
            &mut values,
            "TEMP",
            &request.profile_root.join("Temp").display().to_string(),
        );
        upsert_env(
            &mut values,
            "TMP",
            &request.profile_root.join("Temp").display().to_string(),
        );
        for item in &request.environment_overrides {
            upsert_env(&mut values, &item.name, &item.value);
        }
        values.sort_by(|left, right| left.0.to_uppercase().cmp(&right.0.to_uppercase()));

        let mut block = Vec::new();
        for (name, value) in values {
            block.extend(format!("{name}={value}").encode_utf16());
            block.push(0);
        }
        block.push(0);
        block
    }

    struct LogonSession {
        token: HANDLE,
    }

    impl LogonSession {
        fn interactive(credentials: &LaunchCredentials) -> Result<Self> {
            let username = wide_null(&credentials.username);
            let domain = credentials
                .domain
                .as_deref()
                .map(wide_null)
                .unwrap_or_else(|| wide_null("."));
            let password = wide_null(&credentials.password);
            let mut token = null_mut();
            let ok = unsafe {
                LogonUserW(
                    username.as_ptr(),
                    domain.as_ptr(),
                    password.as_ptr(),
                    LOGON32_LOGON_INTERACTIVE,
                    LOGON32_PROVIDER_DEFAULT,
                    &mut token,
                )
            };
            if ok == 0 {
                return Err(last_error("LogonUserW"));
            }
            Ok(Self { token })
        }
    }

    impl Drop for LogonSession {
        fn drop(&mut self) {
            if !self.token.is_null() {
                unsafe {
                    CloseHandle(self.token);
                }
            }
        }
    }

    struct LoadedUserProfile {
        profile: HANDLE,
    }

    impl LoadedUserProfile {
        fn load(token: HANDLE, username: &str) -> Result<Self> {
            let mut username = wide_null(username);
            let mut profile: PROFILEINFOW = unsafe { std::mem::zeroed() };
            profile.dwSize = size_of::<PROFILEINFOW>() as u32;
            profile.dwFlags = PI_NOUI;
            profile.lpUserName = username.as_mut_ptr();

            let ok = unsafe { LoadUserProfileW(token, &mut profile) };
            if ok == 0 {
                return Err(last_error("LoadUserProfileW"));
            }

            Ok(Self {
                profile: profile.hProfile,
            })
        }

        fn keep_loaded(self) {
            std::mem::forget(self);
        }
    }

    impl Drop for LoadedUserProfile {
        fn drop(&mut self) {
            let _ = self.profile;
        }
    }

    struct EnvironmentBlock {
        block: Vec<u16>,
    }

    impl EnvironmentBlock {
        fn for_token(token: HANDLE, request: &LaunchAppRequest) -> Result<Self> {
            let mut raw = null_mut();
            let ok = unsafe { CreateEnvironmentBlock(&mut raw, token, 0) };
            if ok == 0 {
                return Err(last_error("CreateEnvironmentBlock"));
            }

            let mut values = parse_environment_block(raw as *const u16);
            unsafe {
                DestroyEnvironmentBlock(raw);
            }

            apply_profile_environment_overrides(&mut values, request);
            values.sort_by(|left, right| left.0.to_uppercase().cmp(&right.0.to_uppercase()));

            let mut block = Vec::new();
            for (name, value) in values {
                block.extend(format!("{name}={value}").encode_utf16());
                block.push(0);
            }
            block.push(0);

            Ok(Self { block })
        }

        fn as_ptr(&self) -> *mut std::ffi::c_void {
            self.block.as_ptr() as *mut _
        }
    }

    fn parse_environment_block(raw: *const u16) -> Vec<(String, String)> {
        let mut values = Vec::new();
        let mut offset = 0usize;
        loop {
            let mut len = 0usize;
            unsafe {
                while *raw.add(offset + len) != 0 {
                    len += 1;
                }
            }
            if len == 0 {
                break;
            }

            let item = unsafe { std::slice::from_raw_parts(raw.add(offset), len) };
            let item = String::from_utf16_lossy(item);
            if let Some((name, value)) = item.split_once('=') {
                if !name.is_empty() {
                    values.push((name.to_string(), value.to_string()));
                }
            }
            offset += len + 1;
        }
        values
    }

    fn apply_profile_environment_overrides(
        values: &mut Vec<(String, String)>,
        request: &LaunchAppRequest,
    ) {
        upsert_env(
            values,
            "USERPROFILE",
            &request.profile_root.display().to_string(),
        );
        upsert_env(
            values,
            "APPDATA",
            &request
                .profile_root
                .join("AppData\\Roaming")
                .display()
                .to_string(),
        );
        upsert_env(
            values,
            "LOCALAPPDATA",
            &request
                .profile_root
                .join("AppData\\Local")
                .display()
                .to_string(),
        );
        upsert_env(
            values,
            "TEMP",
            &request.profile_root.join("Temp").display().to_string(),
        );
        upsert_env(
            values,
            "TMP",
            &request.profile_root.join("Temp").display().to_string(),
        );
        for item in &request.environment_overrides {
            upsert_env(values, &item.name, &item.value);
        }
    }

    fn upsert_env(values: &mut Vec<(String, String)>, name: &str, value: &str) {
        if let Some((_, existing)) = values
            .iter_mut()
            .find(|(existing_name, _)| existing_name.eq_ignore_ascii_case(name))
        {
            *existing = value.to_string();
        } else {
            values.push((name.to_string(), value.to_string()));
        }
    }

    struct DesktopSecurity {
        descriptor: PSECURITY_DESCRIPTOR,
        attributes: SECURITY_ATTRIBUTES,
    }

    impl DesktopSecurity {
        fn new() -> Result<Self> {
            let sddl = wide_null(
                "D:\
                 (A;;0x000F01FF;;;SY)\
                 (A;;0x000F01FF;;;BA)\
                 (A;;0x000F01FF;;;IU)\
                 (A;;0x000F01FF;;;WD)",
            );
            let mut descriptor = null_mut();
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    null_mut(),
                )
            };
            if ok == 0 {
                return Err(last_error(
                    "ConvertStringSecurityDescriptorToSecurityDescriptorW",
                ));
            }

            let attributes = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            Ok(Self {
                descriptor,
                attributes,
            })
        }

        fn attributes_mut(&mut self) -> *const SECURITY_ATTRIBUTES {
            &self.attributes
        }
    }

    impl Drop for DesktopSecurity {
        fn drop(&mut self) {
            if !self.descriptor.is_null() {
                unsafe {
                    LocalFree(self.descriptor as HLOCAL);
                }
            }
        }
    }

    fn quote_arg(value: &str) -> String {
        if value.contains(' ') || value.contains('\t') {
            format!("\"{}\"", value.replace('"', "\\\""))
        } else {
            value.to_string()
        }
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error(api: &str) -> SandboxError {
        let code = unsafe { GetLastError() };
        SandboxError::System(format!("{api} failed with Win32 error {code}"))
    }
}

#[cfg(not(windows))]
mod platform {
    use crate::LaunchCredentials;
    use sandbox_common::{LaunchAppRequest, LaunchAppResponse, Result, SandboxError};

    pub fn launch_restricted(
        _request: LaunchAppRequest,
        _credentials: Option<LaunchCredentials>,
        _job_handle: Option<isize>,
    ) -> Result<LaunchAppResponse> {
        Err(SandboxError::UnsupportedPlatform(
            "restricted token launch is only available on Windows".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sandbox_common::{LaunchAppRequest, SandboxId};
    use std::path::PathBuf;

    fn valid_request() -> LaunchAppRequest {
        LaunchAppRequest {
            sandbox_id: SandboxId("test".to_string()),
            app_id: "cmd".to_string(),
            executable: PathBuf::from("cmd.exe"),
            arguments: vec!["/c".to_string(), "exit 0".to_string()],
            working_directory: None,
            desktop_name: "Sandbox-Test".to_string(),
            profile_root: PathBuf::from(r"C:\SandboxPlus\Test"),
            environment_overrides: Vec::new(),
            policy_version: "test".to_string(),
        }
    }

    #[test]
    fn dry_run_returns_boundary_response() {
        let response = launch_app(valid_request(), LauncherOptions::default())
            .expect("dry-run launch should pass");

        assert_eq!(response.process.process_id, 0);
        assert_eq!(response.process.app_id.as_deref(), Some("cmd"));
        assert!(!response.restricted_token_applied);
        assert!(!response.profile_applied);
        assert!(!response.job_assigned);
    }

    #[test]
    fn real_launch_starts_restricted_process() {
        let response = launch_app(
            valid_request(),
            LauncherOptions {
                dry_run: false,
                credentials: None,
                job_handle: None,
            },
        )
        .expect("real restricted launch should start");

        assert!(response.process.process_id > 0);
        assert_eq!(response.process.app_id.as_deref(), Some("cmd"));
        assert!(response.restricted_token_applied);
        assert!(response.profile_applied);
    }

    #[test]
    fn missing_desktop_is_rejected() {
        let mut request = valid_request();
        request.desktop_name = " ".to_string();

        let err = validate_launch_request(&request).expect_err("desktop should be required");

        assert!(matches!(err, SandboxError::Configuration(_)));
    }
}
