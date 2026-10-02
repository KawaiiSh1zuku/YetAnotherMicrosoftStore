use std::{env, io, io::Write, os::windows::io::FromRawHandle, path::PathBuf};

use yet_another_microsoft_store::{
    broker_protocol::{
        decode_frame, encode_frame, BrokerErrorCode, BrokerOperation, BrokerPayload, BrokerRequest,
        BrokerResponse,
    },
    deployment::WindowsDeploymentBackend,
    inventory::WindowsInventory,
    package_validation::VerifiedPackageSet,
};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("deployment broker failed: {error}");
            std::process::ExitCode::from(error.exit_code())
        }
    }
}

#[derive(Debug)]
struct BrokerFailure {
    code: u8,
    message: String,
}

impl BrokerFailure {
    fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn exit_code(&self) -> u8 {
        self.code
    }
}

impl std::fmt::Display for BrokerFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

fn run() -> Result<(), BrokerFailure> {
    ensure_elevated()?;
    let args = Arguments::parse()?;
    let pipe = open_pipe(&args.pipe)?;
    #[cfg(windows)]
    unsafe {
        let mut server_pid = 0;
        let mut server_session = 0;
        windows::Win32::System::Pipes::GetNamedPipeServerProcessId(pipe, &mut server_pid)
            .map_err(|error| BrokerFailure::new(5, error.to_string()))?;
        windows::Win32::System::Pipes::GetNamedPipeServerSessionId(pipe, &mut server_session)
            .map_err(|error| BrokerFailure::new(5, error.to_string()))?;
        if server_pid != args.parent_pid || server_session != args.session_id {
            return Err(BrokerFailure::new(5, "named-pipe caller mismatch"));
        }
        validate_caller_image(server_pid)?;
    }
    let mut file = unsafe { std::fs::File::from_raw_handle(pipe.0 as _) };
    let request_frame = read_frame(&mut file)?;
    let request: BrokerRequest = decode_frame(&request_frame)
        .map_err(|error| BrokerFailure::new(5, format!("invalid request frame: {error:?}")))?;
    if request.nonce != args.nonce
        || request.parent_pid != args.parent_pid
        || request.session_id != args.session_id
    {
        return Err(BrokerFailure::new(5, "caller context or nonce mismatch"));
    }
    request
        .validate_shape()
        .map_err(|error| BrokerFailure::new(5, format!("request rejected: {error:?}")))?;

    let response = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(request)))
    {
        Ok(response) => response,
        Err(_) => BrokerResponse {
            protocol_version: yet_another_microsoft_store::broker_protocol::BROKER_PROTOCOL_VERSION,
            request_id: "panic".to_owned(),
            error: Some(BrokerErrorCode::InvalidRequest),
            stage: "failed".to_owned(),
            hresult: None,
            message: "broker operation panicked".to_owned(),
        },
    };
    let frame = encode_frame(&response)
        .map_err(|error| BrokerFailure::new(5, format!("response encode failed: {error:?}")))?;
    file.write_all(&frame)
        .map_err(|error| BrokerFailure::new(5, error.to_string()))?;
    Ok(())
}

#[cfg(windows)]
fn validate_caller_image(server_pid: u32) -> Result<(), BrokerFailure> {
    use std::path::PathBuf;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, server_pid) }
        .map_err(|error| BrokerFailure::new(5, format!("caller process open failed: {error}")))?;
    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.map_err(|error| BrokerFailure::new(5, format!("caller image query failed: {error}")))?;
    if length == 0 {
        return Err(BrokerFailure::new(5, "caller image path is empty"));
    }
    let caller_path = PathBuf::from(String::from_utf16_lossy(&buffer[..length as usize]));
    if !caller_path.is_file() {
        return Err(BrokerFailure::new(5, "caller image path is not a file"));
    }

    // Debug brokers are used by the local acceptance harness, whose test
    // executable is intentionally unsigned. Release brokers fail closed on
    // an unsigned or untrusted caller image.
    if !cfg!(debug_assertions) {
        verify_authenticode(&caller_path)?;
    }
    Ok(())
}

#[cfg(windows)]
fn verify_authenticode(path: &std::path::Path) -> Result<(), BrokerFailure> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Security::WinTrust::{
        WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
        WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_IGNORE, WTD_UI_NONE,
    };

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        hFile: Default::default(),
        pgKnownSubject: null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut file_info,
        },
        dwStateAction: WTD_STATEACTION_IGNORE,
        ..Default::default()
    };
    let status = unsafe {
        WinVerifyTrust(
            HWND(null_mut()),
            &WINTRUST_ACTION_GENERIC_VERIFY_V2 as *const _ as *mut _,
            (&mut data as *mut WINTRUST_DATA).cast(),
        )
    };
    if status != 0 {
        return Err(BrokerFailure::new(
            5,
            format!("caller image signature rejected: 0x{status:08x}"),
        ));
    }
    Ok(())
}

fn execute(request: BrokerRequest) -> BrokerResponse {
    let request_id = request.request_id.clone();
    let result = match request.operation {
        BrokerOperation::ScanAllUsers => WindowsInventory::scan_all_users()
            .map(|snapshot| serde_json::to_string(&snapshot).unwrap_or_default())
            .map(|message| ("scan-complete".to_owned(), message))
            .map_err(|error| error.to_string()),
        BrokerOperation::InstallAllUsers => install(&request.request_id, request.payload),
        BrokerOperation::UninstallAllUsers => uninstall(request.payload),
    };
    match result {
        Ok((stage, message)) => BrokerResponse {
            protocol_version: yet_another_microsoft_store::broker_protocol::BROKER_PROTOCOL_VERSION,
            request_id,
            error: None,
            stage,
            hresult: None,
            message,
        },
        Err(message) => BrokerResponse {
            protocol_version: yet_another_microsoft_store::broker_protocol::BROKER_PROTOCOL_VERSION,
            request_id,
            error: Some(BrokerErrorCode::InvalidRequest),
            stage: "failed".to_owned(),
            hresult: None,
            message,
        },
    }
}

fn install(request_id: &str, payload: BrokerPayload) -> Result<(String, String), String> {
    let BrokerPayload::Install { packages } = payload else {
        return Err("install payload missing".to_owned());
    };
    let mut iter = packages.into_iter();
    let main = iter
        .next()
        .ok_or_else(|| "main package missing".to_owned())?;
    let root = protected_root(request_id);
    let dependencies = iter.collect::<Vec<_>>();
    let package_set = VerifiedPackageSet { main, dependencies };
    let outcome = WindowsDeploymentBackend::stage_and_provision_all_users(&package_set, &root)
        .map_err(|error| error.to_string())?;
    let _ = std::fs::remove_dir_all(&root);
    Ok((
        "all-users-provisioned".to_owned(),
        serde_json::to_string(&outcome).unwrap_or_default(),
    ))
}

fn uninstall(payload: BrokerPayload) -> Result<(String, String), String> {
    let BrokerPayload::Uninstall { target } = payload else {
        return Err("uninstall payload missing".to_owned());
    };
    let outcome = WindowsDeploymentBackend::deprovision_and_remove_all_users(
        &target.package_family_name,
        &target.package_full_names,
    )
    .map_err(|error| error.to_string())?;
    Ok((
        "all-users-uninstalled".to_owned(),
        serde_json::to_string(&outcome).unwrap_or_default(),
    ))
}

fn protected_root(request_id: &str) -> PathBuf {
    let base = env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let safe_id: String = request_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(64)
        .collect();
    base.join("YetAnotherMicrosoftStore")
        .join("BrokerStaging")
        .join(format!("{}-{safe_id}", std::process::id()))
}

struct Arguments {
    pipe: String,
    nonce: String,
    parent_pid: u32,
    session_id: u32,
}

impl Arguments {
    fn parse() -> Result<Self, BrokerFailure> {
        let mut args = env::args().skip(1);
        let mut pipe = None;
        let mut nonce = None;
        let mut parent_pid = None;
        let mut session_id = None;
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| BrokerFailure::new(5, "missing broker argument"))?;
            match flag.as_str() {
                "--pipe" => pipe = Some(value),
                "--nonce" => nonce = Some(value),
                "--parent-pid" => parent_pid = value.parse().ok(),
                "--session-id" => session_id = value.parse().ok(),
                _ => return Err(BrokerFailure::new(5, "unknown broker argument")),
            }
        }
        Ok(Self {
            pipe: pipe.ok_or_else(|| BrokerFailure::new(5, "missing pipe"))?,
            nonce: nonce.ok_or_else(|| BrokerFailure::new(5, "missing nonce"))?,
            parent_pid: parent_pid.ok_or_else(|| BrokerFailure::new(5, "missing parent pid"))?,
            session_id: session_id.ok_or_else(|| BrokerFailure::new(5, "missing session id"))?,
        })
    }
}

fn read_frame(reader: &mut impl io::Read) -> Result<Vec<u8>, BrokerFailure> {
    let mut header = [0_u8; 4];
    reader
        .read_exact(&mut header)
        .map_err(|error| BrokerFailure::new(5, error.to_string()))?;
    let size = u32::from_le_bytes(header) as usize;
    if size > yet_another_microsoft_store::broker_protocol::MAX_FRAME_BYTES {
        return Err(BrokerFailure::new(5, "frame too large"));
    }
    let mut frame = Vec::with_capacity(size + 4);
    frame.extend_from_slice(&header);
    frame.resize(size + 4, 0);
    reader
        .read_exact(&mut frame[4..])
        .map_err(|error| BrokerFailure::new(5, error.to_string()))?;
    Ok(frame)
}

#[cfg(windows)]
fn open_pipe(name: &str) -> Result<windows::Win32::Foundation::HANDLE, BrokerFailure> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_MODE, OPEN_EXISTING,
    };
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .map_err(|error| BrokerFailure::new(5, error.to_string()))
    }
}

#[cfg(windows)]
fn ensure_elevated() -> Result<(), BrokerFailure> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = Default::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|error| BrokerFailure::new(5, error.to_string()))?;
        let mut elevation = TOKEN_ELEVATION::default();
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut 0,
        )
        .map_err(|error| BrokerFailure::new(5, error.to_string()))?;
        let _ = CloseHandle(token);
        if elevation.TokenIsElevated == 0 {
            return Err(BrokerFailure::new(5, "broker is not elevated"));
        }
    }
    Ok(())
}
