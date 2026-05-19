use sandbox_common::{
    AppSummary, CloseSessionMode, CloseSessionRequest, LaunchAppRequest, PolicySummary,
    ProcessInfo, SandboxId, SandboxInstance, SandboxStatus,
};
use sandbox_ipc::{send_request, IpcRequest, IpcResponse, SERVICE_PIPE_NAME};
use std::process::ExitCode;

fn main() -> ExitCode {
    let options = match ShellOptions::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };

    match run_shell(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellOptions {
    title: String,
    sandbox_id: Option<SandboxId>,
    desktop_name: Option<String>,
    refresh_ms: u32,
}

impl ShellOptions {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut options = Self {
            title: "Sandbox+ Workspace".to_string(),
            sandbox_id: None,
            desktop_name: None,
            refresh_ms: 2000,
        };

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--title" => options.title = next_non_empty(&mut args, "--title")?,
                "--sandbox-id" => {
                    options.sandbox_id = Some(SandboxId(next_non_empty(&mut args, "--sandbox-id")?))
                }
                "--desktop-name" => {
                    options.desktop_name = Some(next_non_empty(&mut args, "--desktop-name")?)
                }
                "--refresh-ms" => {
                    let value = next_non_empty(&mut args, "--refresh-ms")?;
                    options.refresh_ms = value
                        .parse()
                        .map_err(|_| "--refresh-ms must be an integer".to_string())?;
                    if options.refresh_ms < 500 {
                        return Err("--refresh-ms must be at least 500".to_string());
                    }
                }
                "--help" | "-h" => return Err(usage().to_string()),
                unknown => return Err(format!("unknown argument: {unknown}\n{}", usage())),
            }
        }

        Ok(options)
    }
}

fn next_non_empty(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    let value = args
        .next()
        .ok_or_else(|| format!("{name} requires a value"))?;
    if value.trim().is_empty() {
        return Err(format!("{name} must not be empty"));
    }
    Ok(value)
}

fn usage() -> &'static str {
    "usage: sandbox-shell [--title <title>] [--sandbox-id <id>] [--desktop-name <name>] [--refresh-ms <ms>]"
}

#[cfg(windows)]
fn run_shell(options: ShellOptions) -> sandbox_common::Result<()> {
    windows_shell::run(options)
}

#[cfg(not(windows))]
fn run_shell(_options: ShellOptions) -> sandbox_common::Result<()> {
    Err(sandbox_common::SandboxError::UnsupportedPlatform(
        "sandbox shell is only available on Windows".to_string(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ButtonAction {
    Refresh,
    ReturnHost,
    CloseSession,
    ResetSession,
    LaunchApp(String),
    ActivateProcess(u32),
    MinimizeProcess(u32),
    CloseProcess(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellButton {
    rect: UiRect,
    action: ButtonAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UiRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl UiRect {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x <= self.right && y >= self.top && y <= self.bottom
    }
}

struct ShellModel {
    options: ShellOptions,
    instance: Option<SandboxInstance>,
    health: String,
    policy: Option<PolicySummary>,
    apps: Vec<AppSummary>,
    processes: Vec<ProcessInfo>,
    message: String,
    buttons: Vec<ShellButton>,
}

impl ShellModel {
    fn new(options: ShellOptions) -> Self {
        Self {
            options,
            instance: None,
            health: "Starting".to_string(),
            policy: None,
            apps: Vec::new(),
            processes: Vec::new(),
            message: "Connecting to Sandbox+ Service".to_string(),
            buttons: Vec::new(),
        }
    }

    fn refresh(&mut self) {
        match self.load_service_state() {
            Ok(()) => {
                self.message = format!(
                    "Ready · {} app(s) · {} process(es)",
                    self.apps.len(),
                    self.processes.len()
                );
            }
            Err(error) => {
                self.message = error;
            }
        }
    }

    fn load_service_state(&mut self) -> Result<(), String> {
        let IpcResponse::Status(status) = request(IpcRequest::GetStatus)? else {
            return Err("service returned an unexpected status response".to_string());
        };
        self.apply_status(status)?;

        let Some(instance) = self.instance.clone() else {
            self.apps.clear();
            self.processes.clear();
            self.policy = None;
            return Ok(());
        };

        let IpcResponse::PolicySummary(policy) = request(IpcRequest::GetPolicySummary {
            sandbox_id: instance.id.clone(),
        })?
        else {
            return Err("service returned an unexpected policy response".to_string());
        };
        self.policy = Some(policy);

        let IpcResponse::Apps(apps) = request(IpcRequest::ListApps {
            sandbox_id: instance.id.clone(),
        })?
        else {
            return Err("service returned an unexpected apps response".to_string());
        };
        self.apps = apps;

        let IpcResponse::Processes(processes) = request(IpcRequest::ListProcesses {
            sandbox_id: instance.id,
        })?
        else {
            return Err("service returned an unexpected process response".to_string());
        };
        self.processes = processes;

        Ok(())
    }

    fn apply_status(&mut self, status: SandboxStatus) -> Result<(), String> {
        self.health = format!("{:?}", status.health);
        let instance = status.instance;
        if let Some(expected) = &self.options.sandbox_id {
            if let Some(active) = &instance {
                if &active.id != expected {
                    return Err(format!(
                        "active session '{}' does not match shell session '{}'",
                        active.id.0, expected.0
                    ));
                }
            }
        }
        self.instance = instance;
        Ok(())
    }

    fn handle_action(&mut self, action: ButtonAction) -> bool {
        let result = match action {
            ButtonAction::Refresh => {
                self.refresh();
                return false;
            }
            ButtonAction::ReturnHost => self.return_host(),
            ButtonAction::CloseSession => self.close_session(CloseSessionMode::KeepProfile),
            ButtonAction::ResetSession => self.close_session(CloseSessionMode::ResetProfile),
            ButtonAction::LaunchApp(ref app_id) => self.launch_app(app_id),
            ButtonAction::ActivateProcess(pid) => {
                activate_process_window(pid);
                Ok(())
            }
            ButtonAction::MinimizeProcess(pid) => {
                minimize_process_window(pid);
                Ok(())
            }
            ButtonAction::CloseProcess(pid) => {
                close_process_window(pid);
                Ok(())
            }
        };

        match result {
            Ok(()) => {
                self.refresh();
                matches!(
                    action,
                    ButtonAction::CloseSession | ButtonAction::ResetSession
                )
            }
            Err(error) => {
                self.message = error;
                false
            }
        }
    }

    fn return_host(&self) -> Result<(), String> {
        let instance = self.require_instance()?;
        expect_ok(request(IpcRequest::ReturnToHost {
            sandbox_id: instance.id.clone(),
        })?)
    }

    fn close_session(&self, mode: CloseSessionMode) -> Result<(), String> {
        let instance = self.require_instance()?;
        expect_ok(request(IpcRequest::CloseSession(CloseSessionRequest {
            sandbox_id: instance.id.clone(),
            mode,
        }))?)
    }

    fn launch_app(&self, app_id: &str) -> Result<(), String> {
        let instance = self.require_instance()?;
        let app = self
            .apps
            .iter()
            .find(|app| app.id == app_id)
            .ok_or_else(|| format!("policy app '{app_id}' was not found"))?;

        let response = request(IpcRequest::LaunchApp(LaunchAppRequest {
            sandbox_id: instance.id.clone(),
            app_id: app.id.clone(),
            executable: app.executable.clone(),
            arguments: app.arguments.clone(),
            working_directory: app.working_directory.clone(),
            desktop_name: instance.desktop_name.clone(),
            profile_root: instance.profile_root.clone(),
            environment_overrides: Vec::new(),
            policy_version: instance.policy_version.clone(),
        }))?;

        match response {
            IpcResponse::LaunchApp(_) => Ok(()),
            other => Err(format!(
                "service returned unexpected launch response: {other:?}"
            )),
        }
    }

    fn require_instance(&self) -> Result<&SandboxInstance, String> {
        self.instance
            .as_ref()
            .ok_or_else(|| "no active sandbox session".to_string())
    }
}

fn request(request: IpcRequest) -> Result<IpcResponse, String> {
    send_request(SERVICE_PIPE_NAME, &request).map_err(|error| error.to_string())
}

fn expect_ok(response: IpcResponse) -> Result<(), String> {
    match response {
        IpcResponse::Ok => Ok(()),
        IpcResponse::Error(error) => Err(format!("{}: {}", error.code, error.message)),
        other => Err(format!("service returned unexpected response: {other:?}")),
    }
}

#[cfg(windows)]
mod windows_shell {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, TRUE, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect,
        InvalidateRect, SetBkMode, SetTextColor, DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
        DT_WORDBREAK, PAINTSTRUCT, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, GetClientRect, GetMessageW,
        GetWindowLongPtrW, GetWindowThreadProcessId, IsWindowVisible, KillTimer, PostMessageW,
        PostQuitMessage, RegisterClassW, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
        ShowWindow, TranslateMessage, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HMENU,
        MSG, SM_CXSCREEN, SM_CYSCREEN, SW_MINIMIZE, SW_RESTORE, WM_CLOSE, WM_CREATE, WM_DESTROY,
        WM_LBUTTONUP, WM_PAINT, WM_TIMER, WNDCLASSW, WS_POPUP, WS_VISIBLE,
    };

    const TIMER_ID: usize = 1;
    const DARK_BACKGROUND: u32 = 0x00202020;
    const PANEL: u32 = 0x00303030;
    const CARD: u32 = 0x00404040;
    const ACCENT: u32 = 0x00A85C00;
    const TEXT: u32 = 0x00F2F2F2;
    const MUTED: u32 = 0x00B8B8B8;

    pub fn run(options: ShellOptions) -> sandbox_common::Result<()> {
        let mut model = Box::new(ShellModel::new(options));
        model.refresh();

        let class = wide_null("SandboxPlusProductShell");
        let instance = unsafe { GetModuleHandleW(null()) };
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(shell_wnd_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..unsafe { std::mem::zeroed() }
        };
        unsafe {
            RegisterClassW(&window_class);
        }

        let width =
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(SM_CXSCREEN) };
        let height =
            unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(SM_CYSCREEN) };
        let title = wide_null(&model.options.title);
        let model_ptr = Box::into_raw(model);
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                width,
                height,
                null_mut(),
                null_mut::<std::ffi::c_void>() as HMENU,
                instance,
                model_ptr.cast(),
            )
        };
        if hwnd.is_null() {
            unsafe {
                drop(Box::from_raw(model_ptr));
            }
            return Err(last_error("CreateWindowExW(shell)"));
        }

        unsafe {
            SetTimer(hwnd, TIMER_ID, (*model_ptr).options.refresh_ms, None);
        }

        let mut message: MSG = unsafe { std::mem::zeroed() };
        loop {
            let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
            if result == -1 {
                unsafe {
                    KillTimer(hwnd, TIMER_ID);
                    drop(Box::from_raw(model_ptr));
                }
                return Err(last_error("GetMessageW(shell)"));
            }
            if result == 0 {
                break;
            }
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        unsafe {
            KillTimer(hwnd, TIMER_ID);
            drop(Box::from_raw(model_ptr));
        }
        Ok(())
    }

    unsafe extern "system" fn shell_wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_CREATE => {
                let create = lparam as *const CREATESTRUCTW;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
                0
            }
            WM_TIMER => {
                if wparam == TIMER_ID {
                    if let Some(model) = model_from_hwnd(hwnd) {
                        model.refresh();
                        InvalidateRect(hwnd, null(), TRUE);
                    }
                    return 0;
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_LBUTTONUP => {
                let x = (lparam as u32 & 0xffff) as i16 as i32;
                let y = ((lparam as u32 >> 16) & 0xffff) as i16 as i32;
                if let Some(model) = model_from_hwnd(hwnd) {
                    let action = model
                        .buttons
                        .iter()
                        .find(|button| button.rect.contains(x, y))
                        .map(|button| button.action.clone());
                    if let Some(action) = action {
                        let exit_shell = model.handle_action(action);
                        InvalidateRect(hwnd, null(), TRUE);
                        if exit_shell {
                            PostQuitMessage(0);
                        }
                    }
                }
                0
            }
            WM_PAINT => {
                if let Some(model) = model_from_hwnd(hwnd) {
                    paint(hwnd, model);
                    return 0;
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe fn model_from_hwnd(hwnd: HWND) -> Option<&'static mut ShellModel> {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ShellModel;
        ptr.as_mut()
    }

    unsafe fn paint(hwnd: HWND, model: &mut ShellModel) {
        model.buttons.clear();

        let mut paint: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut paint);
        let mut rect: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rect);

        fill(hdc, rect, DARK_BACKGROUND);
        draw_header(hdc, model, rect);
        draw_workspace(hdc, model, rect);
        draw_taskbar(hdc, model, rect);

        EndPaint(hwnd, &paint);
    }

    unsafe fn draw_header(
        hdc: windows_sys::Win32::Graphics::Gdi::HDC,
        model: &mut ShellModel,
        bounds: RECT,
    ) {
        let header = RECT {
            left: 0,
            top: 0,
            right: bounds.right,
            bottom: 72,
        };
        fill(hdc, header, PANEL);

        text(
            hdc,
            "Sandbox+",
            24,
            18,
            220,
            28,
            TEXT,
            DT_LEFT | DT_SINGLELINE,
        );
        text(
            hdc,
            &format!("Health: {}", model.health),
            190,
            20,
            420,
            24,
            MUTED,
            DT_LEFT | DT_SINGLELINE,
        );

        if let Some(instance) = &model.instance {
            text(
                hdc,
                &format!(
                    "Session: {} · Desktop: {}",
                    instance.id.0, instance.desktop_name
                ),
                520,
                20,
                bounds.right - 900,
                24,
                MUTED,
                DT_LEFT | DT_SINGLELINE,
            );
        } else {
            text(
                hdc,
                "No active sandbox session",
                520,
                20,
                bounds.right - 900,
                24,
                MUTED,
                DT_LEFT | DT_SINGLELINE,
            );
        }

        let mut x = bounds.right - 460;
        draw_button(
            hdc,
            model,
            "Refresh",
            UiRect {
                left: x,
                top: 18,
                right: x + 96,
                bottom: 52,
            },
            ButtonAction::Refresh,
        );
        x += 108;
        draw_button(
            hdc,
            model,
            "Return Host",
            UiRect {
                left: x,
                top: 18,
                right: x + 128,
                bottom: 52,
            },
            ButtonAction::ReturnHost,
        );
        x += 140;
        draw_button(
            hdc,
            model,
            "Close",
            UiRect {
                left: x,
                top: 18,
                right: x + 88,
                bottom: 52,
            },
            ButtonAction::CloseSession,
        );
        x += 100;
        draw_button(
            hdc,
            model,
            "Reset",
            UiRect {
                left: x,
                top: 18,
                right: x + 88,
                bottom: 52,
            },
            ButtonAction::ResetSession,
        );
    }

    unsafe fn draw_workspace(
        hdc: windows_sys::Win32::Graphics::Gdi::HDC,
        model: &mut ShellModel,
        bounds: RECT,
    ) {
        let top = 96;
        let bottom = bounds.bottom - 96;
        let left_panel = RECT {
            left: 32,
            top,
            right: 440,
            bottom,
        };
        let status_panel = RECT {
            left: bounds.right - 440,
            top,
            right: bounds.right - 32,
            bottom,
        };

        fill(hdc, left_panel, PANEL);
        frame(hdc, left_panel, ACCENT);
        text(
            hdc,
            "Launcher",
            56,
            top + 24,
            320,
            28,
            TEXT,
            DT_LEFT | DT_SINGLELINE,
        );
        text(
            hdc,
            "Policy allow-list",
            56,
            top + 56,
            320,
            22,
            MUTED,
            DT_LEFT | DT_SINGLELINE,
        );

        let mut y = top + 96;
        if model.apps.is_empty() {
            text(
                hdc,
                "No launchable applications are available. Configure policy apps in the service policy.",
                56,
                y,
                328,
                64,
                MUTED,
                DT_LEFT | DT_WORDBREAK,
            );
        } else {
            let apps = model.apps.clone();
            for app in apps.iter().take(8) {
                let card = UiRect {
                    left: 56,
                    top: y,
                    right: 408,
                    bottom: y + 64,
                };
                draw_button(
                    hdc,
                    model,
                    &format!("Launch {}", app.name),
                    card,
                    ButtonAction::LaunchApp(app.id.clone()),
                );
                text(
                    hdc,
                    &app.executable.display().to_string(),
                    72,
                    y + 36,
                    300,
                    20,
                    MUTED,
                    DT_LEFT | DT_SINGLELINE,
                );
                y += 76;
            }
        }

        let center = RECT {
            left: 472,
            top,
            right: bounds.right - 472,
            bottom,
        };
        fill(hdc, center, DARK_BACKGROUND);
        text(
            hdc,
            "Sandbox Workspace",
            center.left + 32,
            center.top + 28,
            420,
            34,
            TEXT,
            DT_LEFT | DT_SINGLELINE,
        );
        text(
            hdc,
            "Native Windows applications launched by Sandbox+ appear above this controlled shell. Use the taskbar below to activate, minimize, or request a graceful close.",
            center.left + 32,
            center.top + 72,
            center.right - center.left - 64,
            58,
            MUTED,
            DT_LEFT | DT_WORDBREAK,
        );
        text(
            hdc,
            &model.message,
            center.left + 32,
            center.bottom - 58,
            center.right - center.left - 64,
            32,
            TEXT,
            DT_LEFT | DT_WORDBREAK,
        );

        fill(hdc, status_panel, PANEL);
        frame(hdc, status_panel, ACCENT);
        text(
            hdc,
            "Status",
            status_panel.left + 24,
            top + 24,
            320,
            28,
            TEXT,
            DT_LEFT | DT_SINGLELINE,
        );
        if let Some(policy) = &model.policy {
            let lines = [
                format!("Policy: {}", policy.version),
                format!("Apps: {}", policy.app_count),
                format!("Network: {:?}", policy.network_mode),
                format!("Clipboard: {:?}", policy.clipboard_mode),
                format!("Import: {}", policy.import_allowed),
                format!("Export: {}", policy.export_allowed),
            ];
            let mut y = top + 72;
            for line in lines {
                text(
                    hdc,
                    &line,
                    status_panel.left + 24,
                    y,
                    320,
                    24,
                    MUTED,
                    DT_LEFT | DT_SINGLELINE,
                );
                y += 32;
            }
        } else {
            text(
                hdc,
                "Policy state unavailable until a service session is active.",
                status_panel.left + 24,
                top + 72,
                320,
                64,
                MUTED,
                DT_LEFT | DT_WORDBREAK,
            );
        }

        text(
            hdc,
            "File Exchange",
            status_panel.left + 24,
            top + 300,
            320,
            28,
            TEXT,
            DT_LEFT | DT_SINGLELINE,
        );
        text(
            hdc,
            "Import/export must flow through a broker. Direct host path access is intentionally not exposed here.",
            status_panel.left + 24,
            top + 336,
            320,
            88,
            MUTED,
            DT_LEFT | DT_WORDBREAK,
        );
    }

    unsafe fn draw_taskbar(
        hdc: windows_sys::Win32::Graphics::Gdi::HDC,
        model: &mut ShellModel,
        bounds: RECT,
    ) {
        let bar = RECT {
            left: 0,
            top: bounds.bottom - 72,
            right: bounds.right,
            bottom: bounds.bottom,
        };
        fill(hdc, bar, PANEL);

        text(
            hdc,
            "Running",
            24,
            bounds.bottom - 48,
            100,
            24,
            TEXT,
            DT_LEFT | DT_SINGLELINE,
        );
        let mut x = 132;
        let processes = model.processes.clone();
        for process in processes.iter().take(5) {
            let label = format!(
                "{} · pid {}",
                process.app_id.as_deref().unwrap_or("process"),
                process.process_id
            );
            draw_button(
                hdc,
                model,
                &label,
                UiRect {
                    left: x,
                    top: bounds.bottom - 56,
                    right: x + 220,
                    bottom: bounds.bottom - 16,
                },
                ButtonAction::ActivateProcess(process.process_id),
            );
            draw_button(
                hdc,
                model,
                "_",
                UiRect {
                    left: x + 226,
                    top: bounds.bottom - 56,
                    right: x + 262,
                    bottom: bounds.bottom - 16,
                },
                ButtonAction::MinimizeProcess(process.process_id),
            );
            draw_button(
                hdc,
                model,
                "×",
                UiRect {
                    left: x + 268,
                    top: bounds.bottom - 56,
                    right: x + 304,
                    bottom: bounds.bottom - 16,
                },
                ButtonAction::CloseProcess(process.process_id),
            );
            x += 320;
        }

        let now = chrono_like_clock();
        text(
            hdc,
            &now,
            bounds.right - 160,
            bounds.bottom - 48,
            132,
            24,
            MUTED,
            DT_CENTER | DT_SINGLELINE,
        );
    }

    unsafe fn draw_button(
        hdc: windows_sys::Win32::Graphics::Gdi::HDC,
        model: &mut ShellModel,
        label: &str,
        rect: UiRect,
        action: ButtonAction,
    ) {
        let native = RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        fill(hdc, native, CARD);
        frame(hdc, native, ACCENT);
        text(
            hdc,
            label,
            rect.left + 8,
            rect.top + 8,
            rect.right - rect.left - 16,
            rect.bottom - rect.top - 16,
            TEXT,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        model.buttons.push(ShellButton { rect, action });
    }

    unsafe fn fill(hdc: windows_sys::Win32::Graphics::Gdi::HDC, rect: RECT, color: u32) {
        let brush = CreateSolidBrush(color);
        FillRect(hdc, &rect, brush);
        DeleteObject(brush);
    }

    unsafe fn frame(hdc: windows_sys::Win32::Graphics::Gdi::HDC, rect: RECT, color: u32) {
        let brush = CreateSolidBrush(color);
        FrameRect(hdc, &rect, brush);
        DeleteObject(brush);
    }

    unsafe fn text(
        hdc: windows_sys::Win32::Graphics::Gdi::HDC,
        value: &str,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        color: u32,
        format: u32,
    ) {
        let mut rect = RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        };
        let value = wide_null(value);
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, color);
        DrawTextW(hdc, value.as_ptr(), -1, &mut rect, format);
    }

    fn chrono_like_clock() -> String {
        let now = std::time::SystemTime::now();
        let elapsed = now
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let seconds = elapsed % 86_400;
        let hour = seconds / 3600;
        let minute = (seconds % 3600) / 60;
        format!("{hour:02}:{minute:02} UTC")
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error(api: &str) -> sandbox_common::SandboxError {
        let code = unsafe { GetLastError() };
        sandbox_common::SandboxError::System(format!("{api} failed with Win32 error {code}"))
    }

    pub(super) fn activate_process_window(process_id: u32) {
        if let Some(hwnd) = find_window_for_process(process_id) {
            unsafe {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
        }
    }

    pub(super) fn minimize_process_window(process_id: u32) {
        if let Some(hwnd) = find_window_for_process(process_id) {
            unsafe {
                ShowWindow(hwnd, SW_MINIMIZE);
            }
        }
    }

    pub(super) fn close_process_window(process_id: u32) {
        if let Some(hwnd) = find_window_for_process(process_id) {
            unsafe {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
        }
    }

    fn find_window_for_process(process_id: u32) -> Option<HWND> {
        #[repr(C)]
        struct Search {
            process_id: u32,
            current_process_id: u32,
            hwnd: HWND,
        }

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> i32 {
            let search = &mut *(lparam as *mut Search);
            let mut window_pid = 0;
            GetWindowThreadProcessId(hwnd, &mut window_pid);
            if window_pid == search.process_id
                && window_pid != search.current_process_id
                && IsWindowVisible(hwnd) != 0
            {
                search.hwnd = hwnd;
                return 0;
            }
            1
        }

        let mut search = Search {
            process_id,
            current_process_id: unsafe { GetCurrentProcessId() },
            hwnd: null_mut(),
        };
        unsafe {
            EnumWindows(Some(enum_proc), &mut search as *mut Search as LPARAM);
        }
        if search.hwnd.is_null() {
            None
        } else {
            Some(search.hwnd)
        }
    }
}

#[cfg(not(windows))]
fn activate_process_window(_process_id: u32) {}

#[cfg(not(windows))]
fn minimize_process_window(_process_id: u32) {}

#[cfg(not(windows))]
fn close_process_window(_process_id: u32) {}

#[cfg(windows)]
fn activate_process_window(process_id: u32) {
    windows_shell::activate_process_window(process_id);
}

#[cfg(windows)]
fn minimize_process_window(process_id: u32) {
    windows_shell::minimize_process_window(process_id);
}

#[cfg(windows)]
fn close_process_window(process_id: u32) {
    windows_shell::close_process_window(process_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_shell_context() {
        let options = ShellOptions::parse([
            "--title".to_string(),
            "Custom".to_string(),
            "--sandbox-id".to_string(),
            "sandbox-1".to_string(),
            "--desktop-name".to_string(),
            "Sandbox-sandbox-1".to_string(),
        ])
        .expect("options should parse");

        assert_eq!(options.title, "Custom");
        assert_eq!(options.sandbox_id, Some(SandboxId("sandbox-1".to_string())));
        assert_eq!(options.desktop_name.as_deref(), Some("Sandbox-sandbox-1"));
    }

    #[test]
    fn rejects_fast_refresh() {
        let error = ShellOptions::parse(["--refresh-ms".to_string(), "100".to_string()])
            .expect_err("fast refresh should be rejected");

        assert!(error.contains("at least 500"));
    }

    #[test]
    fn hit_testing_is_inclusive() {
        let rect = UiRect {
            left: 10,
            top: 20,
            right: 30,
            bottom: 40,
        };

        assert!(rect.contains(10, 20));
        assert!(rect.contains(30, 40));
        assert!(!rect.contains(31, 40));
    }
}
