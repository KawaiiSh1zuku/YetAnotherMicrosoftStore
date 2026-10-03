use std::{
    collections::HashSet,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{stream, StreamExt};
use tokio::sync::Notify;

use crate::{
    applicability::{
        package_incompatibility_reason, preview_packages, select_packages, HostCapabilities,
        InstalledPackage, SelectionMode, SelectionPreferences,
    },
    cache::CacheManager,
    catalog::{
        CatalogIdentifier, CatalogMetadataState, CatalogProduct, CatalogProvider, DeviceFamily,
        StoreLibCatalogAdapter,
    },
    deployment::DeploymentScope,
    deployment_coordinator::DeploymentCoordinator,
    domain::{
        AppSettings, Architecture, PackageFormat, PackageKind, PackageVersion, ProductRecord,
        ProxyCredentialPolicy, ProxyMode, ThemeMode,
    },
    download::DownloadManager,
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    identity::{AssociationConfidence, PackageAssociation},
    inventory::{derive_update_scope, PackageInventoryRecord, PackageKind as InventoryPackageKind},
    job_events::JobCommand,
    job_worker::{
        JobWorker, LocalePackageResolver, ManagerDownloadPort, RunOnceOutcome, SystemClock,
        SystemDeploymentPort, SystemHostEnvironment, WorkerConfig, WorkerError,
    },
    jobs::{Job, JobKind, JobSnapshot, JobStage},
    persistence::{Persistence, PersistenceError},
    resolver::{PackageGraph, PackageResolver, StoreLibResolverAdapter},
    settings::{DefaultProxyProvider, NetworkPolicy, ProxyProvider, MICROSOFT_PACKAGE_HOSTS},
    tauri_api::{
        ApiAppSettings, ApiBackend, ApiCatalogProduct, ApiDeploymentScope, ApiFuture,
        ApiUpdateCandidate, ApiUpdateScanResult, ApiUpdateSkipReason, ApiUpdateSkipped,
        AppDetailsSource, DetailsRequest, JobControlRequest, JobView, ListJobEventsRequest,
        SearchRequest, StartJobSpec,
    },
};

const WORKER_LEASE_TTL_SECONDS: i64 = 30;
const WORKER_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const UPDATE_SCAN_CONCURRENCY_LIMIT: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    database_path: PathBuf,
    cache_root: PathBuf,
}

impl RuntimePaths {
    pub fn new(
        database_path: impl Into<PathBuf>,
        cache_root: impl Into<PathBuf>,
    ) -> Result<Self, AppErrorDto> {
        let database_path = database_path.into();
        let cache_root = cache_root.into();
        if !database_path.is_absolute()
            || !cache_root.is_absolute()
            || has_parent_component(&database_path)
            || has_parent_component(&cache_root)
            || database_path.file_name().is_none()
            || path_is_within(&database_path, &cache_root)
            || database_path.is_dir()
            || cache_root.is_file()
        {
            return Err(runtime_error(
                ErrorCode::DeploymentDenied,
                RetryAdvice::Never,
            ));
        }
        Ok(Self {
            database_path,
            cache_root,
        })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn cache_root(&self) -> &Path {
        &self.cache_root
    }
}

#[derive(Default)]
struct WorkerWakeInner {
    notify: Notify,
    generation: AtomicU64,
}

#[derive(Clone, Default)]
pub struct WorkerWake(Arc<WorkerWakeInner>);

impl WorkerWake {
    pub fn wake(&self) {
        self.0.generation.fetch_add(1, Ordering::AcqRel);
        self.0.notify.notify_one();
    }

    pub fn generation(&self) -> u64 {
        self.0.generation.load(Ordering::Acquire)
    }

    pub async fn notified(&self) {
        self.0.notify.notified().await;
    }
}

#[derive(Clone)]
pub struct ProductionApiBackend {
    paths: RuntimePaths,
    wake: WorkerWake,
}

pub type ProductionJobWorker = JobWorker<
    LocalePackageResolver,
    SystemHostEnvironment,
    ManagerDownloadPort,
    SystemDeploymentPort,
    SystemClock,
>;

impl ProductionApiBackend {
    pub fn new(paths: RuntimePaths) -> Self {
        Self {
            paths,
            wake: WorkerWake::default(),
        }
    }

    pub fn paths(&self) -> &RuntimePaths {
        &self.paths
    }

    pub fn worker_wake(&self) -> WorkerWake {
        self.wake.clone()
    }

    pub fn create_worker(&self) -> Result<ProductionJobWorker, AppErrorDto> {
        let persistence = self.open_persistence()?;
        let settings = self.load_settings(&persistence)?;
        let proxy = DefaultProxyProvider
            .resolve(&settings, None)
            .map_err(|_| runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Never))?;
        let network_policy = NetworkPolicy::production(MICROSOFT_PACKAGE_HOSTS)
            .map_err(|_| runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Never))?;
        let manager = DownloadManager::new(
            proxy,
            network_policy,
            settings.max_concurrent_downloads as usize,
            None,
        )
        .map_err(|error| AppErrorDto::from(&error))?;
        let capabilities = system_host_capabilities()?;
        let config = WorkerConfig {
            owner_id: format!("runtime-{}", uuid::Uuid::new_v4()),
            lease_ttl: WORKER_LEASE_TTL_SECONDS,
            heartbeat_interval: WORKER_HEARTBEAT_INTERVAL,
            cache_root: self.paths.cache_root.clone(),
        };
        Ok(JobWorker::new(
            persistence,
            config,
            LocalePackageResolver,
            SystemHostEnvironment::new(capabilities),
            ManagerDownloadPort::new(manager),
            SystemDeploymentPort::new(self.paths.database_path.clone()),
            SystemClock,
        ))
    }

    pub async fn run_worker_once(&self) -> Result<RunOnceOutcome, AppErrorDto> {
        self.create_worker()?.run_once().await.map_err(worker_error)
    }

    fn open_persistence(&self) -> Result<Persistence, AppErrorDto> {
        ensure_database_parent(&self.paths.database_path)?;
        Persistence::open(&self.paths.database_path).map_err(persistence_error)
    }

    fn load_settings(&self, persistence: &Persistence) -> Result<AppSettings, AppErrorDto> {
        let mut settings = match persistence.settings().map_err(persistence_error)? {
            Some(settings) => settings,
            None => default_settings(&self.paths.cache_root),
        };
        let configured_cache = self.paths.cache_root.to_string_lossy().into_owned();
        if settings.cache_directory != configured_cache {
            settings.cache_directory = configured_cache;
        }
        persistence
            .save_settings(&settings)
            .map_err(persistence_error)?;
        Ok(settings)
    }

    fn job_view(
        &self,
        persistence: &Persistence,
        snapshot: JobSnapshot,
    ) -> Result<JobView, AppErrorDto> {
        let title = persistence
            .product(&snapshot.job.product_id)
            .map_err(persistence_error)?
            .and_then(|product| product.app_name);
        Ok(JobView { snapshot, title })
    }

    fn start_job(&self, request: StartJobSpec, kind: JobKind) -> Result<JobView, AppErrorDto> {
        let persistence = self.open_persistence()?;
        let product = persistence
            .product(&request.product_id)
            .map_err(persistence_error)?
            .ok_or_else(|| runtime_error(ErrorCode::CatalogNotFound, RetryAdvice::ReResolve))?;
        if !product.market.eq_ignore_ascii_case(&request.market) {
            return Err(runtime_error(
                ErrorCode::MarketUnavailable,
                RetryAdvice::ReResolve,
            ));
        }
        let settings = self.load_settings(&persistence)?;
        let now = unix_now();
        let requested_languages = prioritized_languages(&request.language, &settings);
        let job = Job {
            job_id: format!("job-{}", uuid::Uuid::new_v4()),
            kind,
            product_id: request.product_id,
            requested_market: request.market,
            requested_architectures: settings.preferred_architectures,
            requested_languages,
            deployment_scope: request.scope,
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
        };
        persistence.save_job(&job).map_err(persistence_error)?;
        let snapshot = persistence
            .job_snapshot(&job.job_id)
            .map_err(persistence_error)?
            .ok_or_else(|| runtime_error(ErrorCode::DeploymentFailed, RetryAdvice::Retry))?;
        self.wake.wake();
        Ok(JobView {
            snapshot,
            title: product.app_name,
        })
    }
}

impl ApiBackend for ProductionApiBackend {
    fn search_apps(&self, request: SearchRequest) -> ApiFuture<'_, Vec<CatalogProduct>> {
        let backend = self.clone();
        Box::pin(async move {
            let mut catalog =
                StoreLibCatalogAdapter::production_for_locale(&request.market, &request.language)
                    .map_err(|error| AppErrorDto::from(&error))?;
            let products = catalog
                .search(&request.query, DeviceFamily::Desktop)
                .await
                .map_err(|error| AppErrorDto::from(&error))?;
            let market = request.market.clone();
            let language = request.language.clone();
            let mut products = stream::iter(products.into_iter().take(20).enumerate())
                .map(|(index, summary)| {
                    let market = market.clone();
                    let language = language.clone();
                    async move {
                        let details =
                            match StoreLibCatalogAdapter::production_for_locale(&market, &language)
                            {
                                Ok(mut catalog) => catalog.product(&summary.product_id).await.ok(),
                                Err(_) => None,
                            };
                        (index, merge_catalog_metadata(summary, details))
                    }
                })
                .buffer_unordered(4)
                .collect::<Vec<_>>()
                .await;
            products.sort_by_key(|(index, _)| *index);
            let products = products
                .into_iter()
                .map(|(_, product)| product)
                .collect::<Vec<_>>();
            let persistence = backend.open_persistence()?;
            for product in &products {
                persist_catalog_product(&persistence, product, &request.market, &request.language)?;
            }
            Ok(products)
        })
    }

    fn get_app_details(&self, request: DetailsRequest) -> ApiFuture<'_, AppDetailsSource> {
        let backend = self.clone();
        Box::pin(async move {
            let mut catalog =
                StoreLibCatalogAdapter::production_for_locale(&request.market, &request.language)
                    .map_err(|error| AppErrorDto::from(&error))?;
            let product = catalog
                .product(&request.product_id)
                .await
                .map_err(|error| AppErrorDto::from(&error))?;
            let persistence = backend.open_persistence()?;
            persist_catalog_product(&persistence, &product, &request.market, &request.language)?;
            let settings = backend.load_settings(&persistence)?;
            let mut resolver =
                StoreLibResolverAdapter::production_for_locale(&request.market, &request.language)
                    .map_err(|error| AppErrorDto::from(&error))?;
            let graph = resolver
                .resolve(&request.product_id)
                .await
                .map_err(|error| AppErrorDto::from(&error))?;
            validate_resolved_product(&graph, &request.product_id, &request.market)?;
            let host = system_host_capabilities()?;
            let preferred_languages = prioritized_languages(&request.language, &settings);
            let preferences = SelectionPreferences {
                market: request.market,
                preferred_architectures: settings.preferred_architectures,
                preferred_languages,
                mode: SelectionMode::Install,
            };
            let selection_preview = preview_packages(&graph, &host, &preferences, &[]);
            let supported_architectures = selection_preview
                .main
                .as_ref()
                .map(|main| vec![main.architecture])
                .unwrap_or_default();
            Ok(AppDetailsSource {
                product,
                supported_architectures,
                selection_preview,
            })
        })
    }

    fn scan_installed_packages(
        &self,
        scope: DeploymentScope,
    ) -> ApiFuture<'_, crate::inventory::InventorySnapshot> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || DeploymentCoordinator::scan(scope))
                .await
                .map_err(|_| runtime_error(ErrorCode::DeploymentFailed, RetryAdvice::Retry))?
                .map_err(|error| AppErrorDto::from(&error))
        })
    }

    fn scan_updates(&self) -> ApiFuture<'_, ApiUpdateScanResult> {
        let backend = self.clone();
        Box::pin(async move {
            let inventory = tokio::task::spawn_blocking(|| {
                DeploymentCoordinator::scan(DeploymentScope::AllUsers)
            })
            .await
            .map_err(|_| runtime_error(ErrorCode::DeploymentFailed, RetryAdvice::Retry))?
            .map_err(|error| AppErrorDto::from(&error))?;
            let persistence = backend.open_persistence()?;
            let settings = backend.load_settings(&persistence)?;
            let update_scan_concurrency =
                usize::try_from(settings.max_concurrent_update_scans).unwrap_or(1);
            let mut complete = inventory.complete;
            let mut scanned_families = HashSet::new();
            let mut association_jobs = Vec::new();
            for installed in inventory.records.into_iter().filter(|installed| {
                installed.package_kind == InventoryPackageKind::Main
                    && scanned_families.insert(installed.package_family_name.to_ascii_lowercase())
            }) {
                let association = persistence
                    .package_association(&installed.package_family_name)
                    .map_err(persistence_error)?
                    .filter(|association| {
                        matches!(
                            association.confidence,
                            AssociationConfidence::VerifiedDeployment
                                | AssociationConfidence::ExactPackageFamilyName
                                | AssociationConfidence::ExactIdentityPublisher
                        ) && association.product_id.is_some()
                    });
                association_jobs.push(UpdateAssociationJob {
                    index: association_jobs.len(),
                    installed,
                    association,
                });
            }
            let scanned_main_packages = association_jobs.len();
            let association_settings = settings.clone();
            let mut association_outcomes =
                collect_bounded(association_jobs, update_scan_concurrency, move |job| {
                    let settings = association_settings.clone();
                    async move { associate_update_package(job, &settings).await }
                })
                .await;
            association_outcomes.sort_by_key(UpdateAssociationOutcome::index);

            let mut resolution_jobs = Vec::new();
            let mut skipped = Vec::new();
            for outcome in association_outcomes {
                let UpdateAssociationReady {
                    index,
                    installed,
                    association,
                    product,
                    lookup_language,
                } = match outcome {
                    UpdateAssociationOutcome::Ready(ready) => *ready,
                    UpdateAssociationOutcome::Skipped { skipped: item, .. } => {
                        complete = false;
                        skipped.push(item);
                        continue;
                    }
                };
                if let Some(product) = product {
                    let language = lookup_language.as_deref().unwrap_or("en-US");
                    persist_catalog_product(&persistence, &product, &settings.market, language)?;
                    persistence
                        .upsert_package_association(&association)
                        .map_err(persistence_error)?;
                }
                let product_id = association.product_id.ok_or_else(|| {
                    runtime_error(ErrorCode::SourceIdentityMismatch, RetryAdvice::Never)
                })?;
                let product = persistence
                    .product(&product_id)
                    .map_err(persistence_error)?;
                let market = product
                    .as_ref()
                    .map(|product| product.market.as_str())
                    .unwrap_or(&settings.market);
                let language = product
                    .as_ref()
                    .and_then(|product| product.languages.first())
                    .or_else(|| settings.preferred_languages.first())
                    .map(String::as_str)
                    .ok_or_else(|| {
                        runtime_error(ErrorCode::CatalogUnavailable, RetryAdvice::ReResolve)
                    })?;
                let market = market.to_owned();
                let language = language.to_owned();
                resolution_jobs.push(UpdateResolutionJob {
                    index,
                    installed,
                    product_id,
                    product,
                    market,
                    language,
                });
            }
            let associated_packages = resolution_jobs.len();
            let host = system_host_capabilities()?;
            let resolution_settings = settings.clone();
            let mut resolution_outcomes =
                collect_bounded(resolution_jobs, update_scan_concurrency, move |job| {
                    let host = host.clone();
                    let settings = resolution_settings.clone();
                    async move { resolve_update_package(job, &host, &settings).await }
                })
                .await;
            resolution_outcomes.sort_by_key(UpdateResolutionOutcome::index);

            let mut candidates = Vec::new();
            for outcome in resolution_outcomes {
                match outcome {
                    UpdateResolutionOutcome::Candidate { candidate, .. } => {
                        candidates.push(candidate);
                    }
                    UpdateResolutionOutcome::UpToDate { .. } => {}
                    UpdateResolutionOutcome::Skipped { skipped: item, .. } => {
                        complete = false;
                        skipped.push(item);
                    }
                }
            }
            Ok(ApiUpdateScanResult {
                scanned_main_packages,
                associated_packages,
                candidates,
                skipped,
                complete,
            })
        })
    }

    fn start_install(&self, request: StartJobSpec) -> ApiFuture<'_, JobView> {
        let backend = self.clone();
        Box::pin(async move { backend.start_job(request, JobKind::Install) })
    }

    fn start_update(&self, request: StartJobSpec) -> ApiFuture<'_, JobView> {
        let backend = self.clone();
        Box::pin(async move { backend.start_job(request, JobKind::Update) })
    }

    fn request_job_control(&self, request: JobControlRequest) -> ApiFuture<'_, JobView> {
        let backend = self.clone();
        Box::pin(async move {
            let persistence = backend.open_persistence()?;
            persistence
                .enqueue_job_command(&JobCommand {
                    command_id: request.command_id,
                    job_id: request.job_id.clone(),
                    control: request.control,
                    expected_sequence: request.expected_sequence,
                    created_at: unix_now(),
                    processed_at: None,
                    outcome: None,
                })
                .map_err(persistence_error)?;
            let snapshot = persistence
                .job_snapshot(&request.job_id)
                .map_err(persistence_error)?
                .ok_or_else(|| runtime_error(ErrorCode::DeploymentFailed, RetryAdvice::Never))?;
            let view = backend.job_view(&persistence, snapshot)?;
            backend.wake.wake();
            Ok(view)
        })
    }

    fn get_job(&self, job_id: String) -> ApiFuture<'_, Option<JobView>> {
        let backend = self.clone();
        Box::pin(async move {
            let persistence = backend.open_persistence()?;
            persistence
                .job_snapshot(&job_id)
                .map_err(persistence_error)?
                .map(|snapshot| backend.job_view(&persistence, snapshot))
                .transpose()
        })
    }

    fn list_jobs(&self) -> ApiFuture<'_, Vec<JobView>> {
        let backend = self.clone();
        Box::pin(async move {
            let persistence = backend.open_persistence()?;
            persistence
                .list_job_snapshots()
                .map_err(persistence_error)?
                .into_iter()
                .map(|snapshot| backend.job_view(&persistence, snapshot))
                .collect()
        })
    }

    fn list_job_events(
        &self,
        request: ListJobEventsRequest,
    ) -> ApiFuture<'_, Vec<crate::job_events::StoredJobEvent>> {
        let backend = self.clone();
        Box::pin(async move {
            backend
                .open_persistence()?
                .list_job_events(request.after_cursor.unwrap_or(0), request.limit)
                .map_err(persistence_error)
        })
    }

    fn get_settings(&self) -> ApiFuture<'_, ApiAppSettings> {
        let backend = self.clone();
        Box::pin(async move {
            let persistence = backend.open_persistence()?;
            ApiAppSettings::from_domain(backend.load_settings(&persistence)?)
        })
    }

    fn update_settings(&self, settings: ApiAppSettings) -> ApiFuture<'_, ApiAppSettings> {
        let backend = self.clone();
        Box::pin(async move {
            let settings = settings.validated()?;
            let persistence = backend.open_persistence()?;
            let domain = AppSettings {
                region: settings.region,
                market: settings.market,
                preferred_architectures: settings.preferred_architectures,
                preferred_languages: settings.preferred_languages,
                proxy_mode: settings.proxy_mode,
                proxy_host: settings.proxy_host,
                proxy_port: settings.proxy_port,
                proxy_credentials: settings.proxy_credentials,
                cache_enabled: settings.cache_enabled,
                cache_directory: backend.paths.cache_root.to_string_lossy().into_owned(),
                max_cache_bytes: settings.max_cache_bytes,
                retention_days: settings.retention_days,
                keep_installed_payloads: settings.keep_installed_payloads,
                max_concurrent_downloads: settings.max_concurrent_downloads,
                max_concurrent_update_scans: settings.max_concurrent_update_scans,
                theme: settings.theme,
                diagnostics_enabled: settings.diagnostics_enabled,
            };
            persistence
                .save_settings(&domain)
                .map_err(persistence_error)?;
            ApiAppSettings::from_domain(domain)
        })
    }

    fn clear_cache(&self) -> ApiFuture<'_, ()> {
        let backend = self.clone();
        Box::pin(async move { backend.clear_cache_safely() })
    }
}

fn merge_catalog_metadata(
    mut summary: CatalogProduct,
    details: Option<CatalogProduct>,
) -> CatalogProduct {
    let Some(details) = details else {
        summary.metadata_state = CatalogMetadataState::Partial;
        return summary;
    };
    summary.package_family_name = details.package_family_name.or(summary.package_family_name);
    summary.app_name = details.app_name.or(summary.app_name);
    summary.package_name = details.package_name.or(summary.package_name);
    summary.publisher = details.publisher.or(summary.publisher);
    summary.package_publisher = details.package_publisher.or(summary.package_publisher);
    summary.icon_url = details.icon_url.or(summary.icon_url);
    for format in details.package_formats {
        if !summary.package_formats.contains(&format) {
            summary.package_formats.push(format);
        }
    }
    for dependency in details.framework_dependencies {
        if !summary.framework_dependencies.contains(&dependency) {
            summary.framework_dependencies.push(dependency);
        }
    }
    summary.metadata_state = details.metadata_state;
    summary
}

impl ProductionApiBackend {
    pub fn diagnostics_enabled(&self) -> Result<bool, AppErrorDto> {
        let persistence = self.open_persistence()?;
        Ok(self.load_settings(&persistence)?.diagnostics_enabled)
    }

    fn clear_cache_safely(&self) -> Result<(), AppErrorDto> {
        let persistence = self.open_persistence()?;
        let has_active_job = persistence
            .list_job_snapshots()
            .map_err(persistence_error)?
            .into_iter()
            .any(|snapshot| {
                !matches!(
                    snapshot.job.stage,
                    JobStage::Completed | JobStage::Cancelled
                )
            });
        if has_active_job {
            return Err(runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Retry));
        }

        CacheManager::new(&self.paths.cache_root)
            .map_err(|_| runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Never))?;
        let canonical_root = self
            .paths
            .cache_root
            .canonicalize()
            .map_err(|_| runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Never))?;
        let entries = persistence.cache_entries().map_err(persistence_error)?;
        let mut files = HashSet::new();
        for entry in &entries {
            let path = PathBuf::from(&entry.path);
            validate_cache_file(&path, &self.paths.cache_root, &canonical_root)?;
            if path.exists() {
                files.insert(path);
            }
        }
        for directory in ["partial", "verified"] {
            for entry in std::fs::read_dir(self.paths.cache_root.join(directory))
                .map_err(|_| runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Retry))?
            {
                let path = entry
                    .map_err(|_| runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Retry))?
                    .path();
                validate_cache_file(&path, &self.paths.cache_root, &canonical_root)?;
                if !path.is_file() {
                    return Err(runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Never));
                }
                files.insert(path);
            }
        }

        for path in files {
            std::fs::remove_file(path)
                .map_err(|_| runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Retry))?;
        }
        for entry in entries {
            persistence
                .delete_cache_entry(&entry.cache_key)
                .map_err(persistence_error)?;
        }
        Ok(())
    }
}

struct UpdateAssociationJob {
    index: usize,
    installed: PackageInventoryRecord,
    association: Option<PackageAssociation>,
}

struct UpdateAssociationReady {
    index: usize,
    installed: PackageInventoryRecord,
    association: PackageAssociation,
    product: Option<CatalogProduct>,
    lookup_language: Option<String>,
}

enum UpdateAssociationOutcome {
    Ready(Box<UpdateAssociationReady>),
    Skipped {
        index: usize,
        skipped: ApiUpdateSkipped,
    },
}

impl UpdateAssociationOutcome {
    const fn index(&self) -> usize {
        match self {
            Self::Ready(ready) => ready.index,
            Self::Skipped { index, .. } => *index,
        }
    }
}

async fn associate_update_package(
    job: UpdateAssociationJob,
    settings: &AppSettings,
) -> UpdateAssociationOutcome {
    if let Some(association) = job.association {
        return UpdateAssociationOutcome::Ready(Box::new(UpdateAssociationReady {
            index: job.index,
            installed: job.installed,
            association,
            product: None,
            lookup_language: None,
        }));
    }
    let language = settings
        .preferred_languages
        .first()
        .cloned()
        .unwrap_or_else(|| "en-US".to_owned());
    let mut catalog =
        match StoreLibCatalogAdapter::production_for_locale(&settings.market, &language) {
            Ok(catalog) => catalog,
            Err(_) => {
                return skipped_association(
                    job.index,
                    job.installed.package_family_name,
                    ApiUpdateSkipReason::CatalogUnavailable,
                );
            }
        };
    let product = match catalog
        .lookup(
            CatalogIdentifier::PackageFamilyName,
            &job.installed.package_family_name,
        )
        .await
    {
        Ok(product) => product,
        Err(_) => {
            return skipped_association(
                job.index,
                job.installed.package_family_name,
                ApiUpdateSkipReason::MissingAssociation,
            );
        }
    };
    let identity_matches = product
        .package_name
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case(&job.installed.identity_name));
    let publisher_matches = product
        .package_publisher
        .as_deref()
        .is_some_and(|publisher| publisher.eq_ignore_ascii_case(&job.installed.publisher));
    let family_matches = product
        .package_family_name
        .as_deref()
        .is_some_and(|family| family.eq_ignore_ascii_case(&job.installed.package_family_name));
    if !identity_matches || !publisher_matches || !family_matches {
        return skipped_association(
            job.index,
            job.installed.package_family_name,
            ApiUpdateSkipReason::SourceIdentityMismatch,
        );
    }
    let association = PackageAssociation {
        package_family_name: job.installed.package_family_name.clone(),
        product_id: Some(product.product_id.clone()),
        content_id: None,
        identity_name: job.installed.identity_name.clone(),
        publisher: job.installed.publisher.clone(),
        confidence: AssociationConfidence::ExactPackageFamilyName,
        observed_at: unix_now(),
    };
    UpdateAssociationOutcome::Ready(Box::new(UpdateAssociationReady {
        index: job.index,
        installed: job.installed,
        association,
        product: Some(product),
        lookup_language: Some(language),
    }))
}

fn skipped_association(
    index: usize,
    package_family_name: String,
    reason: ApiUpdateSkipReason,
) -> UpdateAssociationOutcome {
    UpdateAssociationOutcome::Skipped {
        index,
        skipped: ApiUpdateSkipped {
            package_family_name,
            reason,
        },
    }
}

struct UpdateResolutionJob {
    index: usize,
    installed: PackageInventoryRecord,
    product_id: String,
    product: Option<ProductRecord>,
    market: String,
    language: String,
}

enum UpdateResolutionOutcome {
    Candidate {
        index: usize,
        candidate: ApiUpdateCandidate,
    },
    UpToDate {
        index: usize,
    },
    Skipped {
        index: usize,
        skipped: ApiUpdateSkipped,
    },
}

impl UpdateResolutionOutcome {
    const fn index(&self) -> usize {
        match self {
            Self::Candidate { index, .. }
            | Self::UpToDate { index }
            | Self::Skipped { index, .. } => *index,
        }
    }
}

async fn resolve_update_package(
    job: UpdateResolutionJob,
    host: &HostCapabilities,
    settings: &AppSettings,
) -> UpdateResolutionOutcome {
    let mut resolver =
        match StoreLibResolverAdapter::production_for_locale(&job.market, &job.language) {
            Ok(resolver) => resolver,
            Err(_) => {
                return skipped_resolution(&job, ApiUpdateSkipReason::CatalogUnavailable);
            }
        };
    let graph = match resolver.resolve(&job.product_id).await {
        Ok(graph) => graph,
        Err(_) => {
            return skipped_resolution(&job, ApiUpdateSkipReason::CatalogUnavailable);
        }
    };
    if validate_resolved_product(&graph, &job.product_id, &job.market).is_err() {
        return skipped_resolution(&job, ApiUpdateSkipReason::SourceIdentityMismatch);
    }
    let installed_version = PackageVersion::new(
        job.installed.version[0],
        job.installed.version[1],
        job.installed.version[2],
        job.installed.version[3],
    );
    let Some(installed_architecture) = parse_architecture(&job.installed.architecture) else {
        return skipped_resolution(&job, ApiUpdateSkipReason::SelectionRejected);
    };
    let latest_compatible_version =
        graph
            .packages
            .iter()
            .filter(|package| package.package_kind == PackageKind::Main)
            .filter(|package| package_incompatibility_reason(package, host).is_none())
            .filter(|package| {
                package.identity_name.as_deref().is_some_and(|identity| {
                    identity.eq_ignore_ascii_case(&job.installed.identity_name)
                }) && package.publisher.as_deref().is_none_or(|publisher| {
                    publisher.eq_ignore_ascii_case(&job.installed.publisher)
                })
            })
            .map(|package| package.version)
            .max();
    let Some(latest_compatible_version) = latest_compatible_version else {
        return skipped_resolution(&job, ApiUpdateSkipReason::SelectionRejected);
    };
    if latest_compatible_version <= installed_version {
        return UpdateResolutionOutcome::UpToDate { index: job.index };
    }
    let installed_selection = InstalledPackage {
        identity_name: job.installed.identity_name.clone(),
        publisher: Some(job.installed.publisher.clone()),
        version: installed_version,
        architecture: installed_architecture,
    };
    let preferences = SelectionPreferences {
        market: job.market.clone(),
        preferred_architectures: settings.preferred_architectures.clone(),
        preferred_languages: prioritized_languages(&job.language, settings),
        mode: SelectionMode::Update,
    };
    let selection = match select_packages(&graph, host, &preferences, &[installed_selection]) {
        Ok(selection) => selection,
        Err(_) => return skipped_resolution(&job, ApiUpdateSkipReason::SelectionRejected),
    };
    let Some(main) = selection
        .packages
        .iter()
        .find(|package| package.package_kind == PackageKind::Main)
    else {
        return skipped_resolution(&job, ApiUpdateSkipReason::SelectionRejected);
    };
    let deployment_scope = match derive_update_scope(&job.installed) {
        DeploymentScope::CurrentUser => ApiDeploymentScope::CurrentUser,
        DeploymentScope::AllUsers => ApiDeploymentScope::AllUsers,
    };
    UpdateResolutionOutcome::Candidate {
        index: job.index,
        candidate: ApiUpdateCandidate {
            app_name: job
                .product
                .as_ref()
                .and_then(|product| product.app_name.clone())
                .unwrap_or_else(|| job.installed.app_name.clone()),
            package_name: job.installed.package_name.clone(),
            publisher: job
                .product
                .as_ref()
                .and_then(|product| product.publisher.clone())
                .unwrap_or_else(|| job.installed.publisher.clone()),
            package_family_name: job.installed.package_family_name.clone(),
            current_version: installed_version.to_string(),
            available_version: main.version.to_string(),
            product_id: Some(job.product_id.clone()),
            deployment_scope,
        },
    }
}

fn skipped_resolution(
    job: &UpdateResolutionJob,
    reason: ApiUpdateSkipReason,
) -> UpdateResolutionOutcome {
    UpdateResolutionOutcome::Skipped {
        index: job.index,
        skipped: ApiUpdateSkipped {
            package_family_name: job.installed.package_family_name.clone(),
            reason,
        },
    }
}

fn persist_catalog_product(
    persistence: &Persistence,
    product: &CatalogProduct,
    market: &str,
    language: &str,
) -> Result<(), AppErrorDto> {
    ApiCatalogProduct::from_domain(product.clone())?;
    persistence
        .upsert_product(&ProductRecord {
            product_id: product.product_id.clone(),
            package_family_name: product.package_family_name.clone(),
            app_name: product.app_name.clone(),
            publisher: product.publisher.clone(),
            market: market.to_owned(),
            languages: vec![language.to_owned()],
            updated_at: unix_now(),
        })
        .map_err(persistence_error)
}

fn validate_resolved_product(
    graph: &crate::resolver::PackageGraph,
    product_id: &str,
    market: &str,
) -> Result<(), AppErrorDto> {
    if graph.product_id.as_deref() != Some(product_id)
        || graph
            .market
            .as_deref()
            .is_none_or(|resolved| !resolved.eq_ignore_ascii_case(market))
    {
        return Err(runtime_error(
            ErrorCode::SourceIdentityMismatch,
            RetryAdvice::ReResolve,
        ));
    }
    Ok(())
}

fn prioritized_languages(requested: &str, settings: &AppSettings) -> Vec<String> {
    let mut languages = vec![requested.to_owned()];
    for language in &settings.preferred_languages {
        if !languages
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(language))
        {
            languages.push(language.clone());
        }
    }
    languages
}

fn default_settings(cache_root: &Path) -> AppSettings {
    AppSettings {
        region: "US".to_owned(),
        market: "US".to_owned(),
        preferred_architectures: default_architectures(),
        preferred_languages: vec!["en-US".to_owned()],
        proxy_mode: ProxyMode::Disabled,
        proxy_host: None,
        proxy_port: None,
        proxy_credentials: ProxyCredentialPolicy::PromptEveryTime,
        cache_enabled: true,
        cache_directory: cache_root.to_string_lossy().into_owned(),
        max_cache_bytes: 10 * 1024 * 1024 * 1024,
        retention_days: 30,
        keep_installed_payloads: false,
        max_concurrent_downloads: 2,
        max_concurrent_update_scans: 16,
        theme: ThemeMode::System,
        diagnostics_enabled: false,
    }
}

fn default_architectures() -> Vec<Architecture> {
    system_host_capabilities()
        .map(|capabilities| capabilities.compatible_architectures)
        .unwrap_or_else(|_| vec![Architecture::Neutral])
}

fn ensure_database_parent(path: &Path) -> Result<(), AppErrorDto> {
    let parent = path
        .parent()
        .ok_or_else(|| runtime_error(ErrorCode::DeploymentDenied, RetryAdvice::Never))?;
    std::fs::create_dir_all(parent)
        .map_err(|_| runtime_error(ErrorCode::DeploymentFailed, RetryAdvice::Retry))
}

fn validate_cache_file(path: &Path, root: &Path, canonical_root: &Path) -> Result<(), AppErrorDto> {
    if !path_is_within(path, root) {
        return Err(runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Never));
    }
    if !path.exists() {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Retry))?;
    if metadata.file_type().is_symlink() || has_reparse_component(path, root)? {
        return Err(runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Never));
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Retry))?;
    if !path_is_within(&canonical, canonical_root) {
        return Err(runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Never));
    }
    Ok(())
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let path = normalized_path(path);
    let root = normalized_path(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| component == std::path::Component::ParentDir)
}

fn normalized_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn has_reparse_component(path: &Path, root: &Path) -> Result<bool, AppErrorDto> {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    for ancestor in path.ancestors() {
        if !path_is_within(ancestor, root) {
            break;
        }
        let metadata = std::fs::symlink_metadata(ancestor)
            .map_err(|_| runtime_error(ErrorCode::DownloadFailed, RetryAdvice::Retry))?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Ok(true);
        }
        if normalized_path(ancestor) == normalized_path(root) {
            break;
        }
    }
    Ok(false)
}

#[cfg(not(windows))]
fn has_reparse_component(_path: &Path, _root: &Path) -> Result<bool, AppErrorDto> {
    Ok(false)
}

fn persistence_error(_error: PersistenceError) -> AppErrorDto {
    runtime_error(ErrorCode::DeploymentFailed, RetryAdvice::Retry)
}

fn worker_error(error: WorkerError) -> AppErrorDto {
    match error {
        WorkerError::Persistence(error) => persistence_error(error),
        WorkerError::InvalidConfiguration => {
            runtime_error(ErrorCode::DeploymentFailed, RetryAdvice::Never)
        }
    }
}

fn runtime_error(code: ErrorCode, retry: RetryAdvice) -> AppErrorDto {
    AppErrorDto::new(code, retry)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

#[cfg(windows)]
fn system_host_capabilities() -> Result<HostCapabilities, AppErrorDto> {
    let native_architecture = native_windows_architecture()?;
    let os_version = windows_version()?;
    if os_version < PackageVersion::new(10, 0, 10240, 0) {
        return Err(runtime_error(
            ErrorCode::UnsupportedPackageType,
            RetryAdvice::Never,
        ));
    }
    Ok(HostCapabilities {
        os_version,
        native_architecture,
        compatible_architectures: compatible_architectures(native_architecture),
        supported_formats: vec![
            PackageFormat::Msix,
            PackageFormat::Appx,
            PackageFormat::MsixBundle,
            PackageFormat::AppxBundle,
        ],
    })
}

pub fn compatible_architectures(native: Architecture) -> Vec<Architecture> {
    match native {
        Architecture::X64 => vec![Architecture::X64, Architecture::X86, Architecture::Neutral],
        Architecture::Arm64 => {
            vec![
                Architecture::Arm64,
                Architecture::X86,
                Architecture::Neutral,
            ]
        }
        Architecture::Arm => vec![Architecture::Arm, Architecture::Neutral],
        Architecture::X86 => vec![Architecture::X86, Architecture::Neutral],
        Architecture::Neutral => vec![Architecture::Neutral],
    }
}

fn parse_architecture(value: &str) -> Option<Architecture> {
    match value.to_ascii_lowercase().as_str() {
        "x64" => Some(Architecture::X64),
        "arm64" => Some(Architecture::Arm64),
        "arm" => Some(Architecture::Arm),
        "x86" | "x86-on-arm64" => Some(Architecture::X86),
        "neutral" => Some(Architecture::Neutral),
        _ => None,
    }
}

pub fn compatible_main_architectures(
    graph: &PackageGraph,
    host: &HostCapabilities,
) -> Result<Vec<Architecture>, AppErrorDto> {
    let supported = host
        .compatible_architectures
        .iter()
        .copied()
        .filter(|architecture| {
            graph.packages.iter().any(|package| {
                package.package_kind == PackageKind::Main
                    && package.architecture == *architecture
                    && package_incompatibility_reason(package, host).is_none()
            })
        })
        .collect::<Vec<_>>();
    if supported.is_empty() {
        Err(runtime_error(
            ErrorCode::NoCompatiblePackage,
            RetryAdvice::Never,
        ))
    } else {
        Ok(supported)
    }
}

#[cfg(not(windows))]
fn system_host_capabilities() -> Result<HostCapabilities, AppErrorDto> {
    Err(runtime_error(
        ErrorCode::UnsupportedPackageType,
        RetryAdvice::Never,
    ))
}

#[cfg(windows)]
fn native_windows_architecture() -> Result<Architecture, AppErrorDto> {
    use std::{ffi::c_void, mem::MaybeUninit};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ProcessorPair {
        architecture: u16,
        reserved: u16,
    }

    #[repr(C)]
    union ProcessorInfo {
        oem_id: u32,
        pair: ProcessorPair,
    }

    #[repr(C)]
    struct SystemInfo {
        processor: ProcessorInfo,
        page_size: u32,
        minimum_application_address: *mut c_void,
        maximum_application_address: *mut c_void,
        active_processor_mask: usize,
        number_of_processors: u32,
        processor_type: u32,
        allocation_granularity: u32,
        processor_level: u16,
        processor_revision: u16,
    }

    #[link(name = "kernel32")]
    extern "system" {
        #[link_name = "GetNativeSystemInfo"]
        fn get_native_system_info(system_info: *mut SystemInfo);
    }

    let mut system_info = MaybeUninit::<SystemInfo>::zeroed();
    unsafe { get_native_system_info(system_info.as_mut_ptr()) };
    let architecture = unsafe { system_info.assume_init().processor.pair.architecture };
    match architecture {
        0 => Ok(Architecture::X86),
        5 => Ok(Architecture::Arm),
        9 => Ok(Architecture::X64),
        12 => Ok(Architecture::Arm64),
        _ => Err(runtime_error(
            ErrorCode::UnsupportedPackageType,
            RetryAdvice::Never,
        )),
    }
}

#[cfg(windows)]
fn windows_version() -> Result<PackageVersion, AppErrorDto> {
    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform_id: u32,
        service_pack: [u16; 128],
    }

    #[link(name = "ntdll")]
    extern "system" {
        #[link_name = "RtlGetVersion"]
        fn rtl_get_version(version: *mut OsVersionInfo) -> i32;
    }

    let mut version = OsVersionInfo {
        size: std::mem::size_of::<OsVersionInfo>() as u32,
        major: 0,
        minor: 0,
        build: 0,
        platform_id: 0,
        service_pack: [0; 128],
    };
    if unsafe { rtl_get_version(&mut version) } < 0 {
        return Err(runtime_error(
            ErrorCode::UnsupportedPackageType,
            RetryAdvice::Never,
        ));
    }
    let major = u16::try_from(version.major)
        .map_err(|_| runtime_error(ErrorCode::UnsupportedPackageType, RetryAdvice::Never))?;
    let minor = u16::try_from(version.minor)
        .map_err(|_| runtime_error(ErrorCode::UnsupportedPackageType, RetryAdvice::Never))?;
    let build = u16::try_from(version.build)
        .map_err(|_| runtime_error(ErrorCode::UnsupportedPackageType, RetryAdvice::Never))?;
    Ok(PackageVersion::new(major, minor, build, 0))
}

pub type WorkerWaitFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

pub fn wait_for_worker_wake(wake: &WorkerWake) -> WorkerWaitFuture<'_> {
    Box::pin(wake.notified())
}

async fn collect_bounded<T, R, F, Fut>(items: Vec<T>, concurrency: usize, operation: F) -> Vec<R>
where
    F: FnMut(T) -> Fut,
    Fut: Future<Output = R>,
{
    stream::iter(items)
        .map(operation)
        .buffer_unordered(concurrency.clamp(1, UPDATE_SCAN_CONCURRENCY_LIMIT))
        .collect()
        .await
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use super::{collect_bounded, UPDATE_SCAN_CONCURRENCY_LIMIT};

    #[tokio::test]
    async fn update_network_work_is_bounded_and_concurrent() {
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let results = collect_bounded((0..128).collect(), UPDATE_SCAN_CONCURRENCY_LIMIT, |value| {
            let active = Arc::clone(&active);
            let maximum = Arc::clone(&maximum);
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(current, Ordering::SeqCst);
                tokio::task::yield_now().await;
                active.fetch_sub(1, Ordering::SeqCst);
                value
            }
        })
        .await;

        assert_eq!(results.len(), 128);
        assert_eq!(
            maximum.load(Ordering::SeqCst),
            UPDATE_SCAN_CONCURRENCY_LIMIT
        );
    }
}
