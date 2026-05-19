use std::env;
use std::process::ExitCode;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Cli {
    command: Command,
    desktop_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Demo(DemoOptions),
    Session(SessionOptions),
    Create,
    Switch,
    SwitchBack,
    Launch(LaunchOptions),
    Cleanup,
    ShellChild(ShellOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DemoOptions {
    program: String,
    hold_seconds: u64,
    terminate_child: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchOptions {
    program: String,
    switch_after_launch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionOptions {
    program: Option<String>,
    switch_on_start: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellOptions {
    title: String,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            command: Command::Demo(DemoOptions::default()),
            desktop_name: "Sandbox".to_string(),
        }
    }
}

impl Default for DemoOptions {
    fn default() -> Self {
        Self {
            program: "notepad.exe".to_string(),
            hold_seconds: 10,
            terminate_child: true,
        }
    }
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            program: "notepad.exe".to_string(),
            switch_after_launch: false,
        }
    }
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            program: None,
            switch_on_start: true,
        }
    }
}

impl Default for ShellOptions {
    fn default() -> Self {
        Self {
            title: "Sandbox+ POC".to_string(),
        }
    }
}

fn main() -> ExitCode {
    let cli = match parse_args(env::args().skip(1)) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("POC-0101 failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut args = args.into_iter().peekable();
    let command_name = if args.peek().is_some_and(|value| is_command(value)) {
        args.next()
    } else {
        None
    };

    cli.command = match command_name.as_deref() {
        None | Some("demo") => Command::Demo(DemoOptions::default()),
        Some("session") => Command::Session(SessionOptions::default()),
        Some("create") => Command::Create,
        Some("switch") => Command::Switch,
        Some("switch-back") => Command::SwitchBack,
        Some("launch") => Command::Launch(LaunchOptions::default()),
        Some("cleanup") => Command::Cleanup,
        Some("__shell-child") => Command::ShellChild(ShellOptions::default()),
        Some(other) => return Err(format!("unknown command: {other}")),
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--desktop" => cli.desktop_name = next_value(&mut args, "--desktop")?,
            "--program" => set_program(&mut cli.command, next_value(&mut args, "--program")?)?,
            "--hold-seconds" => {
                let hold_seconds = next_value(&mut args, "--hold-seconds")?
                    .parse()
                    .map_err(|_| "--hold-seconds must be a positive integer".to_string())?;
                if hold_seconds == 0 {
                    return Err("--hold-seconds must be greater than zero".to_string());
                }
                match &mut cli.command {
                    Command::Demo(options) => options.hold_seconds = hold_seconds,
                    _ => return Err("--hold-seconds is only valid for demo".to_string()),
                }
            }
            "--leave-child-running" => match &mut cli.command {
                Command::Demo(options) => options.terminate_child = false,
                _ => return Err("--leave-child-running is only valid for demo".to_string()),
            },
            "--switch-after-launch" => match &mut cli.command {
                Command::Launch(options) => options.switch_after_launch = true,
                _ => return Err("--switch-after-launch is only valid for launch".to_string()),
            },
            "--no-switch" => match &mut cli.command {
                Command::Session(options) => options.switch_on_start = false,
                _ => return Err("--no-switch is only valid for session".to_string()),
            },
            "--title" => match &mut cli.command {
                Command::ShellChild(options) => options.title = next_value(&mut args, "--title")?,
                _ => return Err("--title is only valid for __shell-child".to_string()),
            },
            "--help" | "-h" => return Err("POC-0101 Desktop command runner".to_string()),
            unknown => return Err(format!("unknown argument: {unknown}")),
        }
    }

    if cli.desktop_name.trim().is_empty() {
        return Err("--desktop must not be empty".to_string());
    }

    Ok(cli)
}

fn is_command(value: &str) -> bool {
    matches!(
        value,
        "demo"
            | "session"
            | "create"
            | "switch"
            | "switch-back"
            | "launch"
            | "cleanup"
            | "__shell-child"
    )
}

fn set_program(command: &mut Command, program: String) -> Result<(), String> {
    if program.trim().is_empty() {
        return Err("--program must not be empty".to_string());
    }

    match command {
        Command::Demo(options) => options.program = program,
        Command::Launch(options) => options.program = program,
        Command::Session(options) => options.program = Some(program),
        _ => return Err("--program is only valid for demo, session, or launch".to_string()),
    }

    Ok(())
}

fn next_value(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} requires a value"))
}

fn print_usage() {
    eprintln!(
        "Usage:
  sandbox-desktop-poc demo [--desktop Sandbox] [--program notepad.exe] [--hold-seconds 10] [--leave-child-running]
  sandbox-desktop-poc session [--desktop Sandbox] [--program notepad.exe] [--no-switch]
  sandbox-desktop-poc create [--desktop Sandbox]
  sandbox-desktop-poc switch [--desktop Sandbox]
  sandbox-desktop-poc switch-back
  sandbox-desktop-poc launch [--desktop Sandbox] [--program notepad.exe] [--switch-after-launch]
  sandbox-desktop-poc cleanup [--desktop Sandbox]

Session mode starts a sandbox background shell, adds a tray icon, and registers Ctrl+Alt+S while the process is running."
    );
}

#[cfg(windows)]
fn run(cli: Cli) -> Result<(), String> {
    windows_poc::run(cli)
}

#[cfg(not(windows))]
fn run(_cli: Cli) -> Result<(), String> {
    Err("POC-0101 can only run on Windows because it calls Win32 Desktop APIs".to_string())
}

#[cfg(windows)]
mod windows_poc {
    use super::{Cli, Command, DemoOptions, LaunchOptions, SessionOptions};
    use std::mem::size_of;
    use std::ptr::{null, null_mut};
    use std::thread;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM,
    };
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, SetBkMode,
        SetTextColor, DT_LEFT, DT_TOP, DT_WORDBREAK, PAINTSTRUCT, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, CreateDesktopW, OpenDesktopW, SwitchDesktop, DESKTOP_CREATEWINDOW,
        DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP, DESKTOP_WRITEOBJECTS, HDESK,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, TerminateProcess, PROCESS_INFORMATION, STARTUPINFOW,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL,
    };
    use windows_sys::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics,
        LoadIconW, PostQuitMessage, RegisterClassW, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
        CW_USEDEFAULT, HMENU, IDI_APPLICATION, MSG, SM_CXSCREEN, SM_CYSCREEN, WM_DESTROY,
        WM_HOTKEY, WM_PAINT, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
    };

    const DESKTOP_ACCESS: u32 =
        DESKTOP_CREATEWINDOW | DESKTOP_READOBJECTS | DESKTOP_SWITCHDESKTOP | DESKTOP_WRITEOBJECTS;
    const HOTKEY_ID: i32 = 1001;
    const TRAY_ID: u32 = 2001;
    const WM_TRAYICON: u32 = 0x8001;

    pub fn run(cli: Cli) -> Result<(), String> {
        match cli.command {
            Command::Demo(options) => run_demo(&cli.desktop_name, options),
            Command::Session(options) => run_session(&cli.desktop_name, options),
            Command::Create => {
                let desktop = DesktopHandle::open_or_create(&cli.desktop_name)?;
                println!(
                    "desktop '{}' is ready ({})",
                    cli.desktop_name, desktop.source
                );
                Ok(())
            }
            Command::Switch => {
                let desktop = DesktopHandle::open_or_create(&cli.desktop_name)?;
                let started = Instant::now();
                desktop.switch_to()?;
                println!(
                    "switched to '{}' in {} ms",
                    cli.desktop_name,
                    started.elapsed().as_millis()
                );
                Ok(())
            }
            Command::SwitchBack => {
                let desktop = DesktopHandle::open("Default", DESKTOP_SWITCHDESKTOP)?;
                let started = Instant::now();
                desktop.switch_to()?;
                println!(
                    "switched back to Default in {} ms",
                    started.elapsed().as_millis()
                );
                Ok(())
            }
            Command::Launch(options) => run_launch(&cli.desktop_name, options),
            Command::Cleanup => {
                println!(
                    "cleanup note: Windows desktops are destroyed when all handles close and processes on them exit."
                );
                println!(
                    "close or terminate programs launched on '{}', then rerun create/switch as needed.",
                    cli.desktop_name
                );
                Ok(())
            }
            Command::ShellChild(options) => ShellWindow::run(&options.title),
        }
    }

    fn run_session(desktop_name: &str, options: SessionOptions) -> Result<(), String> {
        println!("POC-0101C session");
        println!("desktop: {desktop_name}");
        println!("hotkey: Ctrl+Alt+S toggles Default/Sandbox while this process is running");

        let sandbox_desktop = DesktopHandle::open_or_create(desktop_name)?;
        let default_desktop = DesktopHandle::open("Default", DESKTOP_SWITCHDESKTOP)?;
        let shell_command = shell_child_command_line(desktop_name)?;
        let shell = ChildProcess::spawn_command_on_desktop(&shell_command, desktop_name)?;
        shell.detach();

        if let Some(program) = options.program {
            let app = ChildProcess::spawn_command_on_desktop(&program, desktop_name)?;
            app.detach();
            println!("started initial app on sandbox desktop: {program}");
        }

        let controller = ControllerWindow::create()?;
        let _tray = TrayIcon::add(controller.hwnd)?;
        let _hotkey = Hotkey::register()?;

        if options.switch_on_start {
            sandbox_desktop.switch_to()?;
        }

        controller.message_loop(&sandbox_desktop, &default_desktop)
    }

    fn run_demo(desktop_name: &str, options: DemoOptions) -> Result<(), String> {
        println!("POC-0101 demo");
        println!("desktop: {desktop_name}");
        println!("program: {}", options.program);
        println!("hold_seconds: {}", options.hold_seconds);

        let sandbox_desktop = DesktopHandle::open_or_create(desktop_name)?;
        println!("desktop source: {}", sandbox_desktop.source);
        let default_desktop = DesktopHandle::open("Default", DESKTOP_SWITCHDESKTOP)?;
        let child = ChildProcess::spawn_command_on_desktop(&options.program, desktop_name)?;
        let mut return_guard = DesktopReturnGuard::new(&default_desktop);

        let started = Instant::now();
        sandbox_desktop.switch_to()?;
        return_guard.arm();
        println!(
            "switched to sandbox desktop in {} ms",
            started.elapsed().as_millis()
        );

        thread::sleep(Duration::from_secs(options.hold_seconds));

        let started = Instant::now();
        default_desktop.switch_to()?;
        return_guard.disarm();
        println!(
            "switched back to default desktop in {} ms",
            started.elapsed().as_millis()
        );

        if options.terminate_child {
            child.terminate()?;
            println!("terminated POC child process");
        } else {
            println!("left POC child process running by request");
        }

        Ok(())
    }

    fn run_launch(desktop_name: &str, options: LaunchOptions) -> Result<(), String> {
        let desktop = DesktopHandle::open_or_create(desktop_name)?;
        println!("desktop '{}' is ready ({})", desktop_name, desktop.source);
        let child = ChildProcess::spawn_command_on_desktop(&options.program, desktop_name)?;
        child.detach();

        if options.switch_after_launch {
            let started = Instant::now();
            desktop.switch_to()?;
            println!(
                "launched and switched to '{}' in {} ms",
                desktop_name,
                started.elapsed().as_millis()
            );
        } else {
            println!(
                "launched on '{}'; run `switch --desktop {}` to view it",
                desktop_name, desktop_name
            );
        }

        Ok(())
    }

    fn shell_child_command_line(desktop_name: &str) -> Result<String, String> {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let exe = exe
            .to_str()
            .ok_or_else(|| "current exe path is not unicode".to_string())?;
        Ok(format!(
            "{} __shell-child --desktop {} --title {}",
            quote_arg(exe),
            quote_arg(desktop_name),
            quote_arg(&format!("Sandbox+ POC - {desktop_name}"))
        ))
    }

    struct DesktopHandle {
        raw: HDESK,
        source: DesktopSource,
    }

    #[derive(Debug, Clone, Copy)]
    enum DesktopSource {
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

    impl DesktopHandle {
        fn open_or_create(name: &str) -> Result<Self, String> {
            match Self::open(name, DESKTOP_ACCESS) {
                Ok(mut desktop) => {
                    desktop.source = DesktopSource::Opened;
                    Ok(desktop)
                }
                Err(open_error) => match Self::create(name) {
                    Ok(desktop) => Ok(desktop),
                    Err(create_error) => Err(format!(
                        "open failed ({open_error}); create failed ({create_error})"
                    )),
                },
            }
        }

        fn create(name: &str) -> Result<Self, String> {
            let name = wide_null(name);
            let raw = unsafe {
                CreateDesktopW(name.as_ptr(), null_mut(), null(), 0, DESKTOP_ACCESS, null())
            };
            if raw.is_null() {
                return Err(last_error("CreateDesktopW"));
            }
            Ok(Self {
                raw,
                source: DesktopSource::Created,
            })
        }

        fn open(name: &str, access: u32) -> Result<Self, String> {
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

        fn switch_to(&self) -> Result<(), String> {
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

    struct DesktopReturnGuard<'a> {
        desktop: &'a DesktopHandle,
        armed: bool,
    }

    impl<'a> DesktopReturnGuard<'a> {
        fn new(desktop: &'a DesktopHandle) -> Self {
            Self {
                desktop,
                armed: false,
            }
        }

        fn arm(&mut self) {
            self.armed = true;
        }

        fn disarm(&mut self) {
            self.armed = false;
        }
    }

    impl Drop for DesktopReturnGuard<'_> {
        fn drop(&mut self) {
            if self.armed {
                let _ = self.desktop.switch_to();
            }
        }
    }

    struct ChildProcess {
        process: HANDLE,
        thread: HANDLE,
        detached: bool,
    }

    impl ChildProcess {
        fn spawn_command_on_desktop(
            command_line: &str,
            desktop_name: &str,
        ) -> Result<Self, String> {
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

            println!(
                "started process {} on WinSta0\\{desktop_name}",
                process_info.dwProcessId
            );

            Ok(Self {
                process: process_info.hProcess,
                thread: process_info.hThread,
                detached: false,
            })
        }

        fn terminate(&self) -> Result<(), String> {
            let ok = unsafe { TerminateProcess(self.process, 0) };
            if ok == 0 {
                return Err(last_error("TerminateProcess"));
            }
            Ok(())
        }

        fn detach(mut self) {
            self.detached = true;
        }
    }

    impl Drop for ChildProcess {
        fn drop(&mut self) {
            unsafe {
                if !self.thread.is_null() {
                    CloseHandle(self.thread);
                }
                if !self.process.is_null() {
                    if !self.detached {
                        let _ = TerminateProcess(self.process, 0);
                    }
                    CloseHandle(self.process);
                }
            }
        }
    }

    struct ControllerWindow {
        hwnd: HWND,
    }

    impl ControllerWindow {
        fn create() -> Result<Self, String> {
            let class = wide_null("SandboxPlusPocController");
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
                    wide_null("Sandbox+ POC Controller").as_ptr(),
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

        fn message_loop(
            &self,
            sandbox_desktop: &DesktopHandle,
            default_desktop: &DesktopHandle,
        ) -> Result<(), String> {
            let mut in_sandbox = true;
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
                    let started = Instant::now();
                    if in_sandbox {
                        default_desktop.switch_to()?;
                        println!(
                            "hotkey switched to Default in {} ms",
                            started.elapsed().as_millis()
                        );
                    } else {
                        sandbox_desktop.switch_to()?;
                        println!(
                            "hotkey switched to Sandbox in {} ms",
                            started.elapsed().as_millis()
                        );
                    }
                    in_sandbox = !in_sandbox;
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
        DefWindowProcW(hwnd, message, wparam, lparam)
    }

    struct Hotkey;

    impl Hotkey {
        fn register() -> Result<Self, String> {
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
        fn add(hwnd: HWND) -> Result<Self, String> {
            let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
            data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = hwnd;
            data.uID = TRAY_ID;
            data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            data.uCallbackMessage = WM_TRAYICON;
            data.hIcon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
            copy_wide_fixed("Sandbox+ POC (Ctrl+Alt+S)", &mut data.szTip);

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

    struct ShellWindow;

    impl ShellWindow {
        fn run(title: &str) -> Result<(), String> {
            let class = wide_null("SandboxPlusPocShell");
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

            let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
            let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
            let title = wide_null(title);
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
                    null(),
                )
            };
            if hwnd.is_null() {
                return Err(last_error("CreateWindowExW(shell)"));
            }

            let mut message: MSG = unsafe { std::mem::zeroed() };
            loop {
                let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
                if result == -1 {
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
            Ok(())
        }
    }

    unsafe extern "system" fn shell_wnd_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_PAINT => {
                let mut paint: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut paint);
                let brush = CreateSolidBrush(0x00202020);
                let rect = RECT {
                    left: 0,
                    top: 0,
                    right: GetSystemMetrics(SM_CXSCREEN),
                    bottom: GetSystemMetrics(SM_CYSCREEN),
                };
                FillRect(hdc, &rect, brush);
                SetBkMode(hdc, TRANSPARENT as i32);
                SetTextColor(hdc, 0x00E6E6E6);
                let mut text_rect = RECT {
                    left: 64,
                    top: 56,
                    right: rect.right - 64,
                    bottom: rect.bottom - 64,
                };
                let text = wide_null(
                    "Sandbox+ POC\n\nThis is the sandbox desktop shell.\n\nCtrl+Alt+S: switch between Sandbox and Default\n\nApplications launched into this Desktop appear above this background.",
                );
                DrawTextW(
                    hdc,
                    text.as_ptr(),
                    -1,
                    &mut text_rect,
                    DT_LEFT | DT_TOP | DT_WORDBREAK,
                );
                DeleteObject(brush);
                EndPaint(hwnd, &paint);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
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

    fn quote_arg(value: &str) -> String {
        if value.contains(' ') || value.contains('\t') {
            format!("\"{}\"", value.replace('"', "\\\""))
        } else {
            value.to_string()
        }
    }

    fn last_error(api: &str) -> String {
        let code = unsafe { GetLastError() };
        format!("{api} failed with Win32 error {code}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_command_defaults_to_demo() {
        let cli = parse_args(Vec::<String>::new()).expect("defaults should parse");

        assert_eq!(cli.desktop_name, "Sandbox");
        assert!(matches!(cli.command, Command::Demo(_)));
    }

    #[test]
    fn parses_legacy_demo_options() {
        let cli = parse_args([
            "--desktop".to_string(),
            "SandboxPoc".to_string(),
            "--program".to_string(),
            "mspaint.exe".to_string(),
            "--hold-seconds".to_string(),
            "3".to_string(),
            "--leave-child-running".to_string(),
        ])
        .expect("custom options should parse");

        assert_eq!(cli.desktop_name, "SandboxPoc");
        let Command::Demo(options) = cli.command else {
            panic!("expected demo");
        };
        assert_eq!(options.program, "mspaint.exe");
        assert_eq!(options.hold_seconds, 3);
        assert!(!options.terminate_child);
    }

    #[test]
    fn parses_session_command() {
        let cli = parse_args([
            "session".to_string(),
            "--desktop".to_string(),
            "Demo".to_string(),
            "--program".to_string(),
            "notepad.exe".to_string(),
            "--no-switch".to_string(),
        ])
        .expect("session should parse");

        assert_eq!(cli.desktop_name, "Demo");
        let Command::Session(options) = cli.command else {
            panic!("expected session");
        };
        assert_eq!(options.program.as_deref(), Some("notepad.exe"));
        assert!(!options.switch_on_start);
    }

    #[test]
    fn parses_launch_command() {
        let cli = parse_args([
            "launch".to_string(),
            "--desktop".to_string(),
            "Demo".to_string(),
            "--program".to_string(),
            "notepad.exe".to_string(),
            "--switch-after-launch".to_string(),
        ])
        .expect("launch should parse");

        assert_eq!(cli.desktop_name, "Demo");
        let Command::Launch(options) = cli.command else {
            panic!("expected launch");
        };
        assert_eq!(options.program, "notepad.exe");
        assert!(options.switch_after_launch);
    }

    #[test]
    fn rejects_empty_program() {
        let err = parse_args(["--program".to_string(), " ".to_string()])
            .expect_err("empty program should be rejected");

        assert!(err.contains("--program"));
    }
}
