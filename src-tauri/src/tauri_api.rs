use std::{future::Future, pin::Pin};

use serde::{Deserialize, Serialize};

use crate::{
    catalog::{normalize_catalog_icon_url, CatalogProduct},
    deployment::DeploymentScope,
    domain::{AppSettings, Architecture, ProxyCredentialPolicy, ProxyMode, ThemeMode},
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    inventory::{
        InventorySnapshot, InventorySource, PackageInventoryRecord,
        PackageKind as InventoryPackageKind,
    },
    job_events::{JobControl, StoredJobEvent},
    jobs::{JobSnapshot, JobStage},
};

pub const JOB_CHANGED_EVENT: &str = "job://changed";

pub type ApiFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, AppErrorDto>> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiDeploymentScope {
    CurrentUser,
    AllUsers,
}

impl From<ApiDeploymentScope> for DeploymentScope {
    fn from(scope: ApiDeploymentScope) -> Self {
        match scope {
            ApiDeploymentScope::CurrentUser => Self::CurrentUser,
            ApiDeploymentScope::AllUsers => Self::AllUsers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchRequest {
    pub query: String,
    pub market: String,
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetailsRequest {
    pub product_id: String,
    pub market: String,
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartJobRequest {
    pub product_id: String,
    pub market: String,
    pub language: String,
    pub scope: ApiDeploymentScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartJobSpec {
    pub product_id: String,
    pub market: String,
    pub language: String,
    pub scope: DeploymentScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobControlRequest {
    pub job_id: String,
    pub command_id: String,
    pub expected_sequence: u64,
    pub control: JobControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListJobEventsRequest {
    pub after_cursor: Option<u64>,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDetailsSource {
    pub product: CatalogProduct,
    pub supported_architectures: Vec<Architecture>,
    pub selection_preview: crate::applicability::SelectionPreview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobView {
    pub snapshot: JobSnapshot,
    pub title: Option<String>,
}

pub trait ApiBackend: Send + Sync {
    fn search_apps(&self, request: SearchRequest) -> ApiFuture<'_, Vec<CatalogProduct>>;

    fn get_app_details(&self, request: DetailsRequest) -> ApiFuture<'_, AppDetailsSource>;

    fn scan_installed_packages(&self, scope: DeploymentScope) -> ApiFuture<'_, InventorySnapshot>;

    fn scan_updates(&self) -> ApiFuture<'_, ApiUpdateScanResult>;

    fn start_install(&self, request: StartJobSpec) -> ApiFuture<'_, JobView>;

    fn start_update(&self, request: StartJobSpec) -> ApiFuture<'_, JobView>;

    fn request_job_control(&self, request: JobControlRequest) -> ApiFuture<'_, JobView>;

    fn get_job(&self, job_id: String) -> ApiFuture<'_, Option<JobView>>;

    fn list_jobs(&self) -> ApiFuture<'_, Vec<JobView>>;

    fn list_job_events(&self, request: ListJobEventsRequest) -> ApiFuture<'_, Vec<StoredJobEvent>>;

    fn get_settings(&self) -> ApiFuture<'_, ApiAppSettings>;

    fn update_settings(&self, settings: ApiAppSettings) -> ApiFuture<'_, ApiAppSettings>;

    fn clear_cache(&self) -> ApiFuture<'_, ()>;
}

pub struct TauriApi<B> {
    backend: B,
}

impl<B> TauriApi<B>
where
    B: ApiBackend,
{
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }

    pub async fn search_apps(
        &self,
        mut request: SearchRequest,
    ) -> Result<Vec<ApiCatalogProduct>, AppErrorDto> {
        request.query = request.query.trim().to_owned();
        request.market.make_ascii_uppercase();
        if !safe_text(&request.query, 256)
            || !safe_market(&request.market)
            || !safe_language(&request.language)
        {
            return Err(boundary_error(ErrorCode::CatalogUnavailable));
        }
        let products = self
            .backend
            .search_apps(request)
            .await
            .map_err(sanitize_error)?;
        if products.len() > 500 {
            return Err(boundary_error(ErrorCode::CatalogUnavailable));
        }
        products
            .into_iter()
            .map(ApiCatalogProduct::from_domain)
            .collect()
    }

    pub async fn get_app_details(
        &self,
        mut request: DetailsRequest,
    ) -> Result<ApiAppDetails, AppErrorDto> {
        normalize_details_request(&mut request)?;
        let market = request.market.clone();
        let language = request.language.clone();
        let source = self
            .backend
            .get_app_details(request)
            .await
            .map_err(sanitize_error)?;
        ApiAppDetails::from_catalog(
            source.product,
            market,
            language,
            source.supported_architectures,
            source.selection_preview,
        )
    }

    pub async fn scan_installed_packages(
        &self,
        scope: ApiDeploymentScope,
    ) -> Result<ApiInventorySnapshot, AppErrorDto> {
        let snapshot = self
            .backend
            .scan_installed_packages(scope.into())
            .await
            .map_err(sanitize_error)?;
        ApiInventorySnapshot::from_domain(snapshot)
    }

    pub async fn scan_updates(&self) -> Result<ApiUpdateScanResult, AppErrorDto> {
        let mut result = self.backend.scan_updates().await.map_err(sanitize_error)?;
        if result.candidates.len() > 1000 || result.skipped.len() > 1000 {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        result.candidates = result
            .candidates
            .into_iter()
            .map(ApiUpdateCandidate::validated)
            .collect::<Result<Vec<_>, _>>()?;
        for skipped in &result.skipped {
            if !safe_package_identity(&skipped.package_family_name) {
                return Err(boundary_error(ErrorCode::DeploymentFailed));
            }
        }
        Ok(result)
    }

    pub async fn start_install(
        &self,
        request: StartJobRequest,
    ) -> Result<ApiJobSnapshot, AppErrorDto> {
        let request = normalize_start_request(request)?;
        let view = self
            .backend
            .start_install(request)
            .await
            .map_err(sanitize_error)?;
        map_job_view(view)
    }

    pub async fn start_update(
        &self,
        request: StartJobRequest,
    ) -> Result<ApiJobSnapshot, AppErrorDto> {
        let request = normalize_start_request(request)?;
        let view = self
            .backend
            .start_update(request)
            .await
            .map_err(sanitize_error)?;
        map_job_view(view)
    }

    pub async fn request_job_control(
        &self,
        request: JobControlRequest,
    ) -> Result<ApiJobSnapshot, AppErrorDto> {
        if !safe_identifier(&request.job_id)
            || !safe_identifier(&request.command_id)
            || request.expected_sequence == 0
        {
            return Err(boundary_error(ErrorCode::DeploymentDenied));
        }
        let view = self
            .backend
            .request_job_control(request)
            .await
            .map_err(sanitize_error)?;
        map_job_view(view)
    }

    pub async fn get_job(&self, job_id: String) -> Result<Option<ApiJobSnapshot>, AppErrorDto> {
        if !safe_identifier(&job_id) {
            return Err(boundary_error(ErrorCode::DeploymentDenied));
        }
        self.backend
            .get_job(job_id)
            .await
            .map_err(sanitize_error)?
            .map(map_job_view)
            .transpose()
    }

    pub async fn list_jobs(&self) -> Result<Vec<ApiJobSnapshot>, AppErrorDto> {
        let jobs = self.backend.list_jobs().await.map_err(sanitize_error)?;
        if jobs.len() > 1000 {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        jobs.into_iter().map(map_job_view).collect()
    }

    pub async fn list_job_events(
        &self,
        request: ListJobEventsRequest,
    ) -> Result<ApiJobEventPage, AppErrorDto> {
        if !(1..=100).contains(&request.limit) {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        let validation_request = request.clone();
        let stored = self
            .backend
            .list_job_events(request)
            .await
            .map_err(sanitize_error)?;
        if stored.len() > validation_request.limit {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        let events = stored
            .into_iter()
            .map(|stored| {
                if stored.sequence != stored.snapshot.sequence
                    || stored.job_id != stored.snapshot.job.job_id
                {
                    return Err(boundary_error(ErrorCode::DeploymentFailed));
                }
                let snapshot = ApiJobSnapshot::from_domain(stored.snapshot, None)?;
                Ok(ApiStoredJobEvent {
                    cursor: stored.cursor,
                    job_id: stored.job_id,
                    sequence: stored.sequence,
                    snapshot,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = events
            .last()
            .map(|event| event.cursor)
            .or(validation_request.after_cursor);
        ApiJobEventPage {
            events,
            next_cursor,
        }
        .validated(&validation_request)
    }

    pub async fn get_settings(&self) -> Result<ApiAppSettings, AppErrorDto> {
        self.backend
            .get_settings()
            .await
            .map_err(sanitize_error)?
            .validated()
    }

    pub async fn update_settings(
        &self,
        settings: ApiAppSettings,
    ) -> Result<ApiAppSettings, AppErrorDto> {
        let settings = settings.validated()?;
        self.backend
            .update_settings(settings)
            .await
            .map_err(sanitize_error)?
            .validated()
    }

    pub async fn clear_cache(&self) -> Result<(), AppErrorDto> {
        self.backend.clear_cache().await.map_err(sanitize_error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiAppSettings {
    pub region: String,
    pub market: String,
    pub preferred_architectures: Vec<Architecture>,
    pub preferred_languages: Vec<String>,
    pub proxy_mode: ProxyMode,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
    pub proxy_credentials: ProxyCredentialPolicy,
    pub cache_enabled: bool,
    pub max_cache_bytes: u64,
    pub retention_days: u32,
    pub keep_installed_payloads: bool,
    pub max_concurrent_downloads: u32,
    pub max_concurrent_update_scans: u32,
    pub theme: ThemeMode,
    pub diagnostics_enabled: bool,
}

impl ApiAppSettings {
    pub fn from_domain(settings: AppSettings) -> Result<Self, AppErrorDto> {
        Self {
            region: settings.region,
            market: settings.market,
            preferred_architectures: settings.preferred_architectures,
            preferred_languages: settings.preferred_languages,
            proxy_mode: settings.proxy_mode,
            proxy_host: settings.proxy_host,
            proxy_port: settings.proxy_port,
            proxy_credentials: settings.proxy_credentials,
            cache_enabled: settings.cache_enabled,
            max_cache_bytes: settings.max_cache_bytes,
            retention_days: settings.retention_days,
            keep_installed_payloads: settings.keep_installed_payloads,
            max_concurrent_downloads: settings.max_concurrent_downloads,
            max_concurrent_update_scans: settings.max_concurrent_update_scans,
            theme: settings.theme,
            diagnostics_enabled: settings.diagnostics_enabled,
        }
        .validated()
    }

    pub fn validated(self) -> Result<Self, AppErrorDto> {
        let proxy_valid = match self.proxy_mode {
            ProxyMode::Disabled | ProxyMode::System => {
                self.proxy_host.is_none() && self.proxy_port.is_none()
            }
            ProxyMode::Http | ProxyMode::Https | ProxyMode::Socks5 => {
                self.proxy_host.as_deref().is_some_and(safe_proxy_host)
                    && self.proxy_port.is_some_and(|port| port > 0)
            }
        };
        if !safe_market(&self.region)
            || !safe_market(&self.market)
            || self.preferred_architectures.is_empty()
            || self.preferred_architectures.len() > 16
            || self.preferred_languages.len() > 32
            || self
                .preferred_languages
                .iter()
                .any(|language| !safe_language(language))
            || has_case_insensitive_duplicates(&self.preferred_languages)
            || !proxy_valid
            || self.max_cache_bytes == 0
            || !(1..=365).contains(&self.retention_days)
            || !(1..=8).contains(&self.max_concurrent_downloads)
            || !(1..=64).contains(&self.max_concurrent_update_scans)
        {
            return Err(boundary_error(ErrorCode::DownloadFailed));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiInventorySource {
    CurrentUser,
    AllUsersElevated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiInventoryPackageKind {
    Main,
    Framework,
    Resource,
    Optional,
    Bundle,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiInventoryWarning {
    PartialInventory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiPackageInventoryRecord {
    pub app_name: String,
    pub package_name: String,
    pub identity_name: String,
    pub publisher: String,
    pub package_family_name: String,
    pub package_full_name: String,
    pub version: [u16; 4],
    pub architecture: String,
    pub package_kind: ApiInventoryPackageKind,
    pub installed_for_current_user: bool,
    pub has_other_users: bool,
    pub provisioned_for_future_users: bool,
}

impl ApiPackageInventoryRecord {
    fn from_domain(record: PackageInventoryRecord) -> Result<Self, AppErrorDto> {
        if !safe_text(&record.app_name, 512)
            || !safe_package_identity(&record.package_name)
            || !safe_package_identity(&record.identity_name)
            || !safe_text(&record.publisher, 512)
            || !safe_package_identity(&record.package_family_name)
            || !safe_package_identity(&record.package_full_name)
            || !safe_identifier(&record.architecture)
        {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        Ok(Self {
            app_name: record.app_name,
            package_name: record.package_name,
            identity_name: record.identity_name,
            publisher: record.publisher,
            package_family_name: record.package_family_name,
            package_full_name: record.package_full_name,
            version: record.version,
            architecture: record.architecture,
            package_kind: match record.package_kind {
                InventoryPackageKind::Main => ApiInventoryPackageKind::Main,
                InventoryPackageKind::Framework => ApiInventoryPackageKind::Framework,
                InventoryPackageKind::Resource => ApiInventoryPackageKind::Resource,
                InventoryPackageKind::Optional => ApiInventoryPackageKind::Optional,
                InventoryPackageKind::Bundle => ApiInventoryPackageKind::Bundle,
                InventoryPackageKind::Unknown => ApiInventoryPackageKind::Unknown,
            },
            installed_for_current_user: record.installed_for_current_user,
            has_other_users: record.has_other_users,
            provisioned_for_future_users: record.provisioned_for_future_users,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiInventorySnapshot {
    pub source: ApiInventorySource,
    pub captured_at: String,
    pub os_build: String,
    pub complete: bool,
    pub records: Vec<ApiPackageInventoryRecord>,
    pub warnings: Vec<ApiInventoryWarning>,
}

impl ApiInventorySnapshot {
    pub fn from_domain(snapshot: InventorySnapshot) -> Result<Self, AppErrorDto> {
        if !safe_text(&snapshot.captured_at, 64) || !safe_text(&snapshot.os_build, 128) {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        let has_warnings = !snapshot.complete || !snapshot.warnings.is_empty();
        let records = snapshot
            .records
            .into_iter()
            .map(ApiPackageInventoryRecord::from_domain)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            source: match snapshot.source {
                InventorySource::CurrentUser => ApiInventorySource::CurrentUser,
                InventorySource::AllUsersElevated => ApiInventorySource::AllUsersElevated,
            },
            captured_at: snapshot.captured_at,
            os_build: snapshot.os_build,
            complete: snapshot.complete,
            records,
            warnings: has_warnings
                .then_some(ApiInventoryWarning::PartialInventory)
                .into_iter()
                .collect(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCatalogProduct {
    pub product_id: String,
    pub package_family_name: Option<String>,
    pub app_name: String,
    pub package_name: Option<String>,
    pub publisher: Option<String>,
    pub icon_url: Option<String>,
    pub metadata_state: crate::catalog::CatalogMetadataState,
    pub package_formats: Vec<String>,
    pub framework_dependencies: Vec<String>,
}

impl ApiCatalogProduct {
    pub fn from_domain(product: CatalogProduct) -> Result<Self, AppErrorDto> {
        let app_name = product
            .app_name
            .unwrap_or_else(|| product.product_id.clone());
        if !safe_identifier(&product.product_id)
            || product
                .package_family_name
                .as_deref()
                .is_some_and(|value| !safe_identifier(value))
            || !safe_text(&app_name, 512)
            || product
                .package_name
                .as_deref()
                .is_some_and(|value| !safe_identifier(value))
            || product
                .publisher
                .as_deref()
                .is_some_and(|value| !safe_text(value, 512))
            || product
                .package_publisher
                .as_deref()
                .is_some_and(|value| !safe_text(value, 512))
            || product
                .icon_url
                .as_deref()
                .is_some_and(|value| normalize_catalog_icon_url(value).as_deref() != Ok(value))
            || product.package_formats.len() > 32
            || product
                .package_formats
                .iter()
                .any(|value| !safe_identifier(value))
            || product.framework_dependencies.len() > 128
            || product
                .framework_dependencies
                .iter()
                .any(|value| !safe_identifier(value))
        {
            return Err(boundary_error(ErrorCode::CatalogUnavailable));
        }
        Ok(Self {
            product_id: product.product_id,
            package_family_name: product.package_family_name,
            app_name,
            package_name: product.package_name,
            publisher: product.publisher,
            icon_url: product.icon_url,
            metadata_state: product.metadata_state,
            package_formats: product.package_formats,
            framework_dependencies: product.framework_dependencies,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiAppDetails {
    pub product_id: String,
    pub package_family_name: Option<String>,
    pub app_name: String,
    pub package_name: Option<String>,
    pub publisher: Option<String>,
    pub icon_url: Option<String>,
    pub metadata_state: crate::catalog::CatalogMetadataState,
    pub package_formats: Vec<String>,
    pub framework_dependencies: Vec<String>,
    pub market: String,
    pub language: String,
    pub supported_architectures: Vec<Architecture>,
    pub selection_preview: crate::applicability::SelectionPreview,
}

impl ApiAppDetails {
    pub fn from_catalog(
        product: CatalogProduct,
        market: String,
        language: String,
        supported_architectures: Vec<Architecture>,
        selection_preview: crate::applicability::SelectionPreview,
    ) -> Result<Self, AppErrorDto> {
        if !safe_market(&market) || !safe_language(&language) || supported_architectures.len() > 16
        {
            return Err(boundary_error(ErrorCode::CatalogUnavailable));
        }
        let product = ApiCatalogProduct::from_domain(product)?;
        Ok(Self {
            product_id: product.product_id,
            package_family_name: product.package_family_name,
            app_name: product.app_name,
            package_name: product.package_name,
            publisher: product.publisher,
            icon_url: product.icon_url,
            metadata_state: product.metadata_state,
            package_formats: product.package_formats,
            framework_dependencies: product.framework_dependencies,
            market,
            language,
            supported_architectures,
            selection_preview,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiUpdateCandidate {
    pub app_name: String,
    pub package_name: String,
    pub publisher: String,
    pub package_family_name: String,
    pub current_version: String,
    pub available_version: String,
    pub product_id: Option<String>,
    pub deployment_scope: ApiDeploymentScope,
}

impl ApiUpdateCandidate {
    fn validated(self) -> Result<Self, AppErrorDto> {
        if !safe_text(&self.app_name, 512)
            || !safe_package_identity(&self.package_name)
            || !safe_text(&self.publisher, 512)
            || !safe_package_identity(&self.package_family_name)
            || !safe_identifier(&self.current_version)
            || !safe_identifier(&self.available_version)
            || self
                .product_id
                .as_deref()
                .is_some_and(|value| !safe_identifier(value))
        {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiUpdateSkipReason {
    MissingAssociation,
    SourceIdentityMismatch,
    CatalogUnavailable,
    SelectionRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiUpdateSkipped {
    pub package_family_name: String,
    pub reason: ApiUpdateSkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiUpdateScanResult {
    pub scanned_main_packages: usize,
    pub associated_packages: usize,
    pub candidates: Vec<ApiUpdateCandidate>,
    pub skipped: Vec<ApiUpdateSkipped>,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiJobSnapshot {
    pub job_id: String,
    pub sequence: u64,
    pub product_id: String,
    pub package_family_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub stage: JobStage,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub version: Option<String>,
    pub architecture: Option<Architecture>,
    pub language: Option<String>,
    pub allowed_controls: Vec<JobControl>,
    pub error: Option<AppErrorDto>,
    pub updated_at: i64,
}

impl ApiJobSnapshot {
    pub fn from_domain(snapshot: JobSnapshot, title: Option<String>) -> Result<Self, AppErrorDto> {
        let job = snapshot.job;
        if !safe_identifier(&job.job_id)
            || !safe_identifier(&job.product_id)
            || job
                .package_family_name
                .as_deref()
                .is_some_and(|value| !safe_identifier(value))
            || title.as_deref().is_some_and(|value| !safe_text(value, 512))
            || job
                .version
                .as_deref()
                .is_some_and(|value| !safe_identifier(value))
            || job
                .language
                .as_deref()
                .is_some_and(|value| !safe_language(value))
        {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        Self {
            job_id: job.job_id,
            sequence: snapshot.sequence,
            product_id: job.product_id,
            package_family_name: job.package_family_name,
            title,
            stage: job.stage,
            bytes_done: job.bytes_done,
            bytes_total: job.bytes_total,
            version: job.version,
            architecture: job.architecture,
            language: job.language,
            allowed_controls: allowed_controls(job.stage),
            error: job.error.map(sanitize_error),
            updated_at: job.updated_at,
        }
        .validated()
    }

    fn validated(mut self) -> Result<Self, AppErrorDto> {
        self.validate()?;
        self.error = self.error.map(sanitize_error);
        Ok(self)
    }

    fn validate(&self) -> Result<(), AppErrorDto> {
        if !safe_identifier(&self.job_id)
            || self.sequence == 0
            || !safe_identifier(&self.product_id)
            || self
                .package_family_name
                .as_deref()
                .is_some_and(|value| !safe_package_identity(value))
            || self
                .title
                .as_deref()
                .is_some_and(|value| !safe_text(value, 512))
            || self
                .version
                .as_deref()
                .is_some_and(|value| !safe_identifier(value))
            || self
                .language
                .as_deref()
                .is_some_and(|value| !safe_language(value))
            || self
                .bytes_total
                .is_some_and(|total| self.bytes_done > total)
            || self.allowed_controls != allowed_controls(self.stage)
            || self
                .error
                .as_ref()
                .and_then(|error| error.job_id.as_deref())
                .is_some_and(|job_id| job_id != self.job_id)
        {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobChangedHint {
    pub job_id: String,
    pub sequence: u64,
    pub updated_at: i64,
}

impl From<&ApiJobSnapshot> for JobChangedHint {
    fn from(snapshot: &ApiJobSnapshot) -> Self {
        Self {
            job_id: snapshot.job_id.clone(),
            sequence: snapshot.sequence,
            updated_at: snapshot.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiStoredJobEvent {
    pub cursor: u64,
    pub job_id: String,
    pub sequence: u64,
    pub snapshot: ApiJobSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiJobEventPage {
    pub events: Vec<ApiStoredJobEvent>,
    pub next_cursor: Option<u64>,
}

impl ApiJobEventPage {
    fn validated(self, request: &ListJobEventsRequest) -> Result<Self, AppErrorDto> {
        if self.events.len() > request.limit {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        let mut prior = request.after_cursor.unwrap_or(0);
        for event in &self.events {
            if event.cursor <= prior
                || event.sequence == 0
                || event.job_id != event.snapshot.job_id
                || event.sequence != event.snapshot.sequence
                || event.snapshot.validate().is_err()
            {
                return Err(boundary_error(ErrorCode::DeploymentFailed));
            }
            prior = event.cursor;
        }
        let expected_next = self
            .events
            .last()
            .map(|event| event.cursor)
            .or(request.after_cursor);
        if self.next_cursor != expected_next {
            return Err(boundary_error(ErrorCode::DeploymentFailed));
        }
        Ok(self)
    }
}

fn allowed_controls(stage: JobStage) -> Vec<JobControl> {
    match stage {
        JobStage::Downloading => vec![JobControl::Pause, JobControl::Cancel],
        JobStage::Paused | JobStage::Interrupted | JobStage::Failed => {
            vec![JobControl::Resume, JobControl::Cancel]
        }
        JobStage::Queued | JobStage::Resolving | JobStage::Selecting | JobStage::Verifying => {
            vec![JobControl::Cancel]
        }
        JobStage::Deploying
        | JobStage::NeedsReconciliation
        | JobStage::Completed
        | JobStage::Cancelled => Vec::new(),
    }
}

fn map_job_view(view: JobView) -> Result<ApiJobSnapshot, AppErrorDto> {
    ApiJobSnapshot::from_domain(view.snapshot, view.title)
}

fn normalize_details_request(request: &mut DetailsRequest) -> Result<(), AppErrorDto> {
    request.product_id = request.product_id.trim().to_owned();
    request.market.make_ascii_uppercase();
    if !safe_identifier(&request.product_id)
        || !safe_market(&request.market)
        || !safe_language(&request.language)
    {
        return Err(boundary_error(ErrorCode::CatalogUnavailable));
    }
    Ok(())
}

fn normalize_start_request(mut request: StartJobRequest) -> Result<StartJobSpec, AppErrorDto> {
    request.product_id = request.product_id.trim().to_owned();
    request.market.make_ascii_uppercase();
    if !safe_identifier(&request.product_id)
        || !safe_market(&request.market)
        || !safe_language(&request.language)
    {
        return Err(boundary_error(ErrorCode::CatalogUnavailable));
    }
    Ok(StartJobSpec {
        product_id: request.product_id,
        market: request.market,
        language: request.language,
        scope: request.scope.into(),
    })
}

fn boundary_error(code: ErrorCode) -> AppErrorDto {
    AppErrorDto::new(code, RetryAdvice::Never)
}

fn sanitize_error(error: AppErrorDto) -> AppErrorDto {
    let mut sanitized = AppErrorDto::new(error.code, error.retry);
    sanitized.job_id = error.job_id.filter(|value| safe_identifier(value));
    sanitized.details = error.details.into_iter().take(16).collect();
    sanitized
}

fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:{}".contains(&byte))
}

fn safe_language(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn has_case_insensitive_duplicates(values: &[String]) -> bool {
    values.iter().enumerate().any(|(index, value)| {
        values[..index]
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(value))
    })
}

fn safe_market(value: &str) -> bool {
    value.len() == 2 && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn safe_package_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte))
}

fn safe_proxy_host(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._".contains(&byte))
}

fn safe_text(value: &str, maximum_length: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum_length
        && !value.chars().any(char::is_control)
        && !contains_sensitive_marker(value)
}

fn contains_sensitive_marker(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    lowered.contains("http://")
        || lowered.contains("https://")
        || lowered.contains("file://")
        || lowered.contains("authorization:")
        || lowered.contains("proxy-authorization:")
        || lowered.contains("password=")
        || lowered.contains("token=")
        || looks_like_windows_path(value)
        || looks_like_hresult(&lowered)
}

fn looks_like_windows_path(value: &str) -> bool {
    value.starts_with("\\\\")
        || value.starts_with('/')
        || value.as_bytes().windows(3).any(|window| {
            window[0].is_ascii_alphabetic()
                && window[1] == b':'
                && matches!(window[2], b'\\' | b'/')
        })
}

fn looks_like_hresult(value: &str) -> bool {
    value.as_bytes().windows(10).any(|window| {
        window[0] == b'0' && window[1] == b'x' && window[2..].iter().all(u8::is_ascii_hexdigit)
    })
}
