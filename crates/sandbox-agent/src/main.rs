use sandbox_common::{CloseSessionMode, CloseSessionRequest, SandboxError, SandboxId};
use sandbox_ipc::{send_request, IpcRequest, IpcResponse, SERVICE_PIPE_NAME};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> sandbox_common::Result<()> {
    let options = AgentOptions::parse(std::env::args().skip(1))?;
    platform::run(options)
}

#[derive(Debug, Clone)]
struct AgentOptions {
    sandbox_id: SandboxId,
    desktop_name: String,
}

impl AgentOptions {
    fn parse(mut args: impl Iterator<Item = String>) -> sandbox_common::Result<Self> {
        let mut sandbox_id = None;
        let mut desktop_name = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--sandbox-id" => sandbox_id = args.next().map(SandboxId),
                "--desktop-name" => desktop_name = args.next(),
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

fn unexpected_response(response: IpcResponse) -> SandboxError {
    match response {
        IpcResponse::Error(error) => {
            SandboxError::System(format!("{}: {}", error.code, error.message))
        }
        other => SandboxError::System(format!("unexpected service response: {other:?}")),
    }
}

#[cfg(windows)]
mod platform {
    use super::{close_session, return_to_host, AgentOptions};
    use sandbox_common::{CloseSessionMode, Result, SandboxError};
    use std::ptr::{null, null_mut};
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{
        GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect,
        SetBkMode, SetTextColor, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HBRUSH, PAINTSTRUCT,
        TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
        DispatchMessageW, GetCursorPos, GetMessageW, PostQuitMessage, RegisterClassW,
        SetForegroundWindow, TrackPopupMenu, TranslateMessage, CW_USEDEFAULT, HMENU, HTCAPTION,
        MF_SEPARATOR, MF_STRING, MSG, TPM_RETURNCMD, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP,
        WM_NCHITTEST, WM_PAINT, WM_RBUTTONUP, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
    };

    const MENU_RETURN: usize = 4101;
    const MENU_CLOSE: usize = 4102;
    const MENU_RESET: usize = 4103;
    const MENU_EXIT: usize = 4104;
    static CONTEXT: OnceLock<AgentOptions> = OnceLock::new();

    pub fn run(options: AgentOptions) -> Result<()> {
        let _ = CONTEXT.set(options.clone());
        let window = AgentWindow::create(&options)?;
        println!(
            "sandbox-agent running for {} on WinSta0\\{}",
            options.sandbox_id.0, options.desktop_name
        );
        window.message_loop()
    }

    struct AgentWindow {
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

            let title = wide_null(&format!("Sandbox+ {}", options.sandbox_id.0));
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    class.as_ptr(),
                    title.as_ptr(),
                    WS_POPUP | WS_VISIBLE,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    128,
                    52,
                    null_mut(),
                    null_mut::<std::ffi::c_void>() as HMENU,
                    instance,
                    null(),
                )
            };
            if hwnd.is_null() {
                return Err(last_error("CreateWindowExW(agent)"));
            }
            Ok(Self { hwnd })
        }

        fn message_loop(&self) -> Result<()> {
            let _hwnd = self.hwnd;
            let mut message: MSG = unsafe { std::mem::zeroed() };
            loop {
                let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
                if result == -1 {
                    return Err(last_error("GetMessageW"));
                }
                if result == 0 {
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

    unsafe extern "system" fn agent_wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_PAINT => {
                paint_button(hwnd);
                0
            }
            WM_LBUTTONUP => {
                with_context(|context| return_to_host(&context.sandbox_id));
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
            WM_NCHITTEST => HTCAPTION as LRESULT,
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    fn handle_menu_command(command: usize) {
        match command {
            MENU_RETURN => with_context(|context| return_to_host(&context.sandbox_id)),
            MENU_CLOSE => with_context(|context| {
                close_session(&context.sandbox_id, CloseSessionMode::KeepProfile)
            }),
            MENU_RESET => with_context(|context| {
                close_session(&context.sandbox_id, CloseSessionMode::ResetProfile)
            }),
            MENU_EXIT => unsafe {
                PostQuitMessage(0);
            },
            _ => {}
        }
    }

    fn with_context(action: impl FnOnce(&AgentOptions) -> Result<()>) {
        if let Some(context) = CONTEXT.get() {
            if let Err(error) = action(context) {
                eprintln!("sandbox-agent action failed: {error}");
            }
        }
    }

    fn show_context_menu(hwnd: HWND) -> Result<usize> {
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return Err(last_error("CreatePopupMenu"));
        }

        unsafe {
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
                MENU_CLOSE,
                wide_null("Close Sandbox").as_ptr(),
            );
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_RESET,
                wide_null("Reset Sandbox").as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(menu, MF_STRING, MENU_EXIT, wide_null("Exit Agent").as_ptr());
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
        let mut paint: PAINTSTRUCT = unsafe { std::mem::zeroed() };
        let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
        let rect = RECT {
            left: 0,
            top: 0,
            right: 128,
            bottom: 52,
        };
        let background = unsafe { CreateSolidBrush(0x00245fd4) };
        let border = unsafe { CreateSolidBrush(0x000f3b87) };
        fill_rect(hdc, &rect, background);
        frame_rect(hdc, &rect, border);
        unsafe {
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, 0x00ffffff);
        }
        let mut text_rect = rect;
        let text = wide_null("Return Host");
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
