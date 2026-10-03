use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use sha2::{Digest, Sha256};
use yet_another_microsoft_store_lib::{
    applicability::{HostCapabilities, InstalledPackage},
    deployment::DeploymentScope,
    domain::{Architecture, PackageFormat, PackageKind, PackageVersion},
    download::{CancellationToken, DownloadManager, VerifiedDownload},
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    job_events::{CommandOutcome, JobCommand, JobControl, JobEvent, JobTarget},
    job_worker::{
        Clock, DeploymentPort, DeploymentPreparation, DownloadArtifact, DownloadPort,
        DownloadProgress, HostEnvironment, HostEnvironmentPort, JobWorker, ManagerDownloadPort,
        ReconciliationOutcome, RunOnceOutcome, WorkerConfig, WorkerFuture, WorkerResolverPort,
    },
    jobs::{Job, JobKind, JobStage},
    persistence::Persistence,
    resolver::{PackageGraph, ResolvedPackage, ResolverError},
    settings::{NetworkPolicy, ProxyRoute},
};

struct TestDatabase(PathBuf);

impl TestDatabase {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "yamstore-m6-worker-{}.sqlite3",
            uuid::Uuid::new_v4()
        )))
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let _ = fs::remove_file(self.0.with_extension("sqlite3-shm"));
        let _ = fs::remove_file(self.0.with_extension("sqlite3-wal"));
        let _ = fs::remove_dir_all(self.0.with_extension("cache"));
    }
}

struct StatusServer {
    address: SocketAddr,
    requests: Arc<AtomicUsize>,
    shutdown: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl StatusServer {
    fn start(status: u16) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        listener
            .set_nonblocking(true)
            .expect("set fixture listener nonblocking");
        let address = listener.local_addr().expect("fixture address");
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_for_thread = Arc::clone(&requests);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_for_thread = Arc::clone(&shutdown);
        let join = thread::spawn(move || {
            while !shutdown_for_thread.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("accept fixture request: {error}"),
                };
                stream
                    .set_nonblocking(false)
                    .expect("set fixture stream blocking");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("set fixture read timeout");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).expect("read fixture request");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                requests_for_thread.fetch_add(1, Ordering::SeqCst);
                let reason = if status == 403 {
                    "Forbidden"
                } else {
                    "Fixture"
                };
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write fixture response");
                break;
            }
        });
        Self {
            address,
            requests,
            shutdown,
            join: Some(join),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.address, path)
    }

    fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for StatusServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            join.join().expect("join fixture server");
        }
    }
}

#[derive(Clone)]
struct TestClock(Arc<Mutex<i64>>);

impl TestClock {
    fn new(now: i64) -> Self {
        Self(Arc::new(Mutex::new(now)))
    }
}

impl Clock for TestClock {
    fn now(&self) -> i64 {
        *self.0.lock().expect("clock")
    }
}

struct FakeResolver(PackageGraph);

impl WorkerResolverPort for FakeResolver {
    fn resolve<'a>(
        &'a mut self,
        _job: &'a Job,
    ) -> WorkerFuture<'a, Result<PackageGraph, ResolverError>> {
        let graph = self.0.clone();
        Box::pin(async move { Ok(graph) })
    }
}

struct RecordingRefreshResolver {
    graph: PackageGraph,
    calls: Arc<AtomicUsize>,
    jobs: Arc<Mutex<Vec<Job>>>,
}

impl WorkerResolverPort for RecordingRefreshResolver {
    fn resolve<'a>(
        &'a mut self,
        job: &'a Job,
    ) -> WorkerFuture<'a, Result<PackageGraph, ResolverError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.jobs.lock().expect("refresh jobs").push(job.clone());
        let graph = self.graph.clone();
        Box::pin(async move { Ok(graph) })
    }
}

struct FakeHost;

impl HostEnvironmentPort for FakeHost {
    fn inspect<'a>(
        &'a mut self,
        _job: &'a Job,
    ) -> WorkerFuture<'a, Result<HostEnvironment, AppErrorDto>> {
        Box::pin(async {
            Ok(HostEnvironment {
                capabilities: HostCapabilities {
                    os_version: PackageVersion::new(10, 0, 19045, 0),
                    native_architecture: Architecture::X64,
                    compatible_architectures: vec![Architecture::X64, Architecture::Neutral],
                    supported_formats: vec![PackageFormat::Msix],
                },
                installed: Vec::<InstalledPackage>::new(),
            })
        })
    }
}

struct FakeDownload {
    delay: Duration,
}

impl DownloadPort for FakeDownload {
    fn download<'a>(
        &'a mut self,
        artifacts: Vec<DownloadArtifact>,
        cancellation: CancellationToken,
        progress: tokio::sync::mpsc::UnboundedSender<DownloadProgress>,
    ) -> WorkerFuture<'a, Result<Vec<VerifiedDownload>, AppErrorDto>> {
        Box::pin(async move {
            tokio::time::sleep(self.delay).await;
            if cancellation.is_cancelled() {
                return Err(AppErrorDto::new(
                    ErrorCode::DownloadFailed,
                    RetryAdvice::Retry,
                ));
            }
            let mut verified = Vec::new();
            for artifact in artifacts {
                let path = artifact
                    .cache_root()
                    .join("verified")
                    .join(format!("{}.msix", artifact.expected_sha256()));
                fs::create_dir_all(path.parent().expect("verified parent")).expect("cache dir");
                fs::write(&path, b"x").expect("fake verified file");
                let _ = progress.send(DownloadProgress {
                    update_id: artifact.update_id().to_owned(),
                    bytes_done: artifact.expected_size(),
                    bytes_total: artifact.expected_size(),
                });
                verified.push(VerifiedDownload {
                    update_id: artifact.update_id().to_owned(),
                    cache_key: artifact.cache_key().to_owned(),
                    path,
                    size: artifact.expected_size(),
                    sha256: artifact.expected_sha256().to_owned(),
                });
            }
            Ok(verified)
        })
    }
}

#[derive(Default)]
struct DeploymentCalls {
    prepared: Vec<JobStage>,
    committed: usize,
    reconciled: Vec<Vec<JobTarget>>,
}

struct FakeDeployment {
    calls: Arc<Mutex<DeploymentCalls>>,
    reconciliation: ReconciliationOutcome,
    prepare_delay: Duration,
    commit_delay: Duration,
    expire_clock_on_prepare: Option<Arc<Mutex<i64>>>,
    expire_clock: Option<Arc<Mutex<i64>>>,
}

impl DeploymentPort for FakeDeployment {
    type Prepared = ();

    fn prepare<'a>(
        &'a mut self,
        _scope: DeploymentScope,
        _mode: yet_another_microsoft_store_lib::applicability::SelectionMode,
        _plan: yet_another_microsoft_store_lib::deployment_plan::DeploymentPlan,
    ) -> WorkerFuture<'a, Result<DeploymentPreparation<Self::Prepared>, AppErrorDto>> {
        self.calls
            .lock()
            .expect("calls")
            .prepared
            .push(JobStage::Verifying);
        if let Some(clock) = &self.expire_clock_on_prepare {
            *clock.lock().expect("clock") = 1_000;
        }
        let delay = self.prepare_delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(DeploymentPreparation::Ready(()))
        })
    }

    fn commit<'a>(
        &'a mut self,
        _prepared: Self::Prepared,
    ) -> WorkerFuture<'a, Result<(), AppErrorDto>> {
        self.calls.lock().expect("calls").committed += 1;
        if let Some(clock) = &self.expire_clock {
            *clock.lock().expect("clock") = 1_000;
        }
        let delay = self.commit_delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(())
        })
    }

    fn reconcile<'a>(
        &'a mut self,
        _scope: DeploymentScope,
        targets: Vec<JobTarget>,
    ) -> WorkerFuture<'a, Result<ReconciliationOutcome, AppErrorDto>> {
        self.calls.lock().expect("calls").reconciled.push(targets);
        let result = self.reconciliation;
        Box::pin(async move { Ok(result) })
    }
}

fn package_graph() -> PackageGraph {
    PackageGraph {
        product_id: Some("product-1".to_owned()),
        market: Some("CN".to_owned()),
        packages: vec![ResolvedPackage {
            package_moniker: "Example.App_1.0.0.0_x64__abc".to_owned(),
            package_type: "msix".to_owned(),
            package_uri: Some("https://packages.example.test/secret.msix?token=secret".to_owned()),
            file_name: Some("example.msix".to_owned()),
            file_size: Some(1),
            sha256: Some("ab".repeat(32)),
            update_id: "main-update".to_owned(),
            identity_name: Some("Example.App".to_owned()),
            publisher: Some("CN=Example".to_owned()),
            version: PackageVersion::new(1, 0, 0, 0),
            architecture: Architecture::X64,
            resource_id: None,
            package_kind: PackageKind::Main,
            minimum_os_version: None,
            language: None,
            is_neutral: Some(true),
            content_id: Some("content-main".to_owned()),
            format: PackageFormat::Msix,
            prerequisites: Vec::new(),
            bundled_updates: Vec::new(),
        }],
        dependencies: Vec::new(),
        framework_requirements: Vec::new(),
    }
}

fn queued_job(job_id: &str, now: i64) -> Job {
    Job {
        job_id: job_id.to_owned(),
        kind: JobKind::Install,
        product_id: "product-1".to_owned(),
        requested_market: "CN".to_owned(),
        requested_architectures: vec![Architecture::X64],
        requested_languages: vec!["zh-CN".to_owned()],
        deployment_scope: DeploymentScope::CurrentUser,
        selected_update_id: None,
        package_family_name: None,
        stage: JobStage::Queued,
        bytes_done: 0,
        bytes_total: None,
        version: None,
        architecture: None,
        language: None,
        error: None,
        created_at: now,
        updated_at: now,
    }
}

fn seed_active_job(store: &Persistence, job_id: &str, active_stages: &[JobStage]) {
    store
        .append_job_event(
            job_id,
            0,
            JobEvent::Created {
                job: queued_job(job_id, 100),
            },
            100,
        )
        .expect("create");
    let mut sequence = 1;
    for stage in [JobStage::Resolving, JobStage::Selecting] {
        store
            .append_job_event(job_id, sequence, JobEvent::StageChanged { stage }, 100)
            .expect("advance");
        sequence += 1;
    }
    store
        .append_job_event(
            job_id,
            sequence,
            JobEvent::SelectionRecorded {
                selected_update_id: "main-update".to_owned(),
                package_family_name: "Example.App_abc".to_owned(),
                version: "1.0.0.0".to_owned(),
                architecture: Architecture::X64,
                language: None,
                targets: vec![JobTarget {
                    role: yet_another_microsoft_store_lib::job_events::JobTargetRole::Main,
                    update_id: "main-update".to_owned(),
                    identity_name: "Example.App".to_owned(),
                    publisher: "CN=Example".to_owned(),
                    version: "1.0.0.0".to_owned(),
                    architecture: Architecture::X64,
                    resource_id: None,
                    package_kind: PackageKind::Main,
                    expected_size: 1,
                    sha256: "ab".repeat(32),
                }],
            },
            100,
        )
        .expect("freeze selection");
    sequence += 1;
    for stage in active_stages {
        store
            .append_job_event(
                job_id,
                sequence,
                JobEvent::StageChanged { stage: *stage },
                100,
            )
            .expect("advance");
        sequence += 1;
    }
}

fn seed_deploying_job(store: &Persistence, job_id: &str) {
    seed_active_job(
        store,
        job_id,
        &[
            JobStage::Downloading,
            JobStage::Verifying,
            JobStage::Deploying,
        ],
    );
}

fn config(owner_id: &str, cache_root: PathBuf) -> WorkerConfig {
    WorkerConfig {
        owner_id: owner_id.to_owned(),
        lease_ttl: 60,
        heartbeat_interval: Duration::from_millis(5),
        cache_root,
    }
}

fn worker(
    database: &TestDatabase,
    owner_id: &str,
    calls: Arc<Mutex<DeploymentCalls>>,
) -> JobWorker<FakeResolver, FakeHost, FakeDownload, FakeDeployment, TestClock> {
    JobWorker::new(
        Persistence::open(&database.0).expect("worker store"),
        config(owner_id, database.0.with_extension("cache")),
        FakeResolver(package_graph()),
        FakeHost,
        FakeDownload {
            delay: Duration::ZERO,
        },
        FakeDeployment {
            calls,
            reconciliation: ReconciliationOutcome::Converged,
            prepare_delay: Duration::ZERO,
            commit_delay: Duration::ZERO,
            expire_clock_on_prepare: None,
            expire_clock: None,
        },
        TestClock::new(100),
    )
}

fn command(job_id: &str, control: JobControl, expected_sequence: u64) -> JobCommand {
    JobCommand {
        command_id: format!("command-{job_id}-{control:?}-{expected_sequence}"),
        job_id: job_id.to_owned(),
        control,
        expected_sequence,
        created_at: 100,
        processed_at: None,
        outcome: None,
    }
}

#[tokio::test]
async fn run_once_fences_duplicate_workers_and_processes_oldest_job() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    store
        .append_job_event(
            "job-a",
            0,
            JobEvent::Created {
                job: queued_job("job-a", 100),
            },
            100,
        )
        .expect("create");
    let held = store
        .acquire_worker_lease("other", 100, 60)
        .expect("lease")
        .expect("acquired");

    let calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut blocked = worker(&database, "worker-b", calls.clone());
    assert_eq!(
        blocked.run_once().await.expect("run"),
        RunOnceOutcome::LeaseBusy
    );
    assert_eq!(
        store.job("job-a").expect("job").unwrap().stage,
        JobStage::Queued
    );

    assert!(store.release_worker_lease(&held).expect("release"));
    let mut active = worker(&database, "worker-a", calls.clone());
    assert!(matches!(
        active.run_once().await.expect("run"),
        RunOnceOutcome::Processed { job_id, stage: JobStage::Completed } if job_id == "job-a"
    ));
    assert_eq!(calls.lock().expect("calls").committed, 1);
}

#[tokio::test]
async fn happy_path_freezes_safe_targets_and_persists_deploying_before_commit() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    store
        .append_job_event(
            "job-safe",
            0,
            JobEvent::Created {
                job: queued_job("job-safe", 100),
            },
            100,
        )
        .expect("create");
    let calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut worker = worker(&database, "worker", calls);
    worker.run_once().await.expect("run");

    let events = store.list_job_events(0, 100).expect("events");
    let stages = events
        .iter()
        .filter_map(|stored| match &stored.event {
            JobEvent::StageChanged { stage } => Some(*stage),
            JobEvent::Completed => Some(JobStage::Completed),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        stages,
        vec![
            JobStage::Resolving,
            JobStage::Selecting,
            JobStage::Downloading,
            JobStage::Verifying,
            JobStage::Deploying,
            JobStage::Completed,
        ]
    );
    let json = serde_json::to_string(&events).expect("serialize safe events");
    assert!(!json.contains("packages.example.test"));
    assert!(!json.contains("secret.msix"));
    assert!(!json.contains("verified"));
    assert_eq!(store.job_targets("job-safe").expect("targets").len(), 1);
}

#[tokio::test]
async fn all_users_moves_directly_from_verifying_to_deploying() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    let mut job = queued_job("job-all-users", 100);
    job.deployment_scope = DeploymentScope::AllUsers;
    store
        .append_job_event("job-all-users", 0, JobEvent::Created { job }, 100)
        .expect("create");

    let calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut worker = worker(&database, "worker", calls);
    worker.run_once().await.expect("run");

    let stages = store
        .list_job_events(0, 100)
        .expect("events")
        .into_iter()
        .filter_map(|stored| match stored.event {
            JobEvent::StageChanged { stage } => Some(stage),
            JobEvent::Completed => Some(JobStage::Completed),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        stages,
        vec![
            JobStage::Resolving,
            JobStage::Selecting,
            JobStage::Downloading,
            JobStage::Verifying,
            JobStage::Deploying,
            JobStage::Completed,
        ]
    );
}

#[tokio::test]
async fn recovered_deploying_job_reconciles_without_committing_again() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    seed_deploying_job(&store, "job-reconcile");

    let calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut worker = worker(&database, "worker", calls.clone());
    let result = worker.run_once().await.expect("run");
    assert!(matches!(
        result,
        RunOnceOutcome::Processed {
            stage: JobStage::Completed,
            ..
        }
    ));
    let calls = calls.lock().expect("calls");
    assert_eq!(calls.committed, 0);
    assert_eq!(calls.reconciled.len(), 1);
}

#[test]
fn worker_error_values_remain_redacted() {
    let error = AppErrorDto::new(ErrorCode::DeploymentFailed, RetryAdvice::ReconcileInventory);
    let json = serde_json::to_string(&error).expect("serialize");
    assert!(!json.contains("C:\\"));
    assert!(!json.contains("https://"));
}

#[tokio::test]
async fn pause_during_download_cancels_at_boundary_and_resume_restarts_resolution() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    store
        .append_job_event(
            "job-control",
            0,
            JobEvent::Created {
                job: queued_job("job-control", 100),
            },
            100,
        )
        .expect("create");
    let calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut worker = JobWorker::new(
        Persistence::open(&database.0).expect("worker store"),
        config("worker-control", database.0.with_extension("cache")),
        FakeResolver(package_graph()),
        FakeHost,
        FakeDownload {
            delay: Duration::from_millis(50),
        },
        FakeDeployment {
            calls,
            reconciliation: ReconciliationOutcome::Converged,
            prepare_delay: Duration::ZERO,
            commit_delay: Duration::ZERO,
            expire_clock_on_prepare: None,
            expire_clock: None,
        },
        TestClock::new(100),
    );
    let command_store = Persistence::open(&database.0).expect("command store");
    let enqueue_pause = async {
        loop {
            let snapshot = command_store
                .job_snapshot("job-control")
                .expect("snapshot")
                .expect("job");
            if snapshot.job.stage == JobStage::Downloading {
                let pause = command("job-control", JobControl::Pause, snapshot.sequence);
                command_store
                    .enqueue_job_command(&pause)
                    .expect("enqueue pause");
                return pause;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    let (run, pause) = tokio::join!(worker.run_once(), enqueue_pause);
    assert!(matches!(
        run.expect("pause run"),
        RunOnceOutcome::Processed {
            stage: JobStage::Paused,
            ..
        }
    ));
    assert_eq!(
        store
            .enqueue_job_command(&pause)
            .expect("read pause outcome")
            .outcome,
        Some(CommandOutcome::Applied)
    );

    let paused = store
        .job_snapshot("job-control")
        .expect("snapshot")
        .expect("job");
    let resume = command("job-control", JobControl::Resume, paused.sequence);
    store.enqueue_job_command(&resume).expect("enqueue resume");
    assert!(matches!(
        worker.run_once().await.expect("resume run"),
        RunOnceOutcome::Processed {
            stage: JobStage::Completed,
            ..
        }
    ));
    assert_eq!(
        store
            .enqueue_job_command(&resume)
            .expect("read resume outcome")
            .outcome,
        Some(CommandOutcome::Applied)
    );
}

#[tokio::test]
async fn cancel_is_atomic_and_a_terminal_command_id_is_idempotent() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    store
        .append_job_event(
            "job-cancel",
            0,
            JobEvent::Created {
                job: queued_job("job-cancel", 100),
            },
            100,
        )
        .expect("create");
    let cancel = command("job-cancel", JobControl::Cancel, 1);
    store.enqueue_job_command(&cancel).expect("enqueue cancel");
    let calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut worker = worker(&database, "worker", calls);
    assert!(matches!(
        worker.run_once().await.expect("cancel run"),
        RunOnceOutcome::Processed {
            stage: JobStage::Cancelled,
            ..
        }
    ));

    assert_eq!(
        worker.run_once().await.expect("terminal run"),
        RunOnceOutcome::Idle
    );
    assert_eq!(
        store
            .enqueue_job_command(&cancel)
            .expect("read duplicate outcome")
            .outcome,
        Some(CommandOutcome::Applied)
    );

    let terminal = store
        .job_snapshot("job-cancel")
        .expect("snapshot")
        .expect("job");
    let invalid_resume = command("job-cancel", JobControl::Resume, terminal.sequence);
    assert!(store.enqueue_job_command(&invalid_resume).is_err());
}

#[tokio::test]
async fn restart_during_download_recovers_then_re_resolves() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    seed_active_job(&store, "job-restart-download", &[JobStage::Downloading]);
    let calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut worker = worker(&database, "worker", calls);
    assert!(matches!(
        worker.run_once().await.expect("recover run"),
        RunOnceOutcome::Processed {
            stage: JobStage::Completed,
            ..
        }
    ));
    let events = store.list_job_events(0, 100).expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|stored| matches!(
                stored.event,
                JobEvent::Recovered {
                    stage: JobStage::Interrupted
                }
            ))
            .count(),
        1
    );
}

#[tokio::test]
async fn lease_loss_during_blocking_commit_never_writes_terminal_state() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    store
        .append_job_event(
            "job-lease-loss",
            0,
            JobEvent::Created {
                job: queued_job("job-lease-loss", 100),
            },
            100,
        )
        .expect("create");
    let clock = TestClock::new(100);
    let calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut worker = JobWorker::new(
        Persistence::open(&database.0).expect("worker store"),
        config("worker-expiring", database.0.with_extension("cache")),
        FakeResolver(package_graph()),
        FakeHost,
        FakeDownload {
            delay: Duration::ZERO,
        },
        FakeDeployment {
            calls,
            reconciliation: ReconciliationOutcome::Converged,
            prepare_delay: Duration::ZERO,
            commit_delay: Duration::from_millis(50),
            expire_clock_on_prepare: None,
            expire_clock: Some(clock.0.clone()),
        },
        clock,
    );
    assert_eq!(
        worker.run_once().await.expect("run"),
        RunOnceOutcome::LeaseLost
    );
    assert_eq!(
        store.job("job-lease-loss").expect("job").unwrap().stage,
        JobStage::Deploying
    );
    assert!(!store
        .list_job_events(0, 100)
        .expect("events")
        .iter()
        .any(|stored| matches!(stored.event, JobEvent::Completed)));
}

#[tokio::test]
async fn lease_loss_during_slow_prepare_never_enters_deployment() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    store
        .append_job_event(
            "job-prepare-lease-loss",
            0,
            JobEvent::Created {
                job: queued_job("job-prepare-lease-loss", 100),
            },
            100,
        )
        .expect("create");
    let clock = TestClock::new(100);
    let calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut worker = JobWorker::new(
        Persistence::open(&database.0).expect("worker store"),
        config(
            "worker-prepare-expiring",
            database.0.with_extension("cache"),
        ),
        FakeResolver(package_graph()),
        FakeHost,
        FakeDownload {
            delay: Duration::ZERO,
        },
        FakeDeployment {
            calls: calls.clone(),
            reconciliation: ReconciliationOutcome::Converged,
            prepare_delay: Duration::from_millis(50),
            commit_delay: Duration::ZERO,
            expire_clock_on_prepare: Some(clock.0.clone()),
            expire_clock: None,
        },
        clock,
    );

    assert_eq!(
        worker.run_once().await.expect("run"),
        RunOnceOutcome::LeaseLost
    );
    assert_eq!(
        store
            .job("job-prepare-lease-loss")
            .expect("job")
            .unwrap()
            .stage,
        JobStage::Verifying
    );
    let calls = calls.lock().expect("calls");
    assert_eq!(calls.committed, 0);
}

struct RecordingResolver {
    calls: Arc<Mutex<Vec<(String, String)>>>,
}

impl WorkerResolverPort for RecordingResolver {
    fn resolve<'a>(
        &'a mut self,
        job: &'a Job,
    ) -> WorkerFuture<'a, Result<PackageGraph, ResolverError>> {
        let market = job.requested_market.clone();
        let language = job.requested_languages.first().cloned().unwrap_or_default();
        self.calls
            .lock()
            .expect("resolver calls")
            .push((market.clone(), language));
        let mut graph = package_graph();
        graph.market = Some(market);
        Box::pin(async move { Ok(graph) })
    }
}

#[tokio::test]
async fn resolver_port_receives_each_jobs_requested_locale() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    let cn = queued_job("job-cn", 100);
    let mut us = queued_job("job-us", 101);
    us.requested_market = "US".to_owned();
    us.requested_languages = vec!["en-US".to_owned()];
    store
        .append_job_event("job-cn", 0, JobEvent::Created { job: cn }, 100)
        .expect("create cn");
    store
        .append_job_event("job-us", 0, JobEvent::Created { job: us }, 101)
        .expect("create us");
    let resolver_calls = Arc::new(Mutex::new(Vec::new()));
    let deployment_calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut worker = JobWorker::new(
        Persistence::open(&database.0).expect("worker store"),
        config("worker-locales", database.0.with_extension("cache")),
        RecordingResolver {
            calls: resolver_calls.clone(),
        },
        FakeHost,
        FakeDownload {
            delay: Duration::ZERO,
        },
        FakeDeployment {
            calls: deployment_calls,
            reconciliation: ReconciliationOutcome::Converged,
            prepare_delay: Duration::ZERO,
            commit_delay: Duration::ZERO,
            expire_clock_on_prepare: None,
            expire_clock: None,
        },
        TestClock::new(101),
    );
    worker.run_once().await.expect("cn run");
    worker.run_once().await.expect("us run");
    assert_eq!(
        *resolver_calls.lock().expect("resolver calls"),
        vec![
            ("CN".to_owned(), "zh-CN".to_owned()),
            ("US".to_owned(), "en-US".to_owned()),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn manager_download_port_re_resolves_an_expired_url_at_most_once() {
    let first_expired = StatusServer::start(403);
    let refreshed_expired = StatusServer::start(403);
    let payload = b"x";
    let digest = format!("{:x}", Sha256::digest(payload));
    let mut initial_graph = package_graph();
    initial_graph.packages[0].package_uri = Some(first_expired.url("/expired"));
    initial_graph.packages[0].file_size = Some(payload.len() as u64);
    initial_graph.packages[0].sha256 = Some(digest.clone());
    let mut refreshed_graph = initial_graph.clone();
    refreshed_graph.packages[0].package_uri = Some(refreshed_expired.url("/still-expired"));

    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("store");
    let job = queued_job("job-refresh", 100);
    store
        .append_job_event(&job.job_id, 0, JobEvent::Created { job: job.clone() }, 100)
        .expect("create job");
    let refresh_calls = Arc::new(AtomicUsize::new(0));
    let refresh_jobs = Arc::new(Mutex::new(Vec::new()));
    let manager = DownloadManager::new(
        ProxyRoute::Disabled,
        NetworkPolicy::loopback_fixture(5),
        1,
        None,
    )
    .expect("download manager");
    let deployment_calls = Arc::new(Mutex::new(DeploymentCalls::default()));
    let mut worker = JobWorker::new(
        Persistence::open(&database.0).expect("worker store"),
        config("worker-refresh", database.0.with_extension("cache")),
        FakeResolver(initial_graph),
        FakeHost,
        ManagerDownloadPort::with_resolver(
            manager,
            RecordingRefreshResolver {
                graph: refreshed_graph,
                calls: Arc::clone(&refresh_calls),
                jobs: Arc::clone(&refresh_jobs),
            },
        ),
        FakeDeployment {
            calls: deployment_calls,
            reconciliation: ReconciliationOutcome::Converged,
            prepare_delay: Duration::ZERO,
            commit_delay: Duration::ZERO,
            expire_clock_on_prepare: None,
            expire_clock: None,
        },
        TestClock::new(100),
    );

    let outcome = worker.run_once().await.expect("worker run");

    assert_eq!(
        outcome,
        RunOnceOutcome::Processed {
            job_id: job.job_id.clone(),
            stage: JobStage::Failed,
        }
    );
    assert_eq!(first_expired.request_count(), 1);
    let snapshot = store
        .job_snapshot("job-refresh")
        .expect("snapshot query")
        .expect("job snapshot");
    let mut expected_error =
        AppErrorDto::new(ErrorCode::DownloadUrlExpired, RetryAdvice::ReResolve);
    expected_error.job_id = Some(job.job_id.clone());
    assert_eq!(snapshot.job.error, Some(expected_error));
    assert_eq!(refresh_calls.load(Ordering::SeqCst), 1);
    let refresh_jobs = refresh_jobs.lock().expect("refresh jobs");
    assert_eq!(refresh_jobs.len(), 1);
    assert_eq!(refresh_jobs[0].product_id, job.product_id);
    assert_eq!(refresh_jobs[0].requested_market, job.requested_market);
    assert_eq!(refresh_jobs[0].requested_languages, job.requested_languages);
    drop(refresh_jobs);
    assert_eq!(refreshed_expired.request_count(), 1);
}
