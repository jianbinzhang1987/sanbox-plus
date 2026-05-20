use sandbox_common::{
    CloseSessionMode, CloseSessionRequest, PolicySummary, SandboxError, SandboxId, SandboxStatus,
    ServiceHealth,
};
use sandbox_ipc::{send_request, IpcRequest, IpcResponse, SERVICE_PIPE_NAME};
use std::sync::OnceLock;

static LOG_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();

fn main() {
    agent_log("main: starting sandbox-agent");
    if let Err(error) = run() {
        agent_log(&format!("main: fatal error: {error}"));
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> sandbox_common::Result<()> {
    let options = AgentOptions::parse(std::env::args().skip(1))?;
    if let Some(path) = &options.log_path {
        let _ = LOG_PATH.set(path.clone());
    }
    agent_log(&format!(
        "run: sandbox_id={} desktop={}",
        options.sandbox_id.0, options.desktop_name
    ));
    platform::run(options)
}

fn agent_log(message: &str) {
    let line = format!("{} {message}\r\n", unix_timestamp());
    for path in [
        LOG_PATH.get().cloned().unwrap_or_else(|| {
            std::path::PathBuf::from(r"C:\ProgramData\SandboxPlus\Logs\sandbox-agent.log")
        }),
        std::env::temp_dir().join("sandbox-agent.log"),
        std::path::PathBuf::from(r"C:\ProgramData\SandboxPlus\Logs\sandbox-agent.log"),
    ] {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| {
                use std::io::Write;
                file.write_all(line.as_bytes())
            });
    }
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone)]
struct AgentOptions {
    sandbox_id: SandboxId,
    desktop_name: String,
    log_path: Option<std::path::PathBuf>,
}

impl AgentOptions {
    fn parse(mut args: impl Iterator<Item = String>) -> sandbox_common::Result<Self> {
        let mut sandbox_id = None;
        let mut desktop_name = None;
        let mut log_path = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--sandbox-id" => sandbox_id = args.next().map(SandboxId),
                "--desktop-name" => desktop_name = args.next(),
                "--log-path" => log_path = args.next().map(std::path::PathBuf::from),
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                other => {
                    return Err(SandboxError::Configuration(format!(
                        "unknown sandbox-agent argument: {other}"
                    )));
                }
            }
        }

        Ok(Self {
            sandbox_id: sandbox_id.ok_or_else(|| {
                SandboxError::Configuration("sandbox-agent requires --sandbox-id".to_string())
            })?,
            desktop_name: desktop_name.ok_or_else(|| {
                SandboxError::Configuration("sandbox-agent requires --desktop-name".to_string())
            })?,
            log_path,
        })
    }
}

fn print_usage() {
    eprintln!("usage: sandbox-agent --sandbox-id <id> --desktop-name <desktop>");
}

fn request_service(request: IpcRequest) -> sandbox_common::Result<IpcResponse> {
    send_request(SERVICE_PIPE_NAME, &request)
}

fn close_session(sandbox_id: &SandboxId, mode: CloseSessionMode) -> sandbox_common::Result<()> {
    let response = request_service(IpcRequest::CloseSession(CloseSessionRequest {
        sandbox_id: sandbox_id.clone(),
        mode,
    }))?;
    if matches!(response, IpcResponse::Ok) {
        Ok(())
    } else {
        Err(unexpected_response(response))
    }
}

fn return_to_host(sandbox_id: &SandboxId) -> sandbox_common::Result<()> {
    let response = request_service(IpcRequest::ReturnToHost {
        sandbox_id: sandbox_id.clone(),
    })?;
    if matches!(response, IpcResponse::Ok) {
        Ok(())
    } else {
        Err(unexpected_response(response))
    }
}

fn query_status(sandbox_id: &SandboxId) -> Option<(SandboxStatus, Option<PolicySummary>)> {
    let status = match request_service(IpcRequest::GetStatus) {
        Ok(IpcResponse::Status(status)) => status,
        _ => return None,
    };
    let policy = match request_service(IpcRequest::GetPolicySummary {
        sandbox_id: sandbox_id.clone(),
    }) {
        Ok(IpcResponse::PolicySummary(p)) => Some(p),
        _ => None,
    };
    Some((status, policy))
}

fn unexpected_response(response: IpcResponse) -> SandboxError {
    match response {
        IpcResponse::Error(error) => {
            SandboxError::System(format!("{}: {}", error.code, error.message))
        }
        other => SandboxError::System(format!("unexpected service response: {other:?}")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentDisplayState {
    Connected,
    Disconnected,
    Degraded,
    NoSession,
}

impl AgentDisplayState {
    fn from_health(health: &ServiceHealth) -> Self {
        match health {
            ServiceHealth::Healthy => Self::Connected,
            ServiceHealth::Degraded(_) => Self::Degraded,
            ServiceHealth::FailClosed(_) => Self::NoSession,
        }
    }

    fn button_color(self) -> u32 {
        match self {
            Self::Connected => 0x00005ca8,
            Self::Disconnected => 0x000000c8,
            Self::Degraded => 0x0000a8c8,
            Self::NoSession => 0x00808080,
        }
    }

    fn border_color(self) -> u32 {
        match self {
            Self::Connected => 0x00004888,
            Self::Disconnected => 0x000000a0,
            Self::Degraded => 0x000088a0,
            Self::NoSession => 0x00606060,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Connected => "Return Host",
            Self::Disconnected => "! Disconnected",
            Self::Degraded => "! Degraded",
            Self::NoSession => "No Session",
        }
    }

    fn status_text(self) -> &'static str {
        match self {
            Self::Connected => "Connected",
            Self::Disconnected => "Disconnected",
            Self::Degraded => "Degraded",
            Self::NoSession => "Inactive",
        }
    }
}

const POSITION_FILE: &str = r"C:\ProgramData\SandboxPlus\agent-position.dat";

fn load_saved_position() -> Option<(i32, i32)> {
    let data = std::fs::read_to_string(POSITION_FILE).ok()?;
    let mut parts = data.trim().split(',');
    let x: i32 = parts.next()?.parse().ok()?;
    let y: i32 = parts.next()?.parse().ok()?;
    if x >= 0 && y >= 0 {
        Some((x, y))
    } else {
        None
    }
}

fn save_position(x: i32, y: i32) {
    if let Some(parent) = std::path::Path::new(POSITION_FILE).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(POSITION_FILE, format!("{x},{y}"));
}

#[cfg(windows)]
mod platform {
    use super::{
        close_session, load_saved_position, query_status, return_to_host, save_position,
        AgentDisplayState, AgentOptions,
    };
    use sandbox_common::{CloseSessionMode, Result, SandboxError};
    use std::ptr::{null, null_mut};
    use std::sync::{Mutex, OnceLock};
    use windows_sys::Win32::Foundation::{
        GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, TRUE, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect,
        InvalidateRect, SetBkMode, SetTextColor, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HBRUSH,
        PAINTSTRUCT, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
        DispatchMessageW, GetCursorPos, GetMessageW, GetWindowRect, KillTimer, PostQuitMessage,
        RegisterClassW, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, TrackPopupMenu,
        TranslateMessage, HMENU, LWA_ALPHA, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, SM_CXSCREEN,
        SM_CYSCREEN, TPM_RETURNCMD, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_NCHITTEST, WM_PAINT,
        WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
    };

    const HTCAPTION: LRESULT = 2;
    const TIMER_STATUS: usize = 5001;
    const TIMER_SAVE_POS: usize = 5002;
    const STATUS_INTERVAL_MS: u32 = 3000;
    const SAVE_POS_INTERVAL_MS: u32 = 10000;
    const BUTTON_WIDTH: i32 = 128;
    const BUTTON_HEIGHT: i32 = 52;
    const BUTTON_ALPHA: u8 = 220;

    const MENU_RETURN: usize = 4101;
    const MENU_CLOSE: usize = 4102;
    const MENU_RESET: usize = 4103;
    const MENU_IMPORT: usize = 4105;
    const MENU_EXPORT: usize = 4106;
    const MENU_EXIT: usize = 4104;

    struct AgentState {
        options: AgentOptions,
        display_state: AgentDisplayState,
        policy_version: Option<String>,
        network_mode: Option<String>,
    }

    static STATE: OnceLock<Mutex<AgentState>> = OnceLock::new();

    pub fn run(options: AgentOptions) -> Result<()> {
        super::agent_log("platform: initializing state");
        let _ = STATE.set(Mutex::new(AgentState {
            options: options.clone(),
            display_state: AgentDisplayState::Connected,
            policy_version: None,
            network_mode: None,
        }));
        super::agent_log("platform: creating agent window");
        let window = AgentWindow::create(&options)?;
        super::agent_log("platform: creating watermark window");
        let _watermark = create_watermark_window();
        println!(
            "sandbox-agent running for {} on WinSta0\\{}",
            options.sandbox_id.0, options.desktop_name
        );
        super::agent_log("platform: entering message loop");
        window.message_loop()
    }

    struct AgentWindow {
        #[allow(dead_code)]
        hwnd: HWND,
    }

    impl AgentWindow {
        fn create(options: &AgentOptions) -> Result<Self> {
            let class = wide_null("SandboxPlusAgentButton");
            let instance = unsafe { GetModuleHandleW(null()) };
            let window_class = WNDCLASSW {
                lpfnWndProc: Some(agent_wnd_proc),
                hInstance: instance,
                lpszClassName: class.as_ptr(),
                ..unsafe { std::mem::zeroed() }
            };
            unsafe {
                RegisterClassW(&window_class);
            }

            let (x, y) = load_saved_position().unwrap_or_else(|| default_position());

            let title = wide_null(&format!("Sandbox+ {}", options.sandbox_id.0));
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                    class.as_ptr(),
                    title.as_ptr(),
                    WS_POPUP | WS_VISIBLE,
                    x,
                    y,
                    BUTTON_WIDTH,
                    BUTTON_HEIGHT,
                    null_mut(),
                    null_mut::<std::ffi::c_void>() as HMENU,
                    instance,
                    null(),
                )
            };
            if hwnd.is_null() {
                super::agent_log("window: CreateWindowExW(agent) failed");
                return Err(last_error("CreateWindowExW(agent)"));
            }

            unsafe {
                SetLayeredWindowAttributes(hwnd, 0, BUTTON_ALPHA, LWA_ALPHA);
                SetTimer(hwnd, TIMER_STATUS, STATUS_INTERVAL_MS, None);
                SetTimer(hwnd, TIMER_SAVE_POS, SAVE_POS_INTERVAL_MS, None);
            }

            Ok(Self { hwnd })
        }

        fn message_loop(&self) -> Result<()> {
            let mut message: MSG = unsafe { std::mem::zeroed() };
            loop {
                let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
                if result == -1 {
                    super::agent_log("message_loop: GetMessageW failed");
                    return Err(last_error("GetMessageW"));
                }
                if result == 0 {
                    super::agent_log("message_loop: WM_QUIT received");
                    break;
                }
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            Ok(())
        }
    }

    fn default_position() -> (i32, i32) {
        let screen_w =
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(SM_CXSCREEN) };
        let screen_h =
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(SM_CYSCREEN) };
        (
            screen_w - BUTTON_WIDTH - 32,
            screen_h / 2 - BUTTON_HEIGHT / 2,
        )
    }

    unsafe extern "system" fn agent_wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCHITTEST => HTCAPTION,
            WM_PAINT => {
                paint_button(hwnd);
                0
            }
            WM_LBUTTONUP => {
                with_state(|state| {
                    return_to_host(&state.options.sandbox_id)
                        .or_else(|_| sandbox_desktop::switch_to_default_desktop().map(|_| ()))
                });
                0
            }
            WM_RBUTTONUP => {
                if let Ok(command) = show_context_menu(hwnd) {
                    handle_menu_command(command);
                }
                0
            }
            WM_COMMAND => {
                handle_menu_command(wparam & 0xffff);
                0
            }
            WM_TIMER => {
                match wparam {
                    TIMER_STATUS => {
                        refresh_status();
                        InvalidateRect(hwnd, null(), TRUE);
                    }
                    TIMER_SAVE_POS => {
                        save_current_position(hwnd);
                    }
                    _ => {}
                }
                0
            }
            WM_DESTROY => {
                save_current_position(hwnd);
                unsafe {
                    KillTimer(hwnd, TIMER_STATUS);
                    KillTimer(hwnd, TIMER_SAVE_POS);
                }
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    fn refresh_status() {
        let Some(state_lock) = STATE.get() else {
            return;
        };
        let sandbox_id = {
            let state = state_lock.lock().unwrap();
            state.options.sandbox_id.clone()
        };

        let (new_display, policy_ver, net_mode) =
            if let Some((status, policy)) = query_status(&sandbox_id) {
                let display = AgentDisplayState::from_health(&status.health);
                let ver = policy.as_ref().map(|p| p.version.clone());
                let net = policy.as_ref().map(|p| format!("{:?}", p.network_mode));
                (display, ver, net)
            } else {
                (AgentDisplayState::Disconnected, None, None)
            };

        if let Ok(mut state) = state_lock.lock() {
            state.display_state = new_display;
            state.policy_version = policy_ver;
            state.network_mode = net_mode;
        }
    }

    fn save_current_position(hwnd: HWND) {
        let mut rect: RECT = unsafe { std::mem::zeroed() };
        if unsafe { GetWindowRect(hwnd, &mut rect) } != 0 {
            save_position(rect.left, rect.top);
        }
    }

    fn handle_menu_command(command: usize) {
        match command {
            MENU_RETURN => with_state(|state| return_to_host(&state.options.sandbox_id)),
            MENU_CLOSE => with_state(|state| {
                close_session(&state.options.sandbox_id, CloseSessionMode::KeepProfile)
            }),
            MENU_RESET => with_state(|state| {
                close_session(&state.options.sandbox_id, CloseSessionMode::ResetProfile)
            }),
            MENU_IMPORT => {
                if let Some(lock) = STATE.get() {
                    if let Ok(state) = lock.lock() {
                        let _ = super::request_service(sandbox_ipc::IpcRequest::ImportFiles {
                            sandbox_id: state.options.sandbox_id.clone(),
                        });
                    }
                }
            }
            MENU_EXPORT => {
                if let Some(lock) = STATE.get() {
                    if let Ok(state) = lock.lock() {
                        let _ = super::request_service(sandbox_ipc::IpcRequest::ExportFiles {
                            sandbox_id: state.options.sandbox_id.clone(),
                        });
                    }
                }
            }
            MENU_EXIT => unsafe {
                PostQuitMessage(0);
            },
            _ => {}
        }
    }

    fn with_state(action: impl FnOnce(&AgentState) -> Result<()>) {
        if let Some(lock) = STATE.get() {
            if let Ok(state) = lock.lock() {
                if let Err(error) = action(&state) {
                    eprintln!("sandbox-agent action failed: {error}");
                }
            }
        }
    }

    fn current_display_state() -> AgentDisplayState {
        STATE
            .get()
            .and_then(|lock| lock.lock().ok())
            .map(|state| state.display_state)
            .unwrap_or(AgentDisplayState::NoSession)
    }

    fn show_context_menu(hwnd: HWND) -> Result<usize> {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return Err(last_error("CreatePopupMenu"));
        }

        let (status_label, policy_label, network_label) = {
            let info = STATE.get().and_then(|lock| lock.lock().ok());
            let status = info
                .as_ref()
                .map(|s| s.display_state.status_text())
                .unwrap_or("Unknown");
            let policy = info
                .as_ref()
                .and_then(|s| s.policy_version.clone())
                .unwrap_or_else(|| "N/A".to_string());
            let network = info
                .as_ref()
                .and_then(|s| s.network_mode.clone())
                .unwrap_or_else(|| "N/A".to_string());
            (
                format!("Status: {status}"),
                format!("Policy: {policy}"),
                format!("Network: {network}"),
            )
        };

        unsafe {
            AppendMenuW(
                menu,
                MF_STRING | MF_GRAYED,
                0,
                wide_null(&status_label).as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_RETURN,
                wide_null("Return to Host").as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_IMPORT,
                wide_null("Import Files...").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_EXPORT,
                wide_null("Export Files...").as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(
                menu,
                MF_STRING | MF_GRAYED,
                0,
                wide_null(&policy_label).as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING | MF_GRAYED,
                0,
                wide_null(&network_label).as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_CLOSE,
                wide_null("Close Sandbox").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_RESET,
                wide_null("Reset Sandbox").as_ptr(),
            );
            SetForegroundWindow(hwnd);
        }

        let mut point: POINT = unsafe { std::mem::zeroed() };
        if unsafe { GetCursorPos(&mut point) } == 0 {
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

    fn paint_button(hwnd: HWND) {
        let display = current_display_state();
        let mut paint: PAINTSTRUCT = unsafe { std::mem::zeroed() };
        let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
        let rect = RECT {
            left: 0,
            top: 0,
            right: BUTTON_WIDTH,
            bottom: BUTTON_HEIGHT,
        };
        let background = unsafe { CreateSolidBrush(display.button_color()) };
        let border = unsafe { CreateSolidBrush(display.border_color()) };
        fill_rect(hdc, &rect, background);
        frame_rect(hdc, &rect, border);
        unsafe {
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, 0x00ffffff);
        }
        let mut text_rect = rect;
        let text = wide_null(display.label());
        unsafe {
            DrawTextW(
                hdc,
                text.as_ptr(),
                -1,
                &mut text_rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            DeleteObject(background as _);
            DeleteObject(border as _);
            EndPaint(hwnd, &paint);
        }
    }

    fn fill_rect(hdc: windows_sys::Win32::Graphics::Gdi::HDC, rect: &RECT, brush: HBRUSH) {
        unsafe {
            FillRect(hdc, rect, brush);
        }
    }

    fn frame_rect(hdc: windows_sys::Win32::Graphics::Gdi::HDC, rect: &RECT, brush: HBRUSH) {
        unsafe {
            FrameRect(hdc, rect, brush);
        }
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error(api: &str) -> SandboxError {
        let code = unsafe { GetLastError() };
        SandboxError::System(format!("{api} failed with Win32 error {code}"))
    }

    const WATERMARK_WIDTH: i32 = 300;
    const WATERMARK_HEIGHT: i32 = 40;
    const WATERMARK_ALPHA: u8 = 100;

    fn create_watermark_window() -> Option<HWND> {
        let class = wide_null("SandboxPlusWatermark");
        let instance = unsafe { GetModuleHandleW(null()) };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(watermark_wnd_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..unsafe { std::mem::zeroed() }
        };
        unsafe {
            RegisterClassW(&wc);
        }

        let screen_w =
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(SM_CXSCREEN) };
        let screen_h =
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(SM_CYSCREEN) };
        let x = screen_w - WATERMARK_WIDTH - 16;
        let y = screen_h - WATERMARK_HEIGHT - 48;

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST
                    | WS_EX_TRANSPARENT
                    | WS_EX_LAYERED
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                wide_null("Sandbox+ Watermark").as_ptr(),
                WS_POPUP | WS_VISIBLE,
                x,
                y,
                WATERMARK_WIDTH,
                WATERMARK_HEIGHT,
                null_mut(),
                null_mut::<std::ffi::c_void>() as HMENU,
                instance,
                null(),
            )
        };
        if hwnd.is_null() {
            return None;
        }
        unsafe {
            SetLayeredWindowAttributes(hwnd, 0, WATERMARK_ALPHA, LWA_ALPHA);
        }
        Some(hwnd)
    }

    unsafe extern "system" fn watermark_wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_PAINT => {
                paint_watermark(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    fn paint_watermark(hwnd: HWND) {
        let mut paint: PAINTSTRUCT = unsafe { std::mem::zeroed() };
        let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
        let rect = RECT {
            left: 0,
            top: 0,
            right: WATERMARK_WIDTH,
            bottom: WATERMARK_HEIGHT,
        };
        unsafe {
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, 0x00ffffff);
        }
        let mut text_rect = rect;
        let text = wide_null("SANDBOX+");
        unsafe {
            DrawTextW(
                hdc,
                text.as_ptr(),
                -1,
                &mut text_rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            EndPaint(hwnd, &paint);
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::AgentOptions;
    use sandbox_common::{Result, SandboxError};

    pub fn run(_options: AgentOptions) -> Result<()> {
        Err(SandboxError::UnsupportedPlatform(
            "sandbox-agent is only available on Windows".to_string(),
        ))
    }
}
