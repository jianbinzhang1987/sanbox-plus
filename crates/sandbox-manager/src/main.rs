use sandbox_common::{
    CloseSessionMode, CloseSessionRequest, CreateSessionRequest, LaunchAppRequest, ProcessInfo,
    ProcessState, SandboxError, SandboxInstance, SandboxState, UpdateSessionStateRequest,
};
use sandbox_desktop::{
    ensure_desktop, spawn_on_desktop, switch_to_default_desktop, switch_to_desktop,
};
use sandbox_ipc::{send_request, IpcRequest, IpcResponse, SERVICE_PIPE_NAME};

fn main() {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "status".to_string());

    let result = match command.as_str() {
        "status" => print_status(),
        "create-session" => {
            let user_sid = args.next().unwrap_or_else(|| "auto".to_string());
            create_session(user_sid)
        }
        "close-session" => close_session(CloseSessionMode::KeepProfile),
        "reset-session" => close_session(CloseSessionMode::ResetProfile),
        "list-processes" => list_processes(),
        "list-apps" => list_apps(),
        "launch-app" => {
            let app_id = args.next().unwrap_or_else(|| "cmd".to_string());
            launch_policy_app(&app_id)
        }
        "ensure-desktop" => {
            let desktop_name = args.next();
            ensure_controller_desktop(desktop_name.as_deref())
        }
        "enter" => {
            let desktop_name = args.next();
            enter_desktop(desktop_name.as_deref())
        }
        "return" => return_to_host(),
        "launch-on-desktop" => {
            let first = args.next();
            let second = args.next();
            launch_on_desktop(first.as_deref(), second.as_deref())
        }
        "controller" => run_controller(),
        _ => {
            eprintln!("unknown command: {command}");
            print_usage();
            std::process::exit(2);
        }
    };

    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn print_usage() {
    eprintln!(
        "usage:
  sandbox-manager status
  sandbox-manager create-session [user-sid|auto]
  sandbox-manager close-session
  sandbox-manager reset-session
  sandbox-manager list-processes
  sandbox-manager list-apps
  sandbox-manager launch-app <app-id>
  sandbox-manager ensure-desktop [desktop-name]
  sandbox-manager enter [desktop-name]
  sandbox-manager return
  sandbox-manager launch-on-desktop [desktop-name] [command-line]
  sandbox-manager controller

When desktop-name is omitted, manager uses the active service session desktop."
    );
}

fn print_status() -> sandbox_common::Result<()> {
    let response = request_service(IpcRequest::GetStatus)?;
    let IpcResponse::Status(status) = response else {
        return Err(unexpected_response(response));
    };

    println!("Sandbox+ Controller");
    println!("health: {:?}", status.health);
    println!("active_policy_version: {:?}", status.active_policy_version);
    println!("instance: {:?}", status.instance);

    Ok(())
}

fn create_session(user_sid: String) -> sandbox_common::Result<()> {
    let response = request_service(IpcRequest::CreateSession(CreateSessionRequest {
        sandbox_id: None,
        user_sid,
        desktop_name: None,
        profile_root: None,
        policy_version: None,
    }))?;
    let IpcResponse::SessionCreated(response) = response else {
        return Err(unexpected_response(response));
    };

    println!("created sandbox session:");
    println!("  id: {}", response.instance.id.0);
    println!("  desktop: {}", response.instance.desktop_name);
    println!("  profile: {}", response.instance.profile_root.display());
    println!("  state: {:?}", response.instance.state);
    ensure_desktop(&response.instance.desktop_name)?;
    start_workspace(&response.instance)?;
    println!("  workspace: explorer + agent started");

    Ok(())
}

fn close_session(mode: CloseSessionMode) -> sandbox_common::Result<()> {
    let instance = active_instance()?;
    let response = request_service(IpcRequest::CloseSession(CloseSessionRequest {
        sandbox_id: instance.id,
        mode,
    }))?;
    if !matches!(response, IpcResponse::Ok) {
        return Err(unexpected_response(response));
    }

    println!("closed sandbox session");
    Ok(())
}

fn list_processes() -> sandbox_common::Result<()> {
    let instance = active_instance()?;
    let response = request_service(IpcRequest::ListProcesses {
        sandbox_id: instance.id,
    })?;
    let IpcResponse::Processes(processes) = response else {
        return Err(unexpected_response(response));
    };

    if processes.is_empty() {
        println!("no recorded sandbox processes");
    } else {
        for process in processes {
            println!(
                "pid={} app={:?} exe={} state={:?}",
                process.process_id,
                process.app_id,
                process.executable.display(),
                process.state
            );
        }
    }

    Ok(())
}

fn list_apps() -> sandbox_common::Result<()> {
    let instance = active_instance()?;
    let response = request_service(IpcRequest::ListApps {
        sandbox_id: instance.id,
    })?;
    let IpcResponse::Apps(apps) = response else {
        return Err(unexpected_response(response));
    };

    if apps.is_empty() {
        println!("no policy apps configured");
    } else {
        for app in apps {
            println!(
                "id={} name={} exe={} args={:?} auto_start={}",
                app.id,
                app.name,
                app.executable.display(),
                app.arguments,
                app.auto_start
            );
        }
    }

    Ok(())
}

fn launch_policy_app(app_id: &str) -> sandbox_common::Result<()> {
    let instance = active_instance()?;
    let response = request_service(IpcRequest::ListApps {
        sandbox_id: instance.id.clone(),
    })?;
    let IpcResponse::Apps(apps) = response else {
        return Err(unexpected_response(response));
    };
    let app = apps
        .into_iter()
        .find(|app| app.id == app_id)
        .ok_or_else(|| SandboxError::Denied(format!("policy app '{app_id}' was not found")))?;

    ensure_desktop(&instance.desktop_name)?;
    let response = request_service(IpcRequest::LaunchApp(LaunchAppRequest {
        sandbox_id: instance.id,
        app_id: app.id,
        executable: app.executable,
        arguments: app.arguments,
        working_directory: app.working_directory,
        desktop_name: instance.desktop_name,
        profile_root: instance.profile_root,
        environment_overrides: Vec::new(),
        policy_version: instance.policy_version,
    }))?;
    let IpcResponse::LaunchApp(response) = response else {
        return Err(unexpected_response(response));
    };

    println!(
        "launched app pid={} app={:?} restricted_token={} profile={} job={}",
        response.process.process_id,
        response.process.app_id,
        response.restricted_token_applied,
        response.profile_applied,
        response.job_assigned
    );

    Ok(())
}

fn request_service(request: IpcRequest) -> sandbox_common::Result<IpcResponse> {
    send_request(SERVICE_PIPE_NAME, &request)
}

fn unexpected_response(response: IpcResponse) -> sandbox_common::SandboxError {
    match response {
        IpcResponse::Error(error) => {
            sandbox_common::SandboxError::System(format!("{}: {}", error.code, error.message))
        }
        other => {
            sandbox_common::SandboxError::System(format!("unexpected service response: {other:?}"))
        }
    }
}

fn ensure_controller_desktop(desktop_name: Option<&str>) -> sandbox_common::Result<()> {
    let instance = if desktop_name.is_none() {
        Some(active_instance()?)
    } else {
        None
    };
    let desktop_name = desktop_name
        .map(str::to_string)
        .or_else(|| {
            instance
                .as_ref()
                .map(|instance| instance.desktop_name.clone())
        })
        .ok_or_else(|| SandboxError::System("active desktop resolution failed".to_string()))?;
    let ready = ensure_desktop(&desktop_name)?;

    println!("desktop '{}' is ready ({})", ready.name, ready.source);

    Ok(())
}

fn enter_desktop(desktop_name: Option<&str>) -> sandbox_common::Result<()> {
    let instance = if desktop_name.is_none() {
        Some(active_instance()?)
    } else {
        None
    };
    let desktop_name = desktop_name
        .map(str::to_string)
        .or_else(|| {
            instance
                .as_ref()
                .map(|instance| instance.desktop_name.clone())
        })
        .ok_or_else(|| SandboxError::System("active desktop resolution failed".to_string()))?;
    let ready = ensure_desktop(&desktop_name)?;
    let result = switch_to_desktop(&desktop_name)?;

    if let Some(instance) = instance {
        let response =
            request_service(IpcRequest::UpdateSessionState(UpdateSessionStateRequest {
                sandbox_id: instance.id,
                state: SandboxState::Running,
                reason: Some("controller entered sandbox desktop".to_string()),
            }))?;
        if !matches!(response, IpcResponse::Status(_)) {
            return Err(unexpected_response(response));
        }
    }

    println!("switched to '{}' in {} ms", result.name, result.elapsed_ms);
    println!("desktop source: {}", ready.source);

    Ok(())
}

fn return_to_host() -> sandbox_common::Result<()> {
    let result = switch_to_default_desktop()?;

    println!("switched to '{}' in {} ms", result.name, result.elapsed_ms);

    Ok(())
}

fn launch_on_desktop(
    desktop_name: Option<&str>,
    command_line: Option<&str>,
) -> sandbox_common::Result<()> {
    let (desktop_name, command_line) = match (desktop_name, command_line) {
        (Some(desktop_name), Some(command_line)) => (desktop_name.to_string(), command_line),
        (Some(command_line), None) => (active_instance()?.desktop_name, command_line),
        (None, None) => (active_instance()?.desktop_name, "notepad.exe"),
        (None, Some(_)) => unreachable!("argument parser cannot produce this shape"),
    };
    let ready = ensure_desktop(&desktop_name)?;
    let process = spawn_on_desktop(command_line, &desktop_name)?;

    println!("desktop '{}' is ready ({})", ready.name, ready.source);
    println!(
        "started process {} on WinSta0\\{}",
        process.process_id, desktop_name
    );

    Ok(())
}

fn start_workspace(instance: &SandboxInstance) -> sandbox_common::Result<()> {
    if std::env::var("SANDBOX_PLUS_SHELL_FALLBACK")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        start_legacy_shell(instance)?;
        return Ok(());
    }

    start_explorer(instance)?;
    start_agent(instance)?;
    Ok(())
}

fn start_explorer(instance: &SandboxInstance) -> sandbox_common::Result<()> {
    let explorer = std::path::PathBuf::from(r"C:\Windows\explorer.exe");
    let command = format!("{} /separate", quote_arg(&explorer.display().to_string()));
    let process = spawn_on_desktop(&command, &instance.desktop_name)?;
    let response = request_service(IpcRequest::RecordProcess {
        sandbox_id: instance.id.clone(),
        process: ProcessInfo {
            process_id: process.process_id,
            app_id: Some("sandbox-explorer".to_string()),
            executable: explorer,
            state: ProcessState::Running,
        },
    })?;
    if !matches!(response, IpcResponse::Ok) {
        return Err(unexpected_response(response));
    }
    println!(
        "started explorer process {} on WinSta0\\{}",
        process.process_id, instance.desktop_name
    );
    Ok(())
}

fn start_agent(instance: &SandboxInstance) -> sandbox_common::Result<()> {
    let agent = agent_executable()?;
    if !agent.exists() {
        return Err(SandboxError::System(format!(
            "sandbox agent executable not found: {}",
            agent.display()
        )));
    }

    let command = format!(
        "{} --sandbox-id {} --desktop-name {}",
        quote_arg(&agent.display().to_string()),
        quote_arg(&instance.id.0),
        quote_arg(&instance.desktop_name)
    );
    let process = spawn_on_desktop(&command, &instance.desktop_name)?;
    let response = request_service(IpcRequest::RecordProcess {
        sandbox_id: instance.id.clone(),
        process: ProcessInfo {
            process_id: process.process_id,
            app_id: Some("sandbox-agent".to_string()),
            executable: agent,
            state: ProcessState::Running,
        },
    })?;
    if !matches!(response, IpcResponse::Ok) {
        return Err(unexpected_response(response));
    }
    println!(
        "started agent process {} on WinSta0\\{}",
        process.process_id, instance.desktop_name
    );
    Ok(())
}

fn start_legacy_shell(instance: &SandboxInstance) -> sandbox_common::Result<()> {
    let shell = shell_executable()?;
    if !shell.exists() {
        return Err(SandboxError::System(format!(
            "sandbox shell executable not found: {}",
            shell.display()
        )));
    }

    let command = format!(
        "{} --title {} --sandbox-id {} --desktop-name {}",
        quote_arg(&shell.display().to_string()),
        quote_arg(&format!("Sandbox+ - {}", instance.id.0)),
        quote_arg(&instance.id.0),
        quote_arg(&instance.desktop_name)
    );
    let process = spawn_on_desktop(&command, &instance.desktop_name)?;
    let response = request_service(IpcRequest::RecordProcess {
        sandbox_id: instance.id.clone(),
        process: ProcessInfo {
            process_id: process.process_id,
            app_id: Some("sandbox-shell".to_string()),
            executable: shell,
            state: ProcessState::Running,
        },
    })?;
    if !matches!(response, IpcResponse::Ok) {
        return Err(unexpected_response(response));
    }
    println!(
        "started shell process {} on WinSta0\\{}",
        process.process_id, instance.desktop_name
    );
    Ok(())
}

fn agent_executable() -> sandbox_common::Result<std::path::PathBuf> {
    sibling_executable("sandbox-agent.exe")
}

fn shell_executable() -> sandbox_common::Result<std::path::PathBuf> {
    sibling_executable("sandbox-shell.exe")
}

fn sibling_executable(name: &str) -> sandbox_common::Result<std::path::PathBuf> {
    let current = std::env::current_exe()
        .map_err(|error| SandboxError::System(format!("failed to resolve current exe: {error}")))?;
    let dir = current
        .parent()
        .ok_or_else(|| SandboxError::System("current exe has no parent".to_string()))?;
    Ok(dir.join(name))
}

fn quote_arg(value: &str) -> String {
    if value.contains(' ') || value.contains('\t') {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

fn active_instance() -> sandbox_common::Result<SandboxInstance> {
    let response = request_service(IpcRequest::GetStatus)?;
    let IpcResponse::Status(status) = response else {
        return Err(unexpected_response(response));
    };

    status.instance.ok_or_else(|| {
        SandboxError::Denied(
            "no active sandbox session; run `sandbox-manager create-session <user-sid>` first"
                .to_string(),
        )
    })
}

#[cfg(windows)]
fn run_controller() -> sandbox_common::Result<()> {
    controller::run()
}

#[cfg(not(windows))]
fn run_controller() -> sandbox_common::Result<()> {
    Err(SandboxError::UnsupportedPlatform(
        "controller hotkey/tray mode is only available on Windows".to_string(),
    ))
}

#[cfg(windows)]
mod controller {
    use super::{enter_desktop, return_to_host};
    use sandbox_common::{Result, SandboxError};
    use std::mem::size_of;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL,
    };
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
        DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage, RegisterClassW,
        SetForegroundWindow, TrackPopupMenu, TranslateMessage, CW_USEDEFAULT, HMENU,
        IDI_APPLICATION, MF_STRING, MSG, TPM_RETURNCMD, WM_COMMAND, WM_DESTROY, WM_HOTKEY,
        WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };

    const HOTKEY_ID: i32 = 1001;
    const TRAY_ID: u32 = 2001;
    const WM_TRAYICON: u32 = 0x8001;
    const MENU_ENTER: usize = 3001;
    const MENU_RETURN: usize = 3002;
    const MENU_EXIT: usize = 3003;

    pub fn run() -> Result<()> {
        let window = ControllerWindow::create()?;
        let _tray = TrayIcon::add(window.hwnd)?;
        let _hotkey = Hotkey::register()?;

        println!("sandbox-manager controller running");
        println!("hotkey: Ctrl+Alt+S toggles active Sandbox Desktop and Default");

        window.message_loop()
    }

    struct ControllerWindow {
        hwnd: HWND,
    }

    impl ControllerWindow {
        fn create() -> Result<Self> {
            let class = wide_null("SandboxPlusController");
            let instance = unsafe { GetModuleHandleW(null()) };
            let window_class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(controller_wnd_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..unsafe { std::mem::zeroed() }
            };
            unsafe {
                RegisterClassW(&window_class);
            }

            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    wide_null("Sandbox+ Controller").as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    1,
                    1,
                    null_mut(),
                    null_mut::<std::ffi::c_void>() as HMENU,
                    instance,
                    null(),
                )
            };
            if hwnd.is_null() {
                return Err(last_error("CreateWindowExW(controller)"));
            }

            Ok(Self { hwnd })
        }

        fn message_loop(&self) -> Result<()> {
            let mut in_sandbox = false;
            let mut message: MSG = unsafe { std::mem::zeroed() };
            loop {
                let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
                if result == -1 {
                    return Err(last_error("GetMessageW"));
                }
                if result == 0 {
                    break;
                }

                if message.message == WM_HOTKEY && message.wParam == HOTKEY_ID as usize {
                    in_sandbox = toggle_desktop(in_sandbox);
                    continue;
                }

                if message.message == WM_TRAYICON && message.lParam as u32 == WM_RBUTTONUP {
                    match show_tray_menu(self.hwnd) {
                        Ok(MENU_ENTER) => {
                            if enter_desktop(None).is_ok() {
                                in_sandbox = true;
                            }
                        }
                        Ok(MENU_RETURN) => {
                            if return_to_host().is_ok() {
                                in_sandbox = false;
                            }
                        }
                        Ok(MENU_EXIT) => unsafe {
                            PostQuitMessage(0);
                        },
                        Ok(_) => {}
                        Err(error) => eprintln!("tray menu failed: {error}"),
                    }
                    continue;
                }

                if message.message == WM_COMMAND {
                    match message.wParam & 0xffff {
                        MENU_ENTER => {
                            if enter_desktop(None).is_ok() {
                                in_sandbox = true;
                            }
                        }
                        MENU_RETURN => {
                            if return_to_host().is_ok() {
                                in_sandbox = false;
                            }
                        }
                        MENU_EXIT => unsafe {
                            PostQuitMessage(0);
                        },
                        _ => {}
                    }
                    continue;
                }

                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }

            Ok(())
        }
    }

    fn toggle_desktop(in_sandbox: bool) -> bool {
        let result = if in_sandbox {
            return_to_host()
        } else {
            enter_desktop(None)
        };

        match result {
            Ok(()) => !in_sandbox,
            Err(error) => {
                eprintln!("hotkey action failed: {error}");
                in_sandbox
            }
        }
    }

    fn show_tray_menu(hwnd: HWND) -> Result<usize> {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return Err(last_error("CreatePopupMenu"));
        }

        unsafe {
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_ENTER,
                wide_null("Enter Sandbox").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_RETURN,
                wide_null("Return to Host").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_EXIT,
                wide_null("Exit Controller").as_ptr(),
            );
            SetForegroundWindow(hwnd);
        }

        let mut point: POINT = unsafe { std::mem::zeroed() };
        let ok = unsafe { GetCursorPos(&mut point) };
        if ok == 0 {
            unsafe {
                DestroyMenu(menu);
            }
            return Err(last_error("GetCursorPos"));
        }

        let command = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD,
                point.x,
                point.y,
                0,
                hwnd,
                std::ptr::null(),
            )
        };
        unsafe {
            DestroyMenu(menu);
        }

        Ok(command as usize)
    }

    unsafe extern "system" fn controller_wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_DESTROY {
            PostQuitMessage(0);
            return 0;
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    struct Hotkey;

    impl Hotkey {
        fn register() -> Result<Self> {
            let ok = unsafe {
                RegisterHotKey(null_mut(), HOTKEY_ID, MOD_CONTROL | MOD_ALT, b'S' as u32)
            };
            if ok == 0 {
                return Err(last_error("RegisterHotKey(Ctrl+Alt+S)"));
            }
            Ok(Self)
        }
    }

    impl Drop for Hotkey {
        fn drop(&mut self) {
            unsafe {
                UnregisterHotKey(null_mut(), HOTKEY_ID);
            }
        }
    }

    struct TrayIcon {
        data: NOTIFYICONDATAW,
    }

    impl TrayIcon {
        fn add(hwnd: HWND) -> Result<Self> {
            let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
            data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = hwnd;
            data.uID = TRAY_ID;
            data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            data.uCallbackMessage = WM_TRAYICON;
            data.hIcon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
            copy_wide_fixed("Sandbox+ Controller (Ctrl+Alt+S)", &mut data.szTip);

            let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
            if ok == 0 {
                return Err(last_error("Shell_NotifyIconW(NIM_ADD)"));
            }

            Ok(Self { data })
        }
    }

    impl Drop for TrayIcon {
        fn drop(&mut self) {
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &self.data);
            }
        }
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn copy_wide_fixed(value: &str, target: &mut [u16]) {
        let encoded = wide_null(value);
        let count = encoded.len().min(target.len());
        target[..count].copy_from_slice(&encoded[..count]);
        if let Some(last) = target.last_mut() {
            *last = 0;
        }
    }

    fn last_error(api: &str) -> SandboxError {
        let code = unsafe { GetLastError() };
        SandboxError::System(format!("{api} failed with Win32 error {code}"))
    }
}
