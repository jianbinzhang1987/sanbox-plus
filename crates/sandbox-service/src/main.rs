use sandbox_ipc::{serve_once, SERVICE_PIPE_NAME};
use sandbox_service::SandboxService;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "run".to_string());
    let policy_path = parse_policy_path(args);

    let result = match command.as_str() {
        "run" => run_service(false, policy_path),
        "run-service" => run_windows_service(policy_path),
        "run-once" => run_service(true, policy_path),
        "install" => install_service(policy_path),
        "uninstall" => uninstall_service(),
        "start" => control_service("start"),
        "stop" => control_service("stop"),
        "query" | "status" => control_service("query"),
        _ => {
            eprintln!("unknown command: {command}");
            eprintln!(
                "usage: sandbox-service [run|run-service|run-once|install|uninstall|start|stop|query] [--policy path.json]"
            );
            std::process::exit(2);
        }
    };

    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn parse_policy_path(args: impl Iterator<Item = String>) -> Option<String> {
    let mut args = args.peekable();
    let mut policy_path = None;
    while let Some(arg) = args.next() {
        if arg == "--policy" {
            policy_path = args.next();
        }
    }
    policy_path
}

fn run_service(once: bool, policy_path: Option<String>) -> sandbox_common::Result<()> {
    run_service_with_stop(once, policy_path, || false)
}

fn run_service_with_stop(
    once: bool,
    policy_path: Option<String>,
    should_stop: impl Fn() -> bool,
) -> sandbox_common::Result<()> {
    let mut service = if let Some(policy_path) = policy_path {
        SandboxService::from_policy_file(policy_path)?
    } else {
        SandboxService::development()?
    };
    println!("sandbox-service listening on {SERVICE_PIPE_NAME}");

    if once {
        serve_once(SERVICE_PIPE_NAME, &mut |request| {
            service.handle_request(request)
        })
    } else {
        while !should_stop() {
            if let Err(error) = serve_once(SERVICE_PIPE_NAME, &mut |request| {
                service.handle_request(request)
            }) {
                eprintln!("named pipe request failed: {error}");
            }
        }
        Ok(())
    }
}

fn install_service(policy_path: Option<String>) -> sandbox_common::Result<()> {
    platform_service::install(policy_path)
}

fn uninstall_service() -> sandbox_common::Result<()> {
    platform_service::uninstall()
}

fn control_service(command: &str) -> sandbox_common::Result<()> {
    platform_service::control(command)
}

fn run_windows_service(policy_path: Option<String>) -> sandbox_common::Result<()> {
    platform_service::run(policy_path)
}

#[cfg(windows)]
mod platform_service {
    use super::{run_service_with_stop, PathBuf};
    use sandbox_common::{Result, SandboxError};
    use sandbox_ipc::SERVICE_PIPE_NAME;
    use std::io::Write;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use windows_sys::Win32::System::Services::{
        RegisterServiceCtrlHandlerW, SetServiceStatus, StartServiceCtrlDispatcherW,
        SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP, SERVICE_CONTROL_SHUTDOWN,
        SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS,
        SERVICE_STATUS_HANDLE, SERVICE_STOPPED, SERVICE_STOP_PENDING, SERVICE_TABLE_ENTRYW,
        SERVICE_WIN32_OWN_PROCESS,
    };

    const SERVICE_NAME: &str = "SandboxPlusService";
    const DISPLAY_NAME: &str = "Sandbox+ Service";
    static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
    static POLICY_PATH: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    static SERVICE_STATUS_HANDLE_CELL: OnceLock<usize> = OnceLock::new();

    pub fn install(policy_path: Option<String>) -> Result<()> {
        let exe = current_exe()?;
        let mut bin_path = format!("\"{}\" run-service", exe.display());
        if let Some(policy_path) = policy_path {
            bin_path.push_str(" --policy ");
            bin_path.push_str(&quote_arg(&policy_path));
        }

        run_sc(&[
            "create",
            SERVICE_NAME,
            "type=",
            "own",
            "start=",
            "auto",
            "DisplayName=",
            DISPLAY_NAME,
            "binPath=",
            &bin_path,
        ])?;
        run_sc(&[
            "failure",
            SERVICE_NAME,
            "reset=",
            "86400",
            "actions=",
            "restart/60000/restart/60000/none/0",
        ])?;
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let _ = control("stop");
        run_sc(&["delete", SERVICE_NAME])
    }

    pub fn control(command: &str) -> Result<()> {
        run_sc(&[command, SERVICE_NAME])
    }

    pub fn run(policy_path: Option<String>) -> Result<()> {
        let cell = POLICY_PATH.get_or_init(|| Mutex::new(None));
        *cell
            .lock()
            .map_err(|_| SandboxError::System("service policy path lock poisoned".to_string()))? =
            policy_path;

        let mut name = wide_null(SERVICE_NAME);
        let mut table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: name.as_mut_ptr(),
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW {
                lpServiceName: std::ptr::null_mut(),
                lpServiceProc: None,
            },
        ];

        let ok = unsafe { StartServiceCtrlDispatcherW(table.as_mut_ptr()) };
        if ok == 0 {
            return Err(last_error("StartServiceCtrlDispatcherW"));
        }
        Ok(())
    }

    unsafe extern "system" fn service_main(_argc: u32, _argv: *mut *mut u16) {
        let service_name = wide_null(SERVICE_NAME);
        let handle =
            unsafe { RegisterServiceCtrlHandlerW(service_name.as_ptr(), Some(service_handler)) };
        if handle.is_null() {
            return;
        }
        let _ = SERVICE_STATUS_HANDLE_CELL.set(handle as usize);
        set_status(SERVICE_START_PENDING, 0);
        set_status(
            SERVICE_RUNNING,
            SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN,
        );

        let policy_path = POLICY_PATH
            .get()
            .and_then(|cell| cell.lock().ok().and_then(|guard| guard.clone()));
        if let Err(error) =
            run_service_with_stop(false, policy_path, || STOP_REQUESTED.load(Ordering::SeqCst))
        {
            eprintln!("{error}");
        }

        set_status(SERVICE_STOPPED, 0);
    }

    unsafe extern "system" fn service_handler(control: u32) {
        if matches!(control, SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN) {
            STOP_REQUESTED.store(true, Ordering::SeqCst);
            set_status(SERVICE_STOP_PENDING, 0);
            wake_service_pipe();
        }
    }

    fn wake_service_pipe() {
        if let Ok(mut pipe) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(SERVICE_PIPE_NAME)
        {
            let _ = pipe.write_all(br#""GetStatus""#);
            let _ = pipe.flush();
        }
    }

    fn set_status(current_state: u32, controls_accepted: u32) {
        if let Some(handle) = SERVICE_STATUS_HANDLE_CELL.get() {
            let mut status = SERVICE_STATUS {
                dwServiceType: SERVICE_WIN32_OWN_PROCESS,
                dwCurrentState: current_state,
                dwControlsAccepted: controls_accepted,
                dwWin32ExitCode: 0,
                dwServiceSpecificExitCode: 0,
                dwCheckPoint: 0,
                dwWaitHint: 0,
            };
            unsafe {
                SetServiceStatus(*handle as SERVICE_STATUS_HANDLE, &mut status);
            }
        }
    }

    fn current_exe() -> Result<PathBuf> {
        std::env::current_exe().map_err(|error| {
            SandboxError::System(format!("failed to resolve current exe: {error}"))
        })
    }

    fn run_sc(args: &[&str]) -> Result<()> {
        let output = Command::new("sc.exe")
            .args(args)
            .output()
            .map_err(|error| SandboxError::System(format!("failed to run sc.exe: {error}")))?;
        if !output.status.success() {
            return Err(SandboxError::System(format!(
                "sc.exe failed with status {}: {}{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        print!("{}", String::from_utf8_lossy(&output.stdout));
        Ok(())
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
        let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        SandboxError::System(format!("{api} failed with Win32 error {code}"))
    }
}

#[cfg(not(windows))]
mod platform_service {
    use sandbox_common::{Result, SandboxError};

    pub fn install(_policy_path: Option<String>) -> Result<()> {
        Err(windows_only())
    }

    pub fn uninstall() -> Result<()> {
        Err(windows_only())
    }

    pub fn control(_command: &str) -> Result<()> {
        Err(windows_only())
    }

    pub fn run(policy_path: Option<String>) -> Result<()> {
        super::run_service(false, policy_path)
    }

    fn windows_only() -> SandboxError {
        SandboxError::UnsupportedPlatform(
            "Windows Service installation is only available on Windows".to_string(),
        )
    }
}
