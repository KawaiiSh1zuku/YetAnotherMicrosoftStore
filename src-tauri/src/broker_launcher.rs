use std::{fmt, io};

#[cfg(windows)]
use std::os::windows::io::FromRawHandle;

use crate::broker_protocol::{
    decode_frame, encode_frame, BrokerErrorCode, BrokerRequest, BrokerResponse,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerLaunchError {
    UnsupportedPlatform,
    InvalidRequest(BrokerErrorCode),
    CallerContextMismatch,
    UacCancelled,
    BrokerRejected,
    Timeout,
    BrokerUnavailable,
    PipeFailure,
}

impl fmt::Display for BrokerLaunchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("the elevated broker requires Windows")
            }
            Self::InvalidRequest(code) => write!(formatter, "invalid broker request: {code:?}"),
            Self::CallerContextMismatch => formatter.write_str("broker caller context mismatch"),
            Self::UacCancelled => formatter.write_str("UAC elevation was cancelled"),
            Self::BrokerRejected => formatter.write_str("broker rejected the request"),
            Self::Timeout => formatter.write_str("broker timed out"),
            Self::BrokerUnavailable => formatter.write_str("deployment broker is unavailable"),
            Self::PipeFailure => formatter.write_str("broker IPC failed"),
        }
    }
}

impl std::error::Error for BrokerLaunchError {}

pub fn classify_broker_exit_code(code: u32) -> Result<(), BrokerLaunchError> {
    match code {
        0 => Ok(()),
        1223 => Err(BrokerLaunchError::UacCancelled),
        _ => Err(BrokerLaunchError::BrokerRejected),
    }
}

pub fn validate_launch_context(
    expected_pid: u32,
    expected_session: u32,
    actual_pid: u32,
    actual_session: u32,
) -> Result<(), BrokerLaunchError> {
    if expected_pid == 0
        || expected_session == 0
        || expected_pid != actual_pid
        || expected_session != actual_session
    {
        return Err(BrokerLaunchError::CallerContextMismatch);
    }
    Ok(())
}

pub struct BrokerLauncher;

impl BrokerLauncher {
    pub fn install_all_users(request: BrokerRequest) -> Result<BrokerResponse, BrokerLaunchError> {
        Self::send(request)
    }

    pub fn uninstall_all_users(
        request: BrokerRequest,
    ) -> Result<BrokerResponse, BrokerLaunchError> {
        Self::send(request)
    }

    pub fn scan_all_users(request: BrokerRequest) -> Result<BrokerResponse, BrokerLaunchError> {
        Self::send(request)
    }

    #[cfg(not(windows))]
    fn send(request: BrokerRequest) -> Result<BrokerResponse, BrokerLaunchError> {
        request
            .validate_shape()
            .map_err(BrokerLaunchError::InvalidRequest)?;
        Err(BrokerLaunchError::UnsupportedPlatform)
    }

    #[cfg(windows)]
    fn send(request: BrokerRequest) -> Result<BrokerResponse, BrokerLaunchError> {
        request
            .validate_shape()
            .map_err(BrokerLaunchError::InvalidRequest)?;

        let (parent_pid, session_id) =
            current_process_context().map_err(|_| BrokerLaunchError::CallerContextMismatch)?;
        validate_launch_context(
            request.parent_pid,
            request.session_id,
            parent_pid,
            session_id,
        )?;

        let pipe_id = uuid::Uuid::new_v4().simple().to_string();
        let pipe_name = format!(r"\\.\pipe\YetAnotherMicrosoftStore-M0-{pipe_id}");
        let pipe = create_pipe(&pipe_name)?;
        let broker_path = broker_path().ok_or(BrokerLaunchError::BrokerUnavailable)?;
        let args = format!(
            r#"--pipe "{}" --nonce "{}" --parent-pid {} --session-id {}"#,
            pipe_name, request.nonce, request.parent_pid, request.session_id
        );
        let process = launch_elevated(&broker_path, &args)?;
        unsafe {
            windows::Win32::System::Pipes::ConnectNamedPipe(pipe, None)
                .or_else(|error| {
                    (windows::Win32::Foundation::GetLastError()
                        == windows::Win32::Foundation::ERROR_PIPE_CONNECTED)
                        .then_some(())
                        .ok_or(error)
                })
                .map_err(|_| BrokerLaunchError::PipeFailure)?;
        }

        let mut pipe_file = unsafe { std::fs::File::from_raw_handle(pipe.0 as _) };
        let frame = encode_frame(&request).map_err(BrokerLaunchError::InvalidRequest)?;
        io::Write::write_all(&mut pipe_file, &frame).map_err(|_| BrokerLaunchError::PipeFailure)?;
        let response_frame = read_frame(&mut pipe_file)?;
        let exit_code = wait_process(process)?;
        classify_broker_exit_code(exit_code)?;
        let response: BrokerResponse =
            decode_frame(&response_frame).map_err(BrokerLaunchError::InvalidRequest)?;
        if response.error.is_some() {
            return Err(BrokerLaunchError::BrokerRejected);
        }
        Ok(response)
    }
}

#[cfg(windows)]
fn current_process_context() -> Result<(u32, u32), ()> {
    let pid = unsafe { windows::Win32::System::Threading::GetCurrentProcessId() };
    let mut session = 0;
    unsafe { windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(pid, &mut session) }
        .map_err(|_| ())?;
    Ok((pid, session))
}

#[cfg(windows)]
fn broker_path() -> Option<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("M0_BROKER_PATH") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let current = std::env::current_exe().ok()?;
    let candidate = current.parent()?.join("deployment-broker.exe");
    candidate.is_file().then_some(candidate)
}

#[cfg(windows)]
fn create_pipe(name: &str) -> Result<windows::Win32::Foundation::HANDLE, BrokerLaunchError> {
    use windows::core::PCWSTR;
    use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows::Win32::System::Pipes::{CreateNamedPipeW, NAMED_PIPE_MODE, PIPE_WAIT};

    let name_wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let sddl_wide: Vec<u16> = "D:P(A;;GA;;;OW)(A;;GA;;;BA)(A;;GA;;;SY)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = windows::Win32::Security::PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_wide.as_ptr()),
            1,
            &mut descriptor as *mut _,
            None,
        )
        .map_err(|_| BrokerLaunchError::PipeFailure)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let pipe = CreateNamedPipeW(
            PCWSTR(name_wide.as_ptr()),
            FILE_FLAG_FIRST_PIPE_INSTANCE | PIPE_ACCESS_DUPLEX,
            NAMED_PIPE_MODE(PIPE_WAIT.0),
            1,
            1024 * 1024,
            1024 * 1024,
            120_000,
            Some(&attributes),
        );
        windows::Win32::Foundation::LocalFree(Some(windows::Win32::Foundation::HLOCAL(
            descriptor.0,
        )));
        if pipe.is_invalid() {
            return Err(BrokerLaunchError::PipeFailure);
        }
        Ok(pipe)
    }
}

#[cfg(windows)]
fn launch_elevated(
    path: &std::path::Path,
    args: &str,
) -> Result<windows::Win32::Foundation::HANDLE, BrokerLaunchError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_CANCELLED;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

    let verb: Vec<u16> = "runas".encode_utf16().chain(Some(0)).collect();
    let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let parameters: Vec<u16> = args.encode_utf16().chain(Some(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: Default::default(),
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        lpDirectory: PCWSTR::null(),
        nShow: 0,
        ..Default::default()
    };
    unsafe {
        if let Err(error) = ShellExecuteExW(&mut info) {
            if error.code().0 as u32 == ERROR_CANCELLED.0 {
                return Err(BrokerLaunchError::UacCancelled);
            }
            return Err(BrokerLaunchError::BrokerUnavailable);
        }
        if info.hProcess.is_invalid() {
            return Err(BrokerLaunchError::BrokerUnavailable);
        }
        Ok(info.hProcess)
    }
}

#[cfg(windows)]
fn wait_process(process: windows::Win32::Foundation::HANDLE) -> Result<u32, BrokerLaunchError> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    unsafe {
        let wait = WaitForSingleObject(process, 120_000);
        if wait.0 != 0 {
            let _ = CloseHandle(process);
            return Err(BrokerLaunchError::Timeout);
        }
        let mut code = 1;
        GetExitCodeProcess(process, &mut code).map_err(|_| BrokerLaunchError::BrokerUnavailable)?;
        let _ = CloseHandle(process);
        Ok(code)
    }
}

#[cfg(windows)]
fn read_frame(reader: &mut impl io::Read) -> Result<Vec<u8>, BrokerLaunchError> {
    let mut header = [0_u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|_| BrokerLaunchError::PipeFailure)?;
    let size = u32::from_le_bytes(header) as usize;
    if size > crate::broker_protocol::MAX_FRAME_BYTES {
        return Err(BrokerLaunchError::PipeFailure);
    }
    let mut frame = Vec::with_capacity(size + 4);
    frame.extend_from_slice(&header);
    frame.resize(size + 4, 0);
    reader
        .read_exact(&mut frame[4..])
        .map_err(|_| BrokerLaunchError::PipeFailure)?;
    Ok(frame)
}
