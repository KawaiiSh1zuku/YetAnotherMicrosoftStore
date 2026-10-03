use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, Mutex};

use crate::{
    applicability::{
        select_packages, HostCapabilities, InstalledPackage, SelectionMode, SelectionPreferences,
        SelectionResult,
    },
    cache::CacheManager,
    deployment::DeploymentScope,
    deployment_orchestrator::{
        DeploymentOrchestrator, OrchestrationPreparation, PreparedDeployment,
        SystemDeploymentBackend, SystemPackagePreflight,
    },
    deployment_plan::{build_deployment_plan, DeploymentPlan},
    domain::{Architecture, CacheEntry, PackageKind, PackageVersion},
    download::{
        CancellationToken, DownloadError, DownloadManager, DownloadRequest, VerifiedDownload,
    },
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    inventory::PackageInventoryRecord,
    job_events::{
        CommandOutcome, CommandRejectReason, JobControl, JobEvent, JobTarget, JobTargetRole,
        WorkerLease,
    },
    jobs::{Job, JobKind, JobSnapshot, JobStage},
    persistence::{Persistence, PersistenceError},
    resolver::{PackageGraph, PackageResolver, ResolverError, StoreLibResolverAdapter},
};

pub type WorkerFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Clock {
    fn now(&self) -> i64;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs() as i64)
    }
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub owner_id: String,
    pub lease_ttl: i64,
    pub heartbeat_interval: Duration,
    pub cache_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOnceOutcome {
    Idle,
    LeaseBusy,
    LeaseLost,
    Processed { job_id: String, stage: JobStage },
}

#[derive(Debug)]
pub enum WorkerError {
    Persistence(PersistenceError),
    InvalidConfiguration,
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Persistence(error) => error.fmt(formatter),
            Self::InvalidConfiguration => formatter.write_str("worker configuration is invalid"),
        }
    }
}

impl std::error::Error for WorkerError {}

impl From<PersistenceError> for WorkerError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostEnvironment {
    pub capabilities: HostCapabilities,
    pub installed: Vec<InstalledPackage>,
}

pub trait HostEnvironmentPort {
    fn inspect<'a>(
        &'a mut self,
        job: &'a Job,
    ) -> WorkerFuture<'a, Result<HostEnvironment, AppErrorDto>>;
}

pub trait WorkerResolverPort {
    fn resolve<'a>(
        &'a mut self,
        job: &'a Job,
    ) -> WorkerFuture<'a, Result<PackageGraph, ResolverError>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalePackageResolver;

impl WorkerResolverPort for LocalePackageResolver {
    fn resolve<'a>(
        &'a mut self,
        job: &'a Job,
    ) -> WorkerFuture<'a, Result<PackageGraph, ResolverError>> {
        Box::pin(async move {
            let language = job
                .requested_languages
                .first()
                .map(String::as_str)
                .unwrap_or("en-US");
            let mut resolver =
                StoreLibResolverAdapter::production_for_locale(&job.requested_market, language)?;
            resolver.resolve(&job.product_id).await
        })
    }
}

#[derive(Debug, Clone)]
pub struct SystemHostEnvironment {
    capabilities: HostCapabilities,
}

impl SystemHostEnvironment {
    pub fn new(capabilities: HostCapabilities) -> Self {
        Self { capabilities }
    }
}

impl HostEnvironmentPort for SystemHostEnvironment {
    fn inspect<'a>(
        &'a mut self,
        job: &'a Job,
    ) -> WorkerFuture<'a, Result<HostEnvironment, AppErrorDto>> {
        let capabilities = self.capabilities.clone();
        let scope = job.deployment_scope;
        Box::pin(async move {
            let inventory = tokio::task::spawn_blocking(move || {
                crate::deployment_coordinator::DeploymentCoordinator::scan(scope)
            })
            .await
            .map_err(|_| blocking_error())?
            .map_err(|error| AppErrorDto::from(&error))?;
            if !inventory.complete {
                return Err(blocking_error());
            }
            let installed = inventory
                .records
                .iter()
                .filter(|record| scope_contains(record, scope))
                .filter_map(installed_package)
                .collect();
            Ok(HostEnvironment {
                capabilities,
                installed,
            })
        })
    }
}

#[derive(Clone)]
pub struct DownloadArtifact {
    request: DownloadRequest,
    refresh_job: Job,
}

impl std::fmt::Debug for DownloadArtifact {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DownloadArtifact")
            .field("update_id", &self.request.update_id)
            .field("cache_key", &self.request.cache_key)
            .field("expected_size", &self.request.expected_size)
            .field("expected_sha256", &self.request.expected_sha256)
            .finish_non_exhaustive()
    }
}

impl DownloadArtifact {
    pub fn update_id(&self) -> &str {
        &self.request.update_id
    }

    pub fn cache_key(&self) -> &str {
        &self.request.cache_key
    }

    pub fn cache_root(&self) -> &Path {
        &self.request.cache_root
    }

    pub fn expected_size(&self) -> u64 {
        self.request.expected_size
    }

    pub fn expected_sha256(&self) -> &str {
        &self.request.expected_sha256
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    pub update_id: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

pub trait DownloadPort {
    fn download<'a>(
        &'a mut self,
        artifacts: Vec<DownloadArtifact>,
        cancellation: CancellationToken,
        progress: mpsc::UnboundedSender<DownloadProgress>,
    ) -> WorkerFuture<'a, Result<Vec<VerifiedDownload>, AppErrorDto>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentPreparation<T> {
    AlreadyCurrent,
    Ready(T),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationOutcome {
    Converged,
    NotConverged,
}

pub trait DeploymentPort {
    type Prepared: Send;

    fn prepare<'a>(
        &'a mut self,
        scope: DeploymentScope,
        mode: SelectionMode,
        plan: DeploymentPlan,
    ) -> WorkerFuture<'a, Result<DeploymentPreparation<Self::Prepared>, AppErrorDto>>;

    fn commit<'a>(
        &'a mut self,
        prepared: Self::Prepared,
    ) -> WorkerFuture<'a, Result<(), AppErrorDto>>;

    fn reconcile<'a>(
        &'a mut self,
        scope: DeploymentScope,
        targets: Vec<JobTarget>,
    ) -> WorkerFuture<'a, Result<ReconciliationOutcome, AppErrorDto>>;
}

pub struct SystemDeploymentPort {
    database_path: PathBuf,
}

impl SystemDeploymentPort {
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
        }
    }
}

impl DeploymentPort for SystemDeploymentPort {
    type Prepared = PreparedDeployment;

    fn prepare<'a>(
        &'a mut self,
        scope: DeploymentScope,
        mode: SelectionMode,
        plan: DeploymentPlan,
    ) -> WorkerFuture<'a, Result<DeploymentPreparation<Self::Prepared>, AppErrorDto>> {
        let database_path = self.database_path.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let persistence =
                    Persistence::open(&database_path).map_err(|_| blocking_error())?;
                let mut orchestrator =
                    DeploymentOrchestrator::new(SystemDeploymentBackend, SystemPackagePreflight);
                match orchestrator.prepare(scope, mode, &plan, &persistence, unix_now())? {
                    OrchestrationPreparation::AlreadyCurrent(_) => {
                        Ok(DeploymentPreparation::AlreadyCurrent)
                    }
                    OrchestrationPreparation::Ready(prepared) => {
                        Ok(DeploymentPreparation::Ready(prepared))
                    }
                }
            })
            .await
            .map_err(|_| blocking_error())?
        })
    }

    fn commit<'a>(
        &'a mut self,
        prepared: Self::Prepared,
    ) -> WorkerFuture<'a, Result<(), AppErrorDto>> {
        let database_path = self.database_path.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let persistence =
                    Persistence::open(&database_path).map_err(|_| blocking_error())?;
                let mut orchestrator =
                    DeploymentOrchestrator::new(SystemDeploymentBackend, SystemPackagePreflight);
                orchestrator.commit(prepared, &persistence).map(|_| ())
            })
            .await
            .map_err(|_| blocking_error())?
        })
    }

    fn reconcile<'a>(
        &'a mut self,
        scope: DeploymentScope,
        targets: Vec<JobTarget>,
    ) -> WorkerFuture<'a, Result<ReconciliationOutcome, AppErrorDto>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let inventory = crate::deployment_coordinator::DeploymentCoordinator::scan(scope)
                    .map_err(|error| AppErrorDto::from(&error))?;
                if !inventory.complete {
                    return Err(blocking_error());
                }
                let converged = !targets.is_empty()
                    && targets.iter().all(|target| {
                        inventory.records.iter().any(|record| {
                            target_matches(record, target) && scope_contains(record, scope)
                        })
                    });
                Ok(if converged {
                    ReconciliationOutcome::Converged
                } else {
                    ReconciliationOutcome::NotConverged
                })
            })
            .await
            .map_err(|_| blocking_error())?
        })
    }
}

pub struct ManagerDownloadPort<R = LocalePackageResolver> {
    manager: DownloadManager,
    resolver: Arc<Mutex<R>>,
}

impl ManagerDownloadPort<LocalePackageResolver> {
    pub fn new(manager: DownloadManager) -> Self {
        Self::with_resolver(manager, LocalePackageResolver)
    }
}

impl<R> ManagerDownloadPort<R> {
    pub fn with_resolver(manager: DownloadManager, resolver: R) -> Self {
        Self {
            manager,
            resolver: Arc::new(Mutex::new(resolver)),
        }
    }
}

impl<R> DownloadPort for ManagerDownloadPort<R>
where
    R: WorkerResolverPort + Send,
{
    fn download<'a>(
        &'a mut self,
        artifacts: Vec<DownloadArtifact>,
        cancellation: CancellationToken,
        progress: mpsc::UnboundedSender<DownloadProgress>,
    ) -> WorkerFuture<'a, Result<Vec<VerifiedDownload>, AppErrorDto>> {
        Box::pin(async move {
            let mut verified = Vec::with_capacity(artifacts.len());
            let bytes_total = artifacts.iter().map(DownloadArtifact::expected_size).sum();
            let mut bytes_done = 0_u64;
            for artifact in artifacts {
                let DownloadArtifact {
                    request,
                    refresh_job,
                } = artifact;
                let expected_file_name = request.file_name.clone();
                let expected_size = request.expected_size;
                let expected_sha256 = request.expected_sha256.clone();
                let resolver = Arc::clone(&self.resolver);
                let result = self
                    .manager
                    .download_with_refresh(request, cancellation.clone(), move |update_id| {
                        let resolver = Arc::clone(&resolver);
                        let refresh_job = refresh_job.clone();
                        let update_id = update_id.to_owned();
                        let expected_file_name = expected_file_name.clone();
                        let expected_sha256 = expected_sha256.clone();
                        async move {
                            let graph = {
                                let mut resolver = resolver.lock().await;
                                resolver.resolve(&refresh_job).await
                            }
                            .map_err(|_| DownloadError::UrlExpired)?;
                            refreshed_package_url(
                                &graph,
                                &refresh_job,
                                &update_id,
                                &expected_file_name,
                                expected_size,
                                &expected_sha256,
                            )
                        }
                    })
                    .await
                    .map_err(|error| AppErrorDto::from(&error))?;
                bytes_done = bytes_done.saturating_add(result.size);
                let _ = progress.send(DownloadProgress {
                    update_id: result.update_id.clone(),
                    bytes_done,
                    bytes_total,
                });
                verified.push(result);
            }
            Ok(verified)
        })
    }
}

fn refreshed_package_url(
    graph: &PackageGraph,
    job: &Job,
    update_id: &str,
    expected_file_name: &str,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<String, DownloadError> {
    if graph.product_id.as_deref() != Some(job.product_id.as_str())
        || !graph
            .market
            .as_deref()
            .is_some_and(|market| market.eq_ignore_ascii_case(&job.requested_market))
    {
        return Err(DownloadError::UrlExpired);
    }
    let mut matching = graph
        .packages
        .iter()
        .filter(|package| package.update_id == update_id);
    let package = matching.next().ok_or(DownloadError::UrlExpired)?;
    if matching.next().is_some()
        || package.file_name.as_deref() != Some(expected_file_name)
        || package.file_size != Some(expected_size)
        || !package
            .sha256
            .as_deref()
            .is_some_and(|digest| digest.eq_ignore_ascii_case(expected_sha256))
    {
        return Err(DownloadError::UrlExpired);
    }
    package
        .package_uri
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .map(str::to_owned)
        .ok_or(DownloadError::UrlExpired)
}

pub struct JobWorker<R, H, D, P, C> {
    store: Persistence,
    config: WorkerConfig,
    resolver: R,
    host: H,
    downloader: D,
    deployment: P,
    clock: C,
    recovered: bool,
}

impl<R, H, D, P, C> JobWorker<R, H, D, P, C>
where
    R: WorkerResolverPort,
    H: HostEnvironmentPort,
    D: DownloadPort,
    P: DeploymentPort,
    C: Clock,
{
    pub fn new(
        store: Persistence,
        config: WorkerConfig,
        resolver: R,
        host: H,
        downloader: D,
        deployment: P,
        clock: C,
    ) -> Self {
        Self {
            store,
            config,
            resolver,
            host,
            downloader,
            deployment,
            clock,
            recovered: false,
        }
    }

    pub async fn run_once(&mut self) -> Result<RunOnceOutcome, WorkerError> {
        if self.config.owner_id.is_empty()
            || self.config.lease_ttl <= 0
            || self.config.heartbeat_interval.is_zero()
            || !self.config.cache_root.is_absolute()
        {
            return Err(WorkerError::InvalidConfiguration);
        }
        let now = self.clock.now();
        let Some(mut lease) =
            self.store
                .acquire_worker_lease(&self.config.owner_id, now, self.config.lease_ttl)?
        else {
            return Ok(RunOnceOutcome::LeaseBusy);
        };

        let result = self.run_leased(&mut lease).await;
        let _ = self.store.release_worker_lease(&lease);
        result
    }

    async fn run_leased(&mut self, lease: &mut WorkerLease) -> Result<RunOnceOutcome, WorkerError> {
        if !self.recovered {
            let now = self.clock.now();
            self.store
                .recover_jobs_after_restart_leased(now, lease, now)?;
            self.recovered = true;
        }
        let snapshots = self.store.list_job_snapshots()?;
        let mut selected = None;
        for snapshot in snapshots {
            let has_commands = !self
                .store
                .pending_job_commands(&snapshot.job.job_id)?
                .is_empty();
            if has_commands
                || matches!(
                    snapshot.job.stage,
                    JobStage::Queued | JobStage::Interrupted | JobStage::NeedsReconciliation
                )
            {
                selected = Some(snapshot);
                break;
            }
        }
        let Some(snapshot) = selected else {
            return Ok(RunOnceOutcome::Idle);
        };

        let (snapshot, command_action) =
            process_pending_commands(&self.store, &self.clock, snapshot, lease)?;
        if command_action == CommandAction::Stop {
            return Ok(processed(snapshot));
        }

        if snapshot.job.stage == JobStage::NeedsReconciliation {
            return self.reconcile(snapshot, lease).await;
        }
        let already_resolving = snapshot.job.stage == JobStage::Resolving;
        self.process(snapshot, lease, already_resolving).await
    }

    async fn reconcile(
        &mut self,
        snapshot: JobSnapshot,
        lease: &mut WorkerLease,
    ) -> Result<RunOnceOutcome, WorkerError> {
        let job_id = snapshot.job.job_id.clone();
        let targets = self.store.job_targets(&job_id)?;
        let result = self
            .await_with_heartbeat(lease, |deployment| {
                deployment.reconcile(snapshot.job.deployment_scope, targets)
            })
            .await;
        match result {
            LeaseAware::LeaseLost => Ok(RunOnceOutcome::LeaseLost),
            LeaseAware::Ready(Ok(ReconciliationOutcome::Converged)) => {
                let completed = self.append(lease, snapshot, JobEvent::Completed)?;
                Ok(processed(completed))
            }
            LeaseAware::Ready(Ok(ReconciliationOutcome::NotConverged)) => {
                let failed = self.append(
                    lease,
                    snapshot,
                    JobEvent::Failed {
                        error: safe_error(
                            &job_id,
                            ErrorCode::DeploymentFailed,
                            RetryAdvice::ReconcileInventory,
                        ),
                    },
                )?;
                Ok(processed(failed))
            }
            LeaseAware::Ready(Err(error)) => {
                let failed = self.append(lease, snapshot, JobEvent::Failed { error })?;
                Ok(processed(failed))
            }
        }
    }

    async fn process(
        &mut self,
        mut snapshot: JobSnapshot,
        lease: &mut WorkerLease,
        already_resolving: bool,
    ) -> Result<RunOnceOutcome, WorkerError> {
        let job_id = snapshot.job.job_id.clone();
        if !already_resolving {
            snapshot = self.append(
                lease,
                snapshot,
                JobEvent::StageChanged {
                    stage: JobStage::Resolving,
                },
            )?;
        }
        let graph = match self.resolver.resolve(&snapshot.job).await {
            Ok(graph) => graph,
            Err(error) => {
                let failed = self.append(
                    lease,
                    snapshot,
                    JobEvent::Failed {
                        error: with_job(AppErrorDto::from(&error), &job_id),
                    },
                )?;
                return Ok(processed(failed));
            }
        };
        snapshot = self.append(
            lease,
            snapshot,
            JobEvent::StageChanged {
                stage: JobStage::Selecting,
            },
        )?;
        let environment = match self.host.inspect(&snapshot.job).await {
            Ok(environment) => environment,
            Err(error) => return self.fail(snapshot, lease, with_job(error, &job_id)),
        };
        let mode = match snapshot.job.kind {
            JobKind::Install => SelectionMode::Install,
            JobKind::Update => SelectionMode::Update,
        };
        let preferences = SelectionPreferences {
            market: snapshot.job.requested_market.clone(),
            preferred_architectures: snapshot.job.requested_architectures.clone(),
            preferred_languages: snapshot.job.requested_languages.clone(),
            mode,
        };
        let selection = match select_packages(
            &graph,
            &environment.capabilities,
            &preferences,
            &environment.installed,
        ) {
            Ok(selection) => selection,
            Err(error) => {
                return self.fail(
                    snapshot,
                    lease,
                    with_job(AppErrorDto::from(&error), &job_id),
                )
            }
        };
        let cache_root = self.config.cache_root.clone();
        let (artifacts, targets) = match map_artifacts(&snapshot.job, &selection, &cache_root) {
            Ok(mapped) => mapped,
            Err(error) => return self.fail(snapshot, lease, with_job(error, &job_id)),
        };
        let main = selection
            .packages
            .iter()
            .find(|package| package.package_kind == PackageKind::Main)
            .expect("selection always contains one main package");
        let package_family_name = self
            .store
            .product(&snapshot.job.product_id)?
            .and_then(|product| product.package_family_name)
            .unwrap_or_else(|| {
                main.identity_name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned())
            });
        snapshot = self.append(
            lease,
            snapshot,
            JobEvent::SelectionRecorded {
                selected_update_id: main.update_id.clone(),
                package_family_name,
                version: main.version.to_string(),
                architecture: main.architecture,
                language: main.language.clone(),
                targets,
            },
        )?;
        snapshot = self.append(
            lease,
            snapshot,
            JobEvent::StageChanged {
                stage: JobStage::Downloading,
            },
        )?;

        let download_run = run_download(
            &self.store,
            &self.clock,
            &self.config,
            &mut self.downloader,
            snapshot,
            lease,
            artifacts,
        )
        .await?;
        let (next_snapshot, verified) = match download_run {
            DownloadRun::Completed {
                snapshot,
                downloads,
            } => (snapshot, downloads),
            DownloadRun::Stopped(snapshot) => return Ok(processed(snapshot)),
            DownloadRun::LeaseLost => return Ok(RunOnceOutcome::LeaseLost),
            DownloadRun::Failed { snapshot, error } => {
                return self.fail(snapshot, lease, with_job(error, &job_id))
            }
        };
        snapshot = next_snapshot;
        let cache =
            CacheManager::new(&cache_root).map_err(|_| WorkerError::InvalidConfiguration)?;
        for download in &verified {
            cache
                .record_verified(&self.store, download, Some(&job_id), self.clock.now())
                .map_err(|_| WorkerError::InvalidConfiguration)?;
        }
        snapshot = self.append(
            lease,
            snapshot,
            JobEvent::StageChanged {
                stage: JobStage::Verifying,
            },
        )?;
        let entries = verified
            .iter()
            .map(|download| CacheEntry {
                cache_key: download.cache_key.clone(),
                job_id: Some(job_id.clone()),
                update_id: download.update_id.clone(),
                path: download.path.to_string_lossy().into_owned(),
                size: download.size,
                sha256: download.sha256.clone(),
                state: crate::domain::CacheState::Verified,
                last_accessed_at: self.clock.now(),
            })
            .collect::<Vec<_>>();
        let plan = match build_deployment_plan(&graph, &selection, &entries) {
            Ok(plan) => plan,
            Err(error) => {
                return self.fail(
                    snapshot,
                    lease,
                    with_job(AppErrorDto::from(&error), &job_id),
                )
            }
        };
        let preparation = match self
            .await_with_heartbeat(lease, |deployment| {
                deployment.prepare(snapshot.job.deployment_scope, mode, plan)
            })
            .await
        {
            LeaseAware::LeaseLost => return Ok(RunOnceOutcome::LeaseLost),
            LeaseAware::Ready(Ok(preparation)) => preparation,
            LeaseAware::Ready(Err(error)) => {
                return self.fail(snapshot, lease, with_job(error, &job_id))
            }
        };
        let DeploymentPreparation::Ready(prepared) = preparation else {
            let completed = self.append(lease, snapshot, JobEvent::Completed)?;
            return Ok(processed(completed));
        };
        snapshot = self.append(
            lease,
            snapshot,
            JobEvent::StageChanged {
                stage: JobStage::Deploying,
            },
        )?;
        match self
            .await_with_heartbeat(lease, |deployment| deployment.commit(prepared))
            .await
        {
            LeaseAware::LeaseLost => Ok(RunOnceOutcome::LeaseLost),
            LeaseAware::Ready(Ok(())) => {
                let completed = self.append(lease, snapshot, JobEvent::Completed)?;
                Ok(processed(completed))
            }
            LeaseAware::Ready(Err(error)) => {
                let failed = self.append(
                    lease,
                    snapshot,
                    JobEvent::Failed {
                        error: with_job(error, &job_id),
                    },
                )?;
                Ok(processed(failed))
            }
        }
    }

    fn fail(
        &self,
        snapshot: JobSnapshot,
        lease: &WorkerLease,
        error: AppErrorDto,
    ) -> Result<RunOnceOutcome, WorkerError> {
        let failed = self.append(lease, snapshot, JobEvent::Failed { error })?;
        Ok(processed(failed))
    }

    fn append(
        &self,
        lease: &WorkerLease,
        snapshot: JobSnapshot,
        event: JobEvent,
    ) -> Result<JobSnapshot, WorkerError> {
        let now = self.clock.now();
        self.store
            .append_job_event_leased(
                &snapshot.job.job_id,
                snapshot.sequence,
                event,
                now,
                lease,
                now,
            )
            .map_err(WorkerError::from)
    }

    async fn await_with_heartbeat<T, F>(
        &mut self,
        lease: &mut WorkerLease,
        operation: F,
    ) -> LeaseAware<T>
    where
        F: for<'a> FnOnce(&'a mut P) -> WorkerFuture<'a, T>,
    {
        let future = operation(&mut self.deployment);
        tokio::pin!(future);
        let mut heartbeat = tokio::time::interval(self.config.heartbeat_interval);
        heartbeat.tick().await;
        loop {
            tokio::select! {
                result = &mut future => return LeaseAware::Ready(result),
                _ = heartbeat.tick() => {
                    let now = self.clock.now();
                    match self.store.renew_worker_lease(lease, now, self.config.lease_ttl) {
                        Ok(Some(renewed)) => *lease = renewed,
                        Ok(None) | Err(_) => return LeaseAware::LeaseLost,
                    }
                }
            }
        }
    }
}

enum LeaseAware<T> {
    Ready(T),
    LeaseLost,
}

enum DownloadRun {
    Completed {
        snapshot: JobSnapshot,
        downloads: Vec<VerifiedDownload>,
    },
    Failed {
        snapshot: JobSnapshot,
        error: AppErrorDto,
    },
    Stopped(JobSnapshot),
    LeaseLost,
}

async fn run_download<D, C>(
    store: &Persistence,
    clock: &C,
    config: &WorkerConfig,
    downloader: &mut D,
    mut snapshot: JobSnapshot,
    lease: &mut WorkerLease,
    artifacts: Vec<DownloadArtifact>,
) -> Result<DownloadRun, WorkerError>
where
    D: DownloadPort,
    C: Clock,
{
    let cancellation = CancellationToken::new();
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    let future = downloader.download(artifacts, cancellation.clone(), progress_tx);
    tokio::pin!(future);
    let mut heartbeat = tokio::time::interval(config.heartbeat_interval);
    let mut controls = tokio::time::interval(config.heartbeat_interval);
    heartbeat.tick().await;
    controls.tick().await;
    loop {
        tokio::select! {
            result = &mut future => {
                return Ok(match result {
                    Ok(downloads) => DownloadRun::Completed { snapshot, downloads },
                    Err(_) if cancellation.is_cancelled() => DownloadRun::Stopped(snapshot),
                    Err(error) => DownloadRun::Failed { snapshot, error },
                });
            }
            progress = progress_rx.recv() => {
                if let Some(progress) = progress {
                    let now = clock.now();
                    snapshot = store.append_job_event_leased(
                        &snapshot.job.job_id,
                        snapshot.sequence,
                        JobEvent::ProgressRecorded {
                            bytes_done: progress.bytes_done,
                            bytes_total: Some(progress.bytes_total),
                        },
                        now,
                        lease,
                        now,
                    )?;
                }
            }
            _ = controls.tick() => {
                let (next, action) = process_pending_commands(store, clock, snapshot, lease)?;
                snapshot = next;
                if action == CommandAction::Stop {
                    cancellation.cancel();
                    return Ok(DownloadRun::Stopped(snapshot));
                }
            }
            _ = heartbeat.tick() => {
                let now = clock.now();
                match store.renew_worker_lease(lease, now, config.lease_ttl) {
                    Ok(Some(renewed)) => *lease = renewed,
                    Ok(None) | Err(_) => {
                        cancellation.cancel();
                        return Ok(DownloadRun::LeaseLost);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandAction {
    Continue,
    Stop,
}

fn process_pending_commands<C: Clock>(
    store: &Persistence,
    clock: &C,
    mut snapshot: JobSnapshot,
    lease: &WorkerLease,
) -> Result<(JobSnapshot, CommandAction), WorkerError> {
    let commands = store.pending_job_commands(&snapshot.job.job_id)?;
    let mut action = CommandAction::Continue;
    for command in commands {
        let now = clock.now();
        if command.expected_sequence != snapshot.sequence {
            store.finish_job_command_leased(
                &command.command_id,
                CommandOutcome::Rejected {
                    reason: CommandRejectReason::StaleSequence,
                },
                now,
                lease,
                now,
            )?;
            continue;
        }
        let event = match (command.control, snapshot.job.stage) {
            (JobControl::Pause, JobStage::Downloading) => Some(JobEvent::StageChanged {
                stage: JobStage::Paused,
            }),
            (JobControl::Resume, JobStage::Paused | JobStage::Interrupted | JobStage::Failed) => {
                Some(JobEvent::StageChanged {
                    stage: JobStage::Resolving,
                })
            }
            (
                JobControl::Cancel,
                JobStage::Queued
                | JobStage::Resolving
                | JobStage::Selecting
                | JobStage::Downloading
                | JobStage::Paused
                | JobStage::Verifying
                | JobStage::Interrupted
                | JobStage::Failed,
            ) => Some(JobEvent::Cancelled),
            (JobControl::Pause, JobStage::Paused) | (JobControl::Cancel, JobStage::Cancelled) => {
                store.finish_job_command_leased(
                    &command.command_id,
                    CommandOutcome::AlreadySatisfied,
                    now,
                    lease,
                    now,
                )?;
                action = CommandAction::Stop;
                continue;
            }
            _ => {
                store.finish_job_command_leased(
                    &command.command_id,
                    CommandOutcome::Rejected {
                        reason: CommandRejectReason::InvalidStage,
                    },
                    now,
                    lease,
                    now,
                )?;
                continue;
            }
        };
        snapshot = store.apply_job_command(
            &command.command_id,
            event.expect("applied controls always have an event"),
            CommandOutcome::Applied,
            now,
            lease,
            now,
        )?;
        action = if matches!(snapshot.job.stage, JobStage::Resolving) {
            CommandAction::Continue
        } else {
            CommandAction::Stop
        };
    }
    Ok((snapshot, action))
}

fn processed(snapshot: JobSnapshot) -> RunOnceOutcome {
    RunOnceOutcome::Processed {
        job_id: snapshot.job.job_id,
        stage: snapshot.job.stage,
    }
}

fn map_artifacts(
    job: &Job,
    selection: &SelectionResult,
    cache_root: &Path,
) -> Result<(Vec<DownloadArtifact>, Vec<JobTarget>), AppErrorDto> {
    let mut artifacts = Vec::with_capacity(selection.packages.len());
    let mut targets = Vec::with_capacity(selection.packages.len());
    for package in &selection.packages {
        let Some(url) = package.package_uri.clone() else {
            return Err(mapping_error());
        };
        let Some(file_name) = package.file_name.clone() else {
            return Err(mapping_error());
        };
        let Some(expected_size) = package.file_size else {
            return Err(mapping_error());
        };
        let Some(expected_sha256) = package.sha256.clone() else {
            return Err(mapping_error());
        };
        let Some(identity_name) = package.identity_name.clone() else {
            return Err(mapping_error());
        };
        let Some(publisher) = package.publisher.clone() else {
            return Err(mapping_error());
        };
        let cache_key = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}", package.update_id, expected_sha256).as_bytes())
        );
        artifacts.push(DownloadArtifact {
            request: DownloadRequest {
                job_id: Some(job.job_id.clone()),
                update_id: package.update_id.clone(),
                cache_key,
                url,
                file_name,
                expected_size,
                expected_sha256: expected_sha256.clone(),
                cache_root: cache_root.to_path_buf(),
            },
            refresh_job: job.clone(),
        });
        targets.push(JobTarget {
            role: match package.package_kind {
                PackageKind::Main => JobTargetRole::Main,
                PackageKind::Resource => JobTargetRole::Resource,
                PackageKind::Framework | PackageKind::Unknown => JobTargetRole::Dependency,
            },
            update_id: package.update_id.clone(),
            identity_name,
            publisher,
            version: package.version.to_string(),
            architecture: package.architecture,
            resource_id: package.resource_id.clone(),
            package_kind: package.package_kind,
            expected_size,
            sha256: expected_sha256.to_ascii_lowercase(),
        });
    }
    Ok((artifacts, targets))
}

fn mapping_error() -> AppErrorDto {
    AppErrorDto::new(ErrorCode::SourceIdentityMismatch, RetryAdvice::ReResolve)
}

fn safe_error(job_id: &str, code: ErrorCode, retry: RetryAdvice) -> AppErrorDto {
    with_job(AppErrorDto::new(code, retry), job_id)
}

fn with_job(mut error: AppErrorDto, job_id: &str) -> AppErrorDto {
    error.job_id = Some(job_id.to_owned());
    error
}

fn unix_now() -> i64 {
    SystemClock.now()
}

fn blocking_error() -> AppErrorDto {
    AppErrorDto::new(ErrorCode::DeploymentFailed, RetryAdvice::ReconcileInventory)
}

fn installed_package(record: &PackageInventoryRecord) -> Option<InstalledPackage> {
    Some(InstalledPackage {
        identity_name: record.identity_name.clone(),
        publisher: Some(record.publisher.clone()),
        version: PackageVersion::new(
            record.version[0],
            record.version[1],
            record.version[2],
            record.version[3],
        ),
        architecture: parse_architecture(&record.architecture)?,
    })
}

fn parse_architecture(value: &str) -> Option<Architecture> {
    if value.eq_ignore_ascii_case("x64") {
        Some(Architecture::X64)
    } else if value.eq_ignore_ascii_case("x86") {
        Some(Architecture::X86)
    } else if value.eq_ignore_ascii_case("arm64") {
        Some(Architecture::Arm64)
    } else if value.eq_ignore_ascii_case("arm") {
        Some(Architecture::Arm)
    } else if value.eq_ignore_ascii_case("neutral") {
        Some(Architecture::Neutral)
    } else {
        None
    }
}

fn target_matches(record: &PackageInventoryRecord, target: &JobTarget) -> bool {
    let expected = target.version.parse::<PackageVersion>();
    record
        .identity_name
        .eq_ignore_ascii_case(&target.identity_name)
        && record.publisher == target.publisher
        && parse_architecture(&record.architecture).is_some_and(|architecture| {
            target.architecture == Architecture::Neutral || architecture == target.architecture
        })
        && record
            .resource_id
            .eq_ignore_ascii_case(target.resource_id.as_deref().unwrap_or_default())
        && expected.is_ok_and(|version| {
            let installed = PackageVersion::new(
                record.version[0],
                record.version[1],
                record.version[2],
                record.version[3],
            );
            match target.role {
                JobTargetRole::Main => installed == version,
                JobTargetRole::Dependency | JobTargetRole::Resource => installed >= version,
            }
        })
}

fn scope_contains(record: &PackageInventoryRecord, scope: DeploymentScope) -> bool {
    match scope {
        DeploymentScope::CurrentUser => record.installed_for_current_user,
        DeploymentScope::AllUsers => {
            record.provisioned_for_future_users
                || (record.package_kind == crate::inventory::PackageKind::Framework
                    && record.installed_user_count > 0)
        }
    }
}
