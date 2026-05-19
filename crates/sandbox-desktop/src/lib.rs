#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopSource {
    Opened,
    Created,
}

impl std::fmt::Display for DesktopSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Opened => write!(formatter, "opened existing"),
            Self::Created => write!(formatter, "created new"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopReady {
    pub name: String,
    pub source: DesktopSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopSwitchResult {
    pub name: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopProcess {
    pub process_id: u32,
}

#[cfg(windows)]
mod platform {
    use super::{DesktopProcess, DesktopReady, DesktopSource, DesktopSwitchResult};
    use sandbox_common::{Result, SandboxError};
    use std::mem::size_of;
    use std::ptr::{null, null_mut};
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, HANDLE, HLOCAL};
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        SetUserObjectSecurity, DACL_SECURITY_INFORMATION, OBJECT_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    };
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, CreateDesktopW, GetProcessWindowStation, OpenDesktopW, SwitchDesktop,
        DESKTOP_CREATEWINDOW, DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP, DESKTOP_WRITEOBJECTS,
        HDESK,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, WaitForInputIdle, PROCESS_INFORMATION, STARTUPINFOW,
    };

    const DESKTOP_ACCESS: u32 =
        DESKTOP_CREATEWINDOW | DESKTOP_READOBJECTS | DESKTOP_SWITCHDESKTOP | DESKTOP_WRITEOBJECTS;
    const READ_CONTROL: u32 = 0x0002_0000;
    const WRITE_DAC: u32 = 0x0004_0000;
    const DESKTOP_SECURITY_ACCESS: u32 = DESKTOP_ACCESS | READ_CONTROL | WRITE_DAC;

    pub fn ensure_desktop(name: &str) -> Result<DesktopReady> {
        validate_desktop_name(name)?;
        let desktop = DesktopHandle::open_or_create_with_access(name, DESKTOP_SECURITY_ACCESS)?;

        Ok(DesktopReady {
            name: name.to_string(),
            source: desktop.source,
        })
    }

    pub fn grant_desktop_access(name: &str, user_sid: &str) -> Result<()> {
        validate_desktop_name(name)?;
        if user_sid.trim().is_empty() {
            return Err(SandboxError::Configuration(
                "user SID must not be empty".to_string(),
            ));
        }

        let window_station = unsafe { GetProcessWindowStation() };
        if window_station.is_null() {
            return Err(last_error("GetProcessWindowStation"));
        }
        apply_user_object_dacl(
            window_station as HANDLE,
            &WindowStationSecurity::sddl(user_sid),
            "SetUserObjectSecurity(WinSta0)",
        )?;

        let desktop = DesktopHandle::open_or_create(name)?;
        match apply_user_object_dacl(
            desktop.raw as HANDLE,
            &DesktopSecurity::sddl(user_sid),
            "SetUserObjectSecurity(desktop)",
        ) {
            Ok(()) => Ok(()),
            Err(SandboxError::System(message)) if message.contains("Win32 error 5") => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn switch_to_desktop(name: &str) -> Result<DesktopSwitchResult> {
        validate_desktop_name(name)?;
        let desktop = DesktopHandle::open_or_create(name)?;
        let started = Instant::now();
        desktop.switch_to()?;

        Ok(DesktopSwitchResult {
            name: name.to_string(),
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    pub fn switch_to_default_desktop() -> Result<DesktopSwitchResult> {
        let desktop = DesktopHandle::open("Default", DESKTOP_SWITCHDESKTOP)?;
        let started = Instant::now();
        desktop.switch_to()?;

        Ok(DesktopSwitchResult {
            name: "Default".to_string(),
            elapsed_ms: started.elapsed().as_millis(),
        })
    }

    pub fn spawn_on_desktop(command_line: &str, desktop_name: &str) -> Result<DesktopProcess> {
        validate_desktop_name(desktop_name)?;
        if command_line.trim().is_empty() {
            return Err(SandboxError::Configuration(
                "command line must not be empty".to_string(),
            ));
        }
        let _desktop_handle = DesktopHandle::open_or_create(desktop_name)?;

        let mut command_line = wide_null(command_line);
        let mut desktop = wide_null(&format!("WinSta0\\{desktop_name}"));

        let mut startup_info: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup_info.cb = size_of::<STARTUPINFOW>() as u32;
        startup_info.lpDesktop = desktop.as_mut_ptr();

        let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            CreateProcessW(
                null(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                0,
                0,
                null(),
                null(),
                &mut startup_info,
                &mut process_info,
            )
        };
        if ok == 0 {
            return Err(last_error("CreateProcessW"));
        }

        unsafe {
            WaitForInputIdle(process_info.hProcess, 5000);
        }
        close_handle(process_info.hThread);
        close_handle(process_info.hProcess);

        Ok(DesktopProcess {
            process_id: process_info.dwProcessId,
        })
    }

    struct DesktopHandle {
        raw: HDESK,
        source: DesktopSource,
    }

    impl DesktopHandle {
        fn open_or_create(name: &str) -> Result<Self> {
            Self::open_or_create_with_access(name, DESKTOP_ACCESS)
        }

        fn open_or_create_with_access(name: &str, access: u32) -> Result<Self> {
            match Self::open(name, access) {
                Ok(mut desktop) => {
                    desktop.source = DesktopSource::Opened;
                    Ok(desktop)
                }
                Err(open_error) => match Self::create(name) {
                    Ok(desktop) => Ok(desktop),
                    Err(create_error) => Err(SandboxError::System(format!(
                        "open failed ({open_error}); create failed ({create_error})"
                    ))),
                },
            }
        }

        fn create(name: &str) -> Result<Self> {
            let name = wide_null(name);
            let mut security = DesktopSecurity::default()?;
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

            Ok(Self {
                raw,
                source: DesktopSource::Created,
            })
        }

        fn open(name: &str, access: u32) -> Result<Self> {
            let name = wide_null(name);
            let raw = unsafe { OpenDesktopW(name.as_ptr(), 0, 0, access) };
            if raw.is_null() {
                return Err(last_error("OpenDesktopW"));
            }

            Ok(Self {
                raw,
                source: DesktopSource::Opened,
            })
        }

        fn switch_to(&self) -> Result<()> {
            let ok = unsafe { SwitchDesktop(self.raw) };
            if ok == 0 {
                return Err(last_error("SwitchDesktop"));
            }

            Ok(())
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

    fn validate_desktop_name(name: &str) -> Result<()> {
        if name.trim().is_empty() {
            return Err(SandboxError::Configuration(
                "desktop name must not be empty".to_string(),
            ));
        }

        Ok(())
    }

    fn close_handle(handle: HANDLE) {
        if !handle.is_null() {
            unsafe {
                CloseHandle(handle);
            }
        }
    }

    struct WindowStationSecurity;

    impl WindowStationSecurity {
        fn sddl(user_sid: &str) -> String {
            format!(
                "D:\
                 (A;;0x000F037F;;;SY)\
                 (A;;0x000F037F;;;BA)\
                 (A;;0x000F037F;;;IU)\
                 (A;;0x000F037F;;;WD)\
                 (A;;0x000F037F;;;{user_sid})"
            )
        }
    }

    struct UserObjectSecurity {
        descriptor: PSECURITY_DESCRIPTOR,
        attributes: SECURITY_ATTRIBUTES,
    }

    impl UserObjectSecurity {
        fn from_sddl(sddl: &str) -> Result<Self> {
            let sddl = wide_null(sddl);
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

    impl Drop for UserObjectSecurity {
        fn drop(&mut self) {
            if !self.descriptor.is_null() {
                unsafe {
                    LocalFree(self.descriptor as HLOCAL);
                }
            }
        }
    }

    struct DesktopSecurity;

    impl DesktopSecurity {
        fn default() -> Result<UserObjectSecurity> {
            UserObjectSecurity::from_sddl(&Self::sddl("WD"))
        }

        fn sddl(user_sid: &str) -> String {
            format!(
                "D:\
                 (A;;0x000F01FF;;;SY)\
                 (A;;0x000F01FF;;;BA)\
                 (A;;0x000F01FF;;;IU)\
                 (A;;0x000F01FF;;;WD)\
                 (A;;0x000F01FF;;;{user_sid})"
            )
        }
    }

    fn apply_user_object_dacl(handle: HANDLE, sddl: &str, api: &str) -> Result<()> {
        let security = UserObjectSecurity::from_sddl(sddl)?;
        let mut requested: OBJECT_SECURITY_INFORMATION = DACL_SECURITY_INFORMATION;
        let ok = unsafe { SetUserObjectSecurity(handle, &mut requested, security.descriptor) };
        if ok == 0 {
            return Err(last_error(api));
        }
        Ok(())
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
    use super::{DesktopProcess, DesktopReady, DesktopSwitchResult};
    use sandbox_common::{Result, SandboxError};

    pub fn ensure_desktop(_name: &str) -> Result<DesktopReady> {
        Err(SandboxError::UnsupportedPlatform(
            "Windows Desktop APIs are only available on Windows".to_string(),
        ))
    }

    pub fn grant_desktop_access(_name: &str, _user_sid: &str) -> Result<()> {
        Err(SandboxError::UnsupportedPlatform(
            "Windows Desktop ACL APIs are only available on Windows".to_string(),
        ))
    }

    pub fn switch_to_desktop(_name: &str) -> Result<DesktopSwitchResult> {
        Err(SandboxError::UnsupportedPlatform(
            "Windows Desktop APIs are only available on Windows".to_string(),
        ))
    }

    pub fn switch_to_default_desktop() -> Result<DesktopSwitchResult> {
        Err(SandboxError::UnsupportedPlatform(
            "Windows Desktop APIs are only available on Windows".to_string(),
        ))
    }

    pub fn spawn_on_desktop(_command_line: &str, _desktop_name: &str) -> Result<DesktopProcess> {
        Err(SandboxError::UnsupportedPlatform(
            "Windows Desktop APIs are only available on Windows".to_string(),
        ))
    }
}

pub use platform::{
    ensure_desktop, grant_desktop_access, spawn_on_desktop, switch_to_default_desktop,
    switch_to_desktop,
};
