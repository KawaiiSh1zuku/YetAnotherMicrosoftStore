use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

pub const NETWORK_HOST_ALLOWLIST: &[&str] = &[
    "displaycatalog.mp.microsoft.com",
    "fe3.delivery.mp.microsoft.com",
    "dl.delivery.mp.microsoft.com",
    "tlu.dl.delivery.mp.microsoft.com",
];

const SESSION_MARKER: &str = "session-active";
const EVENT_LOG: &str = "events.jsonl";
const MAX_EXPORTED_EVENTS: usize = 200;
const MAX_EVENT_LOG_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticEvent {
    RuntimeStarted,
    PreviousSessionUnclean,
    WorkerFailure,
    Panic,
    DiagnosticsExported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DiagnosticRecord {
    timestamp: u64,
    event: DiagnosticEvent,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticReport<'a> {
    schema_version: u8,
    app_version: &'static str,
    target_architecture: &'static str,
    previous_session_unclean: bool,
    network_host_allowlist: &'a [&'a str],
    events: Vec<DiagnosticRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticExport {
    pub file_name: String,
    pub destination: String,
}

#[derive(Debug, Clone)]
pub struct DiagnosticService {
    state_root: PathBuf,
    export_root: PathBuf,
    enabled: Arc<AtomicBool>,
    previous_session_unclean: Arc<AtomicBool>,
}

impl DiagnosticService {
    pub fn new(
        state_root: impl Into<PathBuf>,
        export_root: impl Into<PathBuf>,
        enabled: bool,
    ) -> io::Result<Self> {
        let state_root = state_root.into();
        let export_root = export_root.into();
        fs::create_dir_all(&state_root)?;
        fs::create_dir_all(&export_root)?;
        let service = Self {
            state_root,
            export_root,
            enabled: Arc::new(AtomicBool::new(false)),
            previous_session_unclean: Arc::new(AtomicBool::new(false)),
        };
        if enabled {
            service.set_enabled(true)?;
        } else {
            service.remove_session_files()?;
        }
        Ok(service)
    }

    pub fn previous_session_unclean(&self) -> bool {
        self.previous_session_unclean.load(Ordering::Acquire)
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub fn set_enabled(&self, enabled: bool) -> io::Result<()> {
        let previous = self.enabled.swap(enabled, Ordering::AcqRel);
        if previous == enabled {
            return Ok(());
        }
        let result = if enabled {
            self.start_session()
        } else {
            self.previous_session_unclean
                .store(false, Ordering::Release);
            self.remove_session_files()
        };
        if result.is_err() {
            self.enabled.store(previous, Ordering::Release);
        }
        result
    }

    pub fn record(&self, event: DiagnosticEvent) -> io::Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }
        let record = DiagnosticRecord {
            timestamp: unix_now(),
            event,
        };
        let mut line = serde_json::to_vec(&record).map_err(io::Error::other)?;
        line.push(b'\n');
        let event_log = self.state_root.join(EVENT_LOG);
        if fs::metadata(&event_log).is_ok_and(|metadata| metadata.len() >= MAX_EVENT_LOG_BYTES) {
            fs::write(&event_log, [])?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(event_log)?;
        file.write_all(&line)
    }

    pub fn export(&self) -> io::Result<DiagnosticExport> {
        let report = DiagnosticReport {
            schema_version: 1,
            app_version: env!("CARGO_PKG_VERSION"),
            target_architecture: std::env::consts::ARCH,
            previous_session_unclean: self.previous_session_unclean(),
            network_host_allowlist: NETWORK_HOST_ALLOWLIST,
            events: if self.is_enabled() {
                self.read_events()?
            } else {
                Vec::new()
            },
        };
        let file_name = format!(
            "yamstore-diagnostics-{}-{}.json",
            unix_now(),
            uuid::Uuid::new_v4().simple()
        );
        let path = self.export_root.join(&file_name);
        let contents = serde_json::to_vec_pretty(&report).map_err(io::Error::other)?;
        fs::write(path, contents)?;
        self.record(DiagnosticEvent::DiagnosticsExported)?;
        Ok(DiagnosticExport {
            file_name,
            destination: "downloads".to_owned(),
        })
    }

    pub fn clean_shutdown(&self) -> io::Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }
        let marker = self.state_root.join(SESSION_MARKER);
        if marker.exists() {
            fs::remove_file(marker)?;
        }
        Ok(())
    }

    fn start_session(&self) -> io::Result<()> {
        let marker = self.state_root.join(SESSION_MARKER);
        let previous_session_unclean = marker.is_file();
        self.previous_session_unclean
            .store(previous_session_unclean, Ordering::Release);
        fs::write(&marker, b"active\n")?;
        if previous_session_unclean {
            self.record(DiagnosticEvent::PreviousSessionUnclean)?;
        }
        self.record(DiagnosticEvent::RuntimeStarted)
    }

    fn remove_session_files(&self) -> io::Result<()> {
        for path in [
            self.state_root.join(SESSION_MARKER),
            self.state_root.join(EVENT_LOG),
        ] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn read_events(&self) -> io::Result<Vec<DiagnosticRecord>> {
        let path = self.state_root.join(EVENT_LOG);
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut records = contents
            .lines()
            .filter_map(|line| serde_json::from_str::<DiagnosticRecord>(line).ok())
            .collect::<Vec<_>>();
        if records.len() > MAX_EXPORTED_EVENTS {
            records.drain(..records.len() - MAX_EXPORTED_EVENTS);
        }
        Ok(records)
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[cfg(windows)]
pub struct SingleInstanceGuard(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl SingleInstanceGuard {
    pub fn acquire() -> io::Result<Self> {
        use windows::{
            core::w,
            Win32::{
                Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
                System::Threading::CreateMutexW,
            },
        };

        let handle = unsafe {
            CreateMutexW(
                None,
                true,
                w!("Local\\YetAnotherMicrosoftStore.Client.SingleInstance"),
            )
        }
        .map_err(io::Error::other)?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            let _ = unsafe { CloseHandle(handle) };
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "another application instance is running",
            ));
        }
        Ok(Self(handle))
    }
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        use windows::Win32::{Foundation::CloseHandle, System::Threading::ReleaseMutex};
        let _ = unsafe { ReleaseMutex(self.0) };
        let _ = unsafe { CloseHandle(self.0) };
    }
}
