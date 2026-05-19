use std::env;
use std::process::ExitCode;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Options {
    mode: Mode,
    program: String,
    args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    RestrictedToken,
    AppContainerProbe,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::RestrictedToken,
            program: "cmd.exe".to_string(),
            args: vec!["/c".to_string(), "whoami /all && pause".to_string()],
        }
    }
}

fn main() -> ExitCode {
    let options = match parse_args(env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("POC-0102 failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                options.mode = match next_value(&mut args, "--mode")?.as_str() {
                    "restricted-token" => Mode::RestrictedToken,
                    "appcontainer-probe" => Mode::AppContainerProbe,
                    other => return Err(format!("unsupported mode: {other}")),
                };
            }
            "--program" => options.program = next_value(&mut args, "--program")?,
            "--arg" => options.args.push(next_value(&mut args, "--arg")?),
            "--clear-default-args" => options.args.clear(),
            "--help" | "-h" => return Err("POC-0102 restricted token / AppContainer probe".into()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if options.program.trim().is_empty() {
        return Err("--program must not be empty".into());
    }

    Ok(options)
}

fn next_value(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    args.next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{name} requires a value"))
}

fn print_usage() {
    eprintln!(
        "Usage: sandbox-token-poc [--mode restricted-token|appcontainer-probe] [--program cmd.exe] [--clear-default-args] [--arg VALUE]..."
    );
}

#[cfg(windows)]
fn run(options: Options) -> Result<(), String> {
    match options.mode {
        Mode::RestrictedToken => windows_poc::run_restricted_token(options),
        Mode::AppContainerProbe => windows_poc::probe_appcontainer(),
    }
}

#[cfg(not(windows))]
fn run(_options: Options) -> Result<(), String> {
    Err("POC-0102 can only run on Windows".into())
}

#[cfg(windows)]
mod windows_poc {
    use super::Options;
    use std::mem::size_of;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::Security::{
        CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, TOKEN_ALL_ACCESS,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessAsUserW, GetCurrentProcess, OpenProcessToken, WaitForSingleObject, INFINITE,
        PROCESS_INFORMATION, STARTUPINFOW,
    };

    pub fn run_restricted_token(options: Options) -> Result<(), String> {
        println!("POC-0102 restricted token launch");
        println!("program: {}", options.program);
        println!("args: {:?}", options.args);

        let current_token = ProcessToken::open_current()?;
        let restricted_token = current_token.create_restricted()?;
        let command_line = build_command_line(&options.program, &options.args);
        let child = ChildProcess::create_as_user(restricted_token.raw, &command_line)?;

        println!("started restricted process pid={}", child.process_id);
        child.wait()?;
        println!("restricted process exited");
        Ok(())
    }

    pub fn probe_appcontainer() -> Result<(), String> {
        println!("POC-0102 AppContainer probe");
        println!("This POC stage records capability only; full AppContainer process creation is deferred until MVP token module design.");
        println!("Expected: Windows 8+ supports AppContainer APIs, Windows 7 does not.");
        Ok(())
    }

    struct ProcessToken {
        raw: HANDLE,
    }

    impl ProcessToken {
        fn open_current() -> Result<Self, String> {
            let mut token = null_mut();
            let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut token) };
            if ok == 0 {
                return Err(last_error("OpenProcessToken"));
            }
            Ok(Self { raw: token })
        }

        fn create_restricted(&self) -> Result<Self, String> {
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

    struct ChildProcess {
        process: HANDLE,
        thread: HANDLE,
        process_id: u32,
    }

    impl ChildProcess {
        fn create_as_user(token: HANDLE, command_line: &str) -> Result<Self, String> {
            let mut command_line = wide_null(command_line);
            let mut startup_info: STARTUPINFOW = unsafe { std::mem::zeroed() };
            startup_info.cb = size_of::<STARTUPINFOW>() as u32;
            let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

            let ok = unsafe {
                CreateProcessAsUserW(
                    token,
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
                return Err(last_error("CreateProcessAsUserW"));
            }

            Ok(Self {
                process: process_info.hProcess,
                thread: process_info.hThread,
                process_id: process_info.dwProcessId,
            })
        }

        fn wait(&self) -> Result<(), String> {
            unsafe {
                WaitForSingleObject(self.process, INFINITE);
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

    fn build_command_line(program: &str, args: &[String]) -> String {
        let mut parts = vec![quote_arg(program)];
        parts.extend(args.iter().map(|arg| quote_arg(arg)));
        parts.join(" ")
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

    fn last_error(api: &str) -> String {
        let code = unsafe { GetLastError() };
        format!("{api} failed with Win32 error {code}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_is_restricted_token() {
        let result = parse_args(Vec::<String>::new());
        assert!(result.is_ok(), "{result:?}");
        let Ok(options) = result else { return };
        assert_eq!(options.mode, Mode::RestrictedToken);
        assert_eq!(options.program, "cmd.exe");
    }

    #[test]
    fn parses_appcontainer_probe() {
        let result = parse_args(["--mode".into(), "appcontainer-probe".into()]);
        assert!(result.is_ok(), "{result:?}");
        let Ok(options) = result else { return };
        assert_eq!(options.mode, Mode::AppContainerProbe);
    }
}
