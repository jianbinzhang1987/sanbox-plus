use sandbox_common::{
    AppSummary, AttachSessionRequest, CloseSessionRequest, CreateSessionRequest,
    CreateSessionResponse, LaunchAppRequest, LaunchAppResponse, PolicySummary, ProcessInfo, Result,
    SandboxError, SandboxId, SandboxStatus, ServiceError, UpdateSessionStateRequest,
};
use serde::{Deserialize, Serialize};

pub const SERVICE_PIPE_NAME: &str = r"\\.\pipe\SandboxPlus.Service";
const MAX_MESSAGE_BYTES: u32 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IpcRequest {
    CreateSession(CreateSessionRequest),
    AttachSession(AttachSessionRequest),
    UpdateSessionState(UpdateSessionStateRequest),
    CloseSession(CloseSessionRequest),
    ResetSession(SandboxId),
    GetStatus,
    ListApps {
        sandbox_id: SandboxId,
    },
    LaunchApp(LaunchAppRequest),
    LaunchSystemProcess(LaunchAppRequest),
    RecoverWorkspace {
        sandbox_id: SandboxId,
    },
    RecordProcess {
        sandbox_id: SandboxId,
        process: ProcessInfo,
    },
    ListProcesses {
        sandbox_id: SandboxId,
    },
    GetPolicySummary {
        sandbox_id: SandboxId,
    },
    ReturnToHost {
        sandbox_id: SandboxId,
    },
    EnterSession {
        sandbox_id: SandboxId,
    },
    ImportFiles {
        sandbox_id: SandboxId,
    },
    ExportFiles {
        sandbox_id: SandboxId,
    },
    ExportDiagnosticBundle {
        sandbox_id: SandboxId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IpcResponse {
    Ok,
    SessionCreated(CreateSessionResponse),
    Status(SandboxStatus),
    Apps(Vec<AppSummary>),
    Processes(Vec<ProcessInfo>),
    LaunchApp(LaunchAppResponse),
    PolicySummary(PolicySummary),
    Process(ProcessInfo),
    Error(ServiceError),
}

impl IpcResponse {
    pub fn from_error(error: impl Into<ServiceError>) -> Self {
        Self::Error(error.into())
    }
}

pub fn encode_request(request: &IpcRequest) -> Result<Vec<u8>> {
    serde_json::to_vec(request).map_err(SandboxError::from)
}

pub fn decode_request(input: &[u8]) -> Result<IpcRequest> {
    serde_json::from_slice(input).map_err(SandboxError::from)
}

pub fn encode_response(response: &IpcResponse) -> Result<Vec<u8>> {
    serde_json::to_vec(response).map_err(SandboxError::from)
}

pub fn decode_response(input: &[u8]) -> Result<IpcResponse> {
    serde_json::from_slice(input).map_err(SandboxError::from)
}

#[cfg(windows)]
pub fn send_request(pipe_name: &str, request: &IpcRequest) -> Result<IpcResponse> {
    use std::fs::OpenOptions;
    use std::io::{ErrorKind, Read, Write};

    let mut pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name)
        .map_err(|error| {
            SandboxError::System(format!("failed to open named pipe '{pipe_name}': {error}"))
        })?;

    let request = encode_request(request)?;
    pipe.write_all(&request).map_err(|error| {
        SandboxError::System(format!("failed to write named pipe request: {error}"))
    })?;
    pipe.flush().map_err(|error| {
        SandboxError::System(format!("failed to flush named pipe request: {error}"))
    })?;

    let mut response = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => response.extend_from_slice(&chunk[..count]),
            Err(error)
                if !response.is_empty()
                    && (error.kind() == ErrorKind::BrokenPipe
                        || error.raw_os_error() == Some(233)) =>
            {
                break;
            }
            Err(error) => {
                return Err(SandboxError::System(format!(
                    "failed to read named pipe response: {error}"
                )));
            }
        }
    }

    if response.is_empty() {
        return Err(SandboxError::System(
            "named pipe response was empty".to_string(),
        ));
    }

    decode_response(&response)
}

#[cfg(not(windows))]
pub fn send_request(_pipe_name: &str, _request: &IpcRequest) -> Result<IpcResponse> {
    Err(SandboxError::UnsupportedPlatform(
        "named pipe IPC is only implemented on Windows".to_string(),
    ))
}

#[cfg(windows)]
pub fn serve_forever(
    pipe_name: &str,
    mut handler: impl FnMut(IpcRequest) -> IpcResponse,
) -> Result<()> {
    loop {
        serve_once(pipe_name, &mut handler)?;
    }
}

#[cfg(not(windows))]
pub fn serve_forever(
    _pipe_name: &str,
    _handler: impl FnMut(IpcRequest) -> IpcResponse,
) -> Result<()> {
    Err(SandboxError::UnsupportedPlatform(
        "named pipe IPC is only implemented on Windows".to_string(),
    ))
}

#[cfg(windows)]
pub fn serve_once(
    pipe_name: &str,
    handler: &mut impl FnMut(IpcRequest) -> IpcResponse,
) -> Result<()> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
        PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    let pipe_name_wide = wide_null(pipe_name);
    let pipe = unsafe {
        CreateNamedPipeW(
            pipe_name_wide.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
            PIPE_UNLIMITED_INSTANCES,
            MAX_MESSAGE_BYTES,
            MAX_MESSAGE_BYTES,
            0,
            null(),
        )
    };
    if pipe == INVALID_HANDLE_VALUE {
        return Err(last_error("CreateNamedPipeW"));
    }

    let result = (|| {
        let connected = unsafe { ConnectNamedPipe(pipe, null_mut()) };
        if connected == 0 {
            let code = unsafe { GetLastError() };
            if code != ERROR_PIPE_CONNECTED {
                return Err(last_error_code("ConnectNamedPipe", code));
            }
        }

        let request = read_pipe_message(pipe)?;
        let response = match decode_request(&request) {
            Ok(request) => handler(request),
            Err(error) => IpcResponse::from_error(error),
        };
        let response = encode_response(&response)?;
        write_pipe_message(pipe, &response)?;
        Ok(())
    })();

    unsafe {
        DisconnectNamedPipe(pipe);
        CloseHandle(pipe);
    }

    result
}

#[cfg(not(windows))]
pub fn serve_once(
    _pipe_name: &str,
    _handler: &mut impl FnMut(IpcRequest) -> IpcResponse,
) -> Result<()> {
    Err(SandboxError::UnsupportedPlatform(
        "named pipe IPC is only implemented on Windows".to_string(),
    ))
}

#[cfg(windows)]
fn read_pipe_message(pipe: windows_sys::Win32::Foundation::HANDLE) -> Result<Vec<u8>> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Storage::FileSystem::ReadFile;

    let mut buffer = vec![0u8; MAX_MESSAGE_BYTES as usize];
    let mut bytes_read = 0u32;
    let ok = unsafe {
        ReadFile(
            pipe,
            buffer.as_mut_ptr(),
            MAX_MESSAGE_BYTES,
            &mut bytes_read,
            null_mut(),
        )
    };
    if ok == 0 {
        return Err(last_error("ReadFile"));
    }

    buffer.truncate(bytes_read as usize);
    Ok(buffer)
}

#[cfg(windows)]
fn write_pipe_message(pipe: windows_sys::Win32::Foundation::HANDLE, message: &[u8]) -> Result<()> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Storage::FileSystem::WriteFile;

    if message.len() > MAX_MESSAGE_BYTES as usize {
        return Err(SandboxError::Serialization(format!(
            "IPC message too large: {} bytes",
            message.len()
        )));
    }

    let mut bytes_written = 0u32;
    let ok = unsafe {
        WriteFile(
            pipe,
            message.as_ptr(),
            message.len() as u32,
            &mut bytes_written,
            null_mut(),
        )
    };
    if ok == 0 {
        return Err(last_error("WriteFile"));
    }

    if bytes_written as usize != message.len() {
        return Err(SandboxError::System(format!(
            "short named pipe write: {bytes_written}/{} bytes",
            message.len()
        )));
    }

    Ok(())
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn last_error(api: &str) -> SandboxError {
    let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
    last_error_code(api, code)
}

#[cfg(windows)]
fn last_error_code(api: &str, code: u32) -> SandboxError {
    SandboxError::System(format!("{api} failed with Win32 error {code}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sandbox_common::{CreateSessionRequest, SandboxId};

    #[test]
    fn request_round_trips_as_json() {
        let request = IpcRequest::CreateSession(CreateSessionRequest {
            sandbox_id: Some(SandboxId("test".to_string())),
            user_sid: "S-1-5-21-test".to_string(),
            desktop_name: Some("Sandbox-Test".to_string()),
            profile_root: None,
            policy_version: Some("test".to_string()),
        });

        let encoded = encode_request(&request).expect("request should encode");
        let decoded = decode_request(&encoded).expect("request should decode");

        assert_eq!(decoded, request);
    }

    #[test]
    fn response_round_trips_as_json() {
        let response = IpcResponse::Ok;

        let encoded = encode_response(&response).expect("response should encode");
        let decoded = decode_response(&encoded).expect("response should decode");

        assert_eq!(decoded, response);
    }

    #[test]
    fn invalid_request_returns_serialization_error() {
        let err = decode_request(b"{not-json").expect_err("invalid json should fail");

        assert!(matches!(err, SandboxError::Serialization(_)));
    }
}
