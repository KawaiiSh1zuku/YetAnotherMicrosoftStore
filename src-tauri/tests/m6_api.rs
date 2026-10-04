pub use yet_another_microsoft_store_lib::{
    applicability, catalog, deployment, domain, error, inventory, job_events, jobs, package_process,
};

#[path = "../src/tauri_api.rs"]
mod tauri_api;

use tauri_api::{
    ApiAppDetails, ApiAppSettings, ApiBackend, ApiCatalogProduct, ApiDeploymentScope, ApiFuture,
    ApiInventorySnapshot, ApiJobSnapshot, ApiUpdateCandidate, ApiUpdateScanResult,
    AppDetailsSource, DetailsRequest, JobChangedHint, JobControlRequest, JobView,
    ListJobEventsRequest, LocalProductAction, LocalProductActionKind, SearchRequest,
    StartJobRequest, StartJobSpec, TauriApi, JOB_CHANGED_EVENT,
};
use yet_another_microsoft_store_lib::{
    applicability::{SelectedMainPackage, SelectionPreview},
    catalog::{CatalogMetadataState, CatalogProduct},
    deployment::DeploymentScope,
    domain::{
        AppSettings, Architecture, PackageFormat, PackageVersion, ProxyCredentialPolicy, ProxyMode,
        ThemeMode,
    },
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    inventory::{
        InventorySnapshot, InventorySource, PackageInventoryRecord,
        PackageKind as InventoryPackageKind,
    },
    job_events::{JobControl, JobEvent, StoredJobEvent},
    jobs::{Job, JobKind, JobSnapshot, JobStage},
    package_process::{ProcessDescriptor, TerminatePackageProcessesResult},
};

fn catalog_product() -> CatalogProduct {
    CatalogProduct {
        product_id: "9NBLGGH4NNS1".to_owned(),
        package_family_name: Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe".to_owned()),
        app_name: Some("Windows Terminal".to_owned()),
        package_name: Some("Microsoft.WindowsTerminal".to_owned()),
        publisher: Some("Microsoft Corporation".to_owned()),
        package_publisher: Some("CN=Microsoft Corporation".to_owned()),
        icon_url: Some("https://store-images.s-microsoft.com/image.png".to_owned()),
        metadata_state: CatalogMetadataState::Complete,
        package_formats: vec!["msixbundle".to_owned()],
        framework_dependencies: vec!["Microsoft.VCLibs.140.00".to_owned()],
    }
}

fn domain_settings() -> AppSettings {
    AppSettings {
        region: "US".to_owned(),
        market: "US".to_owned(),
        preferred_architectures: vec![Architecture::X64],
        preferred_languages: vec!["en-US".to_owned()],
        proxy_mode: ProxyMode::Disabled,
        proxy_host: None,
        proxy_port: None,
        proxy_credentials: ProxyCredentialPolicy::PromptEveryTime,
        cache_enabled: true,
        cache_directory: r"C:\Users\test\AppData\Local\YAMS\cache".to_owned(),
        max_cache_bytes: 10_737_418_240,
        retention_days: 30,
        keep_installed_payloads: false,
        max_concurrent_downloads: 2,
        max_concurrent_update_scans: 16,
        theme: ThemeMode::Dark,
        diagnostics_enabled: true,
    }
}

fn downloading_job() -> JobSnapshot {
    JobSnapshot {
        sequence: 4,
        job: Job {
            job_id: "job-1".to_owned(),
            kind: JobKind::Install,
            product_id: "9NBLGGH4NNS1".to_owned(),
            requested_market: "US".to_owned(),
            requested_architectures: vec![Architecture::X64],
            requested_languages: vec!["en-US".to_owned()],
            deployment_scope: DeploymentScope::CurrentUser,
            selected_update_id: Some("update-1".to_owned()),
            package_family_name: Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe".to_owned()),
            stage: JobStage::Downloading,
            bytes_done: 25,
            bytes_total: Some(100),
            deployment_progress: None,
            version: Some("1.2.3.4".to_owned()),
            architecture: Some(Architecture::X64),
            language: Some("en-US".to_owned()),
            error: None,
            blocked_processes: Vec::new(),
            created_at: 10,
            updated_at: 20,
        },
    }
}

#[test]
fn job_snapshot_serialization_flattens_domain_state_and_derives_controls() {
    let snapshot =
        ApiJobSnapshot::from_domain(downloading_job(), Some("Windows Terminal".to_owned()))
            .expect("safe domain job should map to an API snapshot");

    assert_eq!(
        serde_json::to_value(snapshot).expect("API snapshot should serialize"),
        serde_json::json!({
            "jobId": "job-1",
            "sequence": 4,
            "productId": "9NBLGGH4NNS1",
            "packageFamilyName": "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
            "title": "Windows Terminal",
            "stage": "downloading",
            "bytesDone": 25,
            "bytesTotal": 100,
            "deploymentProgress": null,
            "version": "1.2.3.4",
            "architecture": "x64",
            "language": "en-US",
            "allowedControls": ["pause", "cancel"],
            "error": null,
            "blockedProcesses": [],
            "updatedAt": 20
        })
    );
}

#[test]
fn catalog_and_details_serialization_match_the_frontend_contract() {
    let product = ApiCatalogProduct::from_domain(catalog_product())
        .expect("safe catalog data should map to the API");
    let details = ApiAppDetails::from_catalog(
        catalog_product(),
        "US".to_owned(),
        "en-US".to_owned(),
        vec![Architecture::X64, Architecture::Arm64],
        selection_preview(),
        LocalProductAction {
            kind: LocalProductActionKind::Update,
            deployment_scope: Some(ApiDeploymentScope::CurrentUser),
            installed_version: Some("1.0.0.0".to_owned()),
            available_version: Some("1.2.3.4".to_owned()),
            launchable: true,
        },
    )
    .expect("safe details should map to the API");

    assert_eq!(
        serde_json::to_value(product).expect("catalog product should serialize"),
        serde_json::json!({
            "productId": "9NBLGGH4NNS1",
            "packageFamilyName": "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
            "appName": "Windows Terminal",
            "packageName": "Microsoft.WindowsTerminal",
            "publisher": "Microsoft Corporation",
            "iconUrl": "https://store-images.s-microsoft.com/image.png",
            "metadataState": "complete",
            "packageFormats": ["msixbundle"],
            "frameworkDependencies": ["Microsoft.VCLibs.140.00"]
        })
    );
    assert_eq!(
        serde_json::to_value(details).expect("app details should serialize"),
        serde_json::json!({
            "productId": "9NBLGGH4NNS1",
            "packageFamilyName": "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
            "appName": "Windows Terminal",
            "packageName": "Microsoft.WindowsTerminal",
            "publisher": "Microsoft Corporation",
            "iconUrl": "https://store-images.s-microsoft.com/image.png",
            "metadataState": "complete",
            "packageFormats": ["msixbundle"],
            "frameworkDependencies": ["Microsoft.VCLibs.140.00"],
            "market": "US",
            "language": "en-US",
            "supportedArchitectures": ["x64", "arm64"],
            "selectionPreview": {
                "installable": true,
                "main": {
                    "version": "1.2.3.4",
                    "architecture": "x64",
                    "format": "msix_bundle",
                    "language": "en-US"
                },
                "dependencyCount": 1,
                "rejectionReason": null
            },
            "localAction": {
                "kind": "update",
                "deploymentScope": "current_user",
                "installedVersion": "1.0.0.0",
                "availableVersion": "1.2.3.4",
                "launchable": true
            }
        })
    );
}

#[test]
fn inventory_serialization_omits_internal_windows_details_and_redacts_warnings() {
    let snapshot = InventorySnapshot {
        source: InventorySource::CurrentUser,
        captured_at: "2026-10-03T00:00:00Z".to_owned(),
        os_build: "19045".to_owned(),
        complete: false,
        records: vec![PackageInventoryRecord {
            app_name: "Windows Terminal".to_owned(),
            package_name: "Microsoft.WindowsTerminal".to_owned(),
            identity_name: "Microsoft.WindowsTerminal".to_owned(),
            publisher: "CN=Microsoft Corporation".to_owned(),
            package_family_name: "Microsoft.WindowsTerminal_8wekyb3d8bbwe".to_owned(),
            package_full_name: "Microsoft.WindowsTerminal_1.2.3.4_x64__8wekyb3d8bbwe".to_owned(),
            version: [1, 2, 3, 4],
            architecture: "x64".to_owned(),
            resource_id: r"C:\sensitive\resource".to_owned(),
            package_kind: InventoryPackageKind::Main,
            signature_kind: "Store".to_owned(),
            status: r"HRESULT 0x80073CF9 at C:\sensitive".to_owned(),
            installed_for_current_user: true,
            installed_user_count: 1,
            has_other_users: false,
            provisioned_for_future_users: false,
        }],
        warnings: vec![r"HRESULT 0x80070005 at C:\Users\secret".to_owned()],
    };

    let value = serde_json::to_value(
        ApiInventorySnapshot::from_domain(snapshot)
            .expect("safe inventory identity should map to the API"),
    )
    .expect("inventory should serialize");

    assert_eq!(value["source"], "current_user");
    assert_eq!(value["warnings"], serde_json::json!(["partial_inventory"]));
    assert_eq!(value["records"][0]["packageKind"], "main");
    assert!(value["records"][0].get("resourceId").is_none());
    assert!(value["records"][0].get("signatureKind").is_none());
    assert!(value["records"][0].get("status").is_none());
    assert!(value["records"][0].get("installedUserCount").is_none());
    let serialized = value.to_string();
    assert!(!serialized.contains("0x80070005"));
    assert!(!serialized.contains(r"C:\Users\secret"));
}

#[test]
fn settings_serialization_preserves_ui_preferences_without_exposing_cache_path() {
    let value = serde_json::to_value(
        ApiAppSettings::from_domain(domain_settings()).expect("valid settings should map to API"),
    )
    .expect("settings should serialize");

    assert_eq!(value["theme"], "dark");
    assert_eq!(value["diagnosticsEnabled"], true);
    assert_eq!(value["proxyCredentials"], "prompt_every_time");
    assert_eq!(value["maxCacheBytes"], 10_737_418_240_u64);
    assert_eq!(value["maxConcurrentUpdateScans"], 16);
    assert!(value.get("cacheDirectory").is_none());
    assert!(!value.to_string().contains("AppData"));
}

#[test]
fn api_scope_uses_snake_case_without_changing_the_domain_wire_contract() {
    assert_eq!(
        serde_json::to_value(ApiDeploymentScope::CurrentUser).expect("scope should serialize"),
        "current_user"
    );
    assert_eq!(
        serde_json::to_value(ApiDeploymentScope::AllUsers).expect("scope should serialize"),
        "all_users"
    );
    assert_eq!(
        DeploymentScope::from(ApiDeploymentScope::CurrentUser),
        DeploymentScope::CurrentUser
    );
    assert_eq!(
        DeploymentScope::from(ApiDeploymentScope::AllUsers),
        DeploymentScope::AllUsers
    );
    assert_eq!(
        serde_json::to_value(DeploymentScope::CurrentUser).expect("domain scope should serialize"),
        "CurrentUser"
    );
}

#[test]
fn reconciliation_snapshot_exposes_no_user_controls() {
    let mut snapshot = downloading_job();
    snapshot.job.stage = JobStage::NeedsReconciliation;
    snapshot.job.bytes_done = 0;
    snapshot.job.bytes_total = None;

    let api = ApiJobSnapshot::from_domain(snapshot, None)
        .expect("reconciliation snapshot should map to API");

    assert!(api.allowed_controls.is_empty());
}

#[test]
fn awaiting_process_exit_snapshot_exposes_only_retry_deployment_and_cancel() {
    let mut snapshot = downloading_job();
    snapshot.job.stage = JobStage::AwaitingProcessExit;
    snapshot.job.bytes_done = 100;
    snapshot.job.bytes_total = Some(100);
    snapshot.job.deployment_progress = Some(0);

    let api =
        ApiJobSnapshot::from_domain(snapshot, None).expect("waiting snapshot should map to API");

    assert_eq!(
        api.allowed_controls,
        vec![JobControl::RetryDeployment, JobControl::Cancel]
    );
}

#[test]
fn settings_mapping_preserves_current_fields_and_domain_legacy_defaults() {
    let current =
        ApiAppSettings::from_domain(domain_settings()).expect("current settings should map to API");
    assert_eq!(current.theme, ThemeMode::Dark);
    assert!(current.diagnostics_enabled);

    let mut legacy = serde_json::to_value(domain_settings()).expect("settings should serialize");
    let object = legacy
        .as_object_mut()
        .expect("settings should serialize as an object");
    object.remove("theme");
    object.remove("diagnosticsEnabled");
    object.remove("maxConcurrentUpdateScans");
    let legacy: AppSettings =
        serde_json::from_value(legacy).expect("legacy settings should use serde defaults");
    let legacy = ApiAppSettings::from_domain(legacy).expect("legacy settings should map to API");

    assert_eq!(legacy.theme, ThemeMode::System);
    assert!(!legacy.diagnostics_enabled);
    assert_eq!(
        serde_json::to_value(legacy).expect("legacy settings should serialize")
            ["maxConcurrentUpdateScans"],
        16
    );
}

#[test]
fn update_scan_concurrency_accepts_sixty_four_and_rejects_values_above_the_limit() {
    let mut value = serde_json::to_value(
        ApiAppSettings::from_domain(domain_settings()).expect("valid settings should map to API"),
    )
    .expect("settings should serialize");
    value["maxConcurrentUpdateScans"] = serde_json::json!(64);
    let accepted: ApiAppSettings =
        serde_json::from_value(value.clone()).expect("64 concurrent update scans should parse");
    assert!(accepted.validated().is_ok());

    value["maxConcurrentUpdateScans"] = serde_json::json!(65);
    let rejected: ApiAppSettings =
        serde_json::from_value(value).expect("out-of-range value should reach validation");
    assert!(rejected.validated().is_err());
}

#[test]
fn language_priorities_allow_an_empty_list_but_reject_case_insensitive_duplicates() {
    let mut settings =
        ApiAppSettings::from_domain(domain_settings()).expect("valid settings should map to API");
    settings.preferred_languages.clear();
    assert!(settings.clone().validated().is_ok());

    settings.preferred_languages = vec!["en-US".to_owned(), "EN-us".to_owned()];
    assert!(settings.validated().is_err());
}

#[derive(Debug, Clone, Copy, Default)]
enum SearchMode {
    #[default]
    Safe,
    UnsafeSuccess,
    UnsafeError,
}

#[derive(Default)]
struct FixtureBackend {
    search_mode: SearchMode,
    mismatch_event_sequence: bool,
}

fn raw_inventory() -> InventorySnapshot {
    InventorySnapshot {
        source: InventorySource::CurrentUser,
        captured_at: "2026-10-03T00:00:00Z".to_owned(),
        os_build: "19045".to_owned(),
        complete: true,
        records: Vec::new(),
        warnings: Vec::new(),
    }
}

fn api_settings() -> ApiAppSettings {
    ApiAppSettings::from_domain(domain_settings()).expect("fixture settings should be valid")
}

fn job_view() -> JobView {
    JobView {
        snapshot: downloading_job(),
        title: Some("Windows Terminal".to_owned()),
    }
}

impl ApiBackend for FixtureBackend {
    fn search_apps(&self, _request: SearchRequest) -> ApiFuture<'_, Vec<CatalogProduct>> {
        Box::pin(async move {
            match self.search_mode {
                SearchMode::Safe => Ok(vec![catalog_product()]),
                SearchMode::UnsafeSuccess => {
                    let mut product = catalog_product();
                    product.publisher = Some(
                        r"https://cdn.example.invalid/package?token=secret&C:\Users\secret"
                            .to_owned(),
                    );
                    Ok(vec![product])
                }
                SearchMode::UnsafeError => {
                    let mut error =
                        AppErrorDto::new(ErrorCode::CatalogUnavailable, RetryAdvice::Retry);
                    error.message_key =
                        r"HRESULT 0x80070005 C:\Users\secret token=secret".to_owned();
                    error.job_id = Some(r"C:\Users\secret".to_owned());
                    Err(error)
                }
            }
        })
    }

    fn get_app_details(&self, _request: DetailsRequest) -> ApiFuture<'_, AppDetailsSource> {
        Box::pin(async {
            Ok(AppDetailsSource {
                product: catalog_product(),
                supported_architectures: vec![Architecture::X64, Architecture::Arm64],
                selection_preview: selection_preview(),
                local_action: LocalProductAction {
                    kind: LocalProductActionKind::Install,
                    deployment_scope: None,
                    installed_version: None,
                    available_version: Some("1.2.3.4".to_owned()),
                    launchable: false,
                },
            })
        })
    }

    fn scan_installed_packages(&self, _scope: DeploymentScope) -> ApiFuture<'_, InventorySnapshot> {
        Box::pin(async { Ok(raw_inventory()) })
    }

    fn scan_updates(&self) -> ApiFuture<'_, ApiUpdateScanResult> {
        Box::pin(async {
            Ok(ApiUpdateScanResult {
                scanned_main_packages: 1,
                associated_packages: 1,
                candidates: vec![ApiUpdateCandidate {
                    app_name: "Windows Terminal".to_owned(),
                    package_name: "Microsoft.WindowsTerminal".to_owned(),
                    publisher: "Microsoft Corporation".to_owned(),
                    package_family_name: "Microsoft.WindowsTerminal_8wekyb3d8bbwe".to_owned(),
                    current_version: "1.0.0.0".to_owned(),
                    available_version: "1.2.3.4".to_owned(),
                    selected_update_id: "update-terminal-v2".to_owned(),
                    product_id: Some("9NBLGGH4NNS1".to_owned()),
                    deployment_scope: ApiDeploymentScope::CurrentUser,
                }],
                skipped: Vec::new(),
                complete: true,
            })
        })
    }

    fn start_install(&self, _request: StartJobSpec) -> ApiFuture<'_, JobView> {
        Box::pin(async { Ok(job_view()) })
    }

    fn start_update(&self, _request: StartJobSpec) -> ApiFuture<'_, JobView> {
        Box::pin(async { Ok(job_view()) })
    }

    fn request_job_control(&self, _request: JobControlRequest) -> ApiFuture<'_, JobView> {
        Box::pin(async { Ok(job_view()) })
    }

    fn terminate_job_package_processes(
        &self,
        _job_id: String,
    ) -> ApiFuture<'_, TerminatePackageProcessesResult> {
        Box::pin(async {
            Ok(TerminatePackageProcessesResult {
                matched: vec![
                    ProcessDescriptor {
                        pid: 420,
                        name: "Terminal.exe".to_owned(),
                    },
                    ProcessDescriptor {
                        pid: 421,
                        name: "OpenConsole.exe".to_owned(),
                    },
                ],
                terminated: vec![
                    ProcessDescriptor {
                        pid: 420,
                        name: "Terminal.exe".to_owned(),
                    },
                    ProcessDescriptor {
                        pid: 421,
                        name: "OpenConsole.exe".to_owned(),
                    },
                ],
                remaining: Vec::new(),
            })
        })
    }

    fn launch_installed_app(&self, _product_id: String) -> ApiFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }

    fn get_job(&self, _job_id: String) -> ApiFuture<'_, Option<JobView>> {
        Box::pin(async { Ok(Some(job_view())) })
    }

    fn list_jobs(&self) -> ApiFuture<'_, Vec<JobView>> {
        Box::pin(async { Ok(vec![job_view()]) })
    }

    fn list_job_events(
        &self,
        _request: ListJobEventsRequest,
    ) -> ApiFuture<'_, Vec<StoredJobEvent>> {
        Box::pin(async move {
            let mut queued = downloading_job();
            queued.sequence = 1;
            queued.job.stage = JobStage::Queued;
            queued.job.selected_update_id = None;
            queued.job.package_family_name = None;
            queued.job.bytes_done = 0;
            queued.job.bytes_total = None;
            queued.job.version = None;
            queued.job.architecture = None;
            queued.job.language = None;
            queued.job.updated_at = 10;
            let current = downloading_job();
            Ok(vec![
                StoredJobEvent {
                    cursor: 9,
                    job_id: queued.job.job_id.clone(),
                    sequence: 1,
                    event: JobEvent::Created {
                        job: queued.job.clone(),
                    },
                    snapshot: queued,
                    occurred_at: 10,
                },
                StoredJobEvent {
                    cursor: 10,
                    job_id: current.job.job_id.clone(),
                    sequence: if self.mismatch_event_sequence { 3 } else { 4 },
                    event: JobEvent::ProgressRecorded {
                        bytes_done: 25,
                        bytes_total: Some(100),
                    },
                    snapshot: current,
                    occurred_at: 20,
                },
            ])
        })
    }

    fn get_settings(&self) -> ApiFuture<'_, ApiAppSettings> {
        Box::pin(async { Ok(api_settings()) })
    }

    fn update_settings(&self, settings: ApiAppSettings) -> ApiFuture<'_, ApiAppSettings> {
        Box::pin(async move { Ok(settings) })
    }

    fn clear_cache(&self) -> ApiFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

fn selection_preview() -> SelectionPreview {
    SelectionPreview {
        installable: true,
        main: Some(SelectedMainPackage {
            version: PackageVersion::new(1, 2, 3, 4),
            architecture: Architecture::X64,
            format: PackageFormat::MsixBundle,
            language: Some("en-US".to_owned()),
        }),
        dependency_count: 1,
        rejection_reason: None,
    }
}

fn details_request() -> DetailsRequest {
    DetailsRequest {
        product_id: "9NBLGGH4NNS1".to_owned(),
        market: "US".to_owned(),
        language: "en-US".to_owned(),
    }
}

fn start_request() -> StartJobRequest {
    StartJobRequest {
        product_id: "9NBLGGH4NNS1".to_owned(),
        market: "US".to_owned(),
        language: "en-US".to_owned(),
        scope: ApiDeploymentScope::CurrentUser,
        selected_update_id: None,
        package_family_name: None,
    }
}

#[tokio::test]
async fn facade_exposes_the_complete_closed_command_set() {
    let api = TauriApi::new(FixtureBackend::default());

    let products = api
        .search_apps(SearchRequest {
            query: "terminal".to_owned(),
            market: "US".to_owned(),
            language: "en-US".to_owned(),
        })
        .await
        .expect("search should succeed");
    assert_eq!(products[0].app_name, "Windows Terminal");
    assert_eq!(
        api.get_app_details(details_request())
            .await
            .expect("details should succeed")
            .market,
        "US"
    );
    assert!(
        api.scan_installed_packages(ApiDeploymentScope::CurrentUser)
            .await
            .expect("inventory should succeed")
            .complete
    );
    assert_eq!(
        api.scan_updates()
            .await
            .expect("update scan should succeed")
            .candidates
            .len(),
        1
    );
    assert_eq!(
        api.start_install(start_request())
            .await
            .expect("install should enqueue")
            .sequence,
        4
    );
    assert_eq!(
        api.start_update(start_request())
            .await
            .expect("update should enqueue")
            .sequence,
        4
    );
    assert_eq!(
        api.request_job_control(JobControlRequest {
            job_id: "job-1".to_owned(),
            command_id: "ui-command-1".to_owned(),
            expected_sequence: 4,
            control: JobControl::Pause,
        })
        .await
        .expect("control should enqueue")
        .job_id,
        "job-1"
    );
    assert!(api
        .get_job("job-1".to_owned())
        .await
        .expect("get job should succeed")
        .is_some());
    assert_eq!(
        api.list_jobs()
            .await
            .expect("list jobs should succeed")
            .len(),
        1
    );
    assert_eq!(
        api.list_job_events(ListJobEventsRequest {
            after_cursor: Some(8),
            limit: 100,
        })
        .await
        .expect("event replay should succeed")
        .next_cursor,
        Some(10)
    );
    let settings = api
        .get_settings()
        .await
        .expect("get settings should succeed");
    assert_eq!(
        api.update_settings(settings)
            .await
            .expect("update settings should succeed")
            .theme,
        ThemeMode::Dark
    );
    api.clear_cache().await.expect("cache clear should succeed");
    api.launch_installed_app("9NBLGGH4NNS1".to_owned())
        .await
        .expect("installed application launch should succeed");
    assert_eq!(
        api.terminate_job_package_processes("job-1".to_owned())
            .await
            .expect("safe job process termination should succeed"),
        TerminatePackageProcessesResult {
            matched: vec![
                ProcessDescriptor {
                    pid: 420,
                    name: "Terminal.exe".to_owned()
                },
                ProcessDescriptor {
                    pid: 421,
                    name: "OpenConsole.exe".to_owned()
                },
            ],
            terminated: vec![
                ProcessDescriptor {
                    pid: 420,
                    name: "Terminal.exe".to_owned()
                },
                ProcessDescriptor {
                    pid: 421,
                    name: "OpenConsole.exe".to_owned()
                },
            ],
            remaining: Vec::new(),
        }
    );
}

#[tokio::test]
async fn installed_application_launch_rejects_an_untrusted_product_identifier() {
    let api = TauriApi::new(FixtureBackend::default());
    let error = api
        .launch_installed_app(r"product\other".to_owned())
        .await
        .expect_err("untrusted product id must fail before reaching the backend");
    assert_eq!(error.code, ErrorCode::DeploymentDenied);
}

#[tokio::test]
async fn process_termination_rejects_an_untrusted_job_identifier() {
    let api = TauriApi::new(FixtureBackend::default());
    let error = api
        .terminate_job_package_processes(r"job\\other".to_owned())
        .await
        .expect_err("untrusted job id must fail before reaching the backend");
    assert_eq!(error.code, ErrorCode::DeploymentDenied);
}

#[tokio::test]
async fn facade_rejects_event_sequence_that_differs_from_its_historical_snapshot() {
    let api = TauriApi::new(FixtureBackend {
        mismatch_event_sequence: true,
        ..FixtureBackend::default()
    });

    let error = api
        .list_job_events(ListJobEventsRequest {
            after_cursor: Some(8),
            limit: 100,
        })
        .await
        .expect_err("event and historical snapshot sequence mismatch must fail closed");

    assert_eq!(error.code, ErrorCode::DeploymentFailed);
}

#[tokio::test]
async fn cursor_page_uses_each_stored_events_historical_same_sequence_snapshot() {
    let api = TauriApi::new(FixtureBackend::default());

    let page = api
        .list_job_events(ListJobEventsRequest {
            after_cursor: Some(8),
            limit: 100,
        })
        .await
        .expect("historical event page should map");

    assert_eq!(page.events[0].sequence, 1);
    assert_eq!(page.events[0].snapshot.sequence, 1);
    assert_eq!(page.events[0].snapshot.stage, JobStage::Queued);
    assert_eq!(page.events[1].sequence, 4);
    assert_eq!(page.events[1].snapshot.sequence, 4);
    assert_eq!(page.events[1].snapshot.stage, JobStage::Downloading);
    assert_eq!(page.next_cursor, Some(10));
}

#[tokio::test]
async fn facade_rejects_sensitive_success_values_and_sanitizes_backend_errors() {
    for mode in [SearchMode::UnsafeSuccess, SearchMode::UnsafeError] {
        let api = TauriApi::new(FixtureBackend {
            search_mode: mode,
            ..FixtureBackend::default()
        });
        let error = api
            .search_apps(SearchRequest {
                query: "terminal".to_owned(),
                market: "US".to_owned(),
                language: "en-US".to_owned(),
            })
            .await
            .expect_err("unsafe backend data must not cross the API boundary");
        let serialized = serde_json::to_string(&error).expect("closed error should serialize");
        assert!(!serialized.contains("https://"));
        assert!(!serialized.contains("0x80070005"));
        assert!(!serialized.contains("token=secret"));
        assert!(!serialized.contains(r"C:\Users\secret"));
        assert_eq!(error.message_key, error.code.message_key());
    }
}

#[test]
fn changed_hint_contains_only_the_replay_watermark() {
    let snapshot =
        ApiJobSnapshot::from_domain(downloading_job(), Some("Windows Terminal".to_owned()))
            .expect("fixture job should map");

    assert_eq!(
        serde_json::to_value(JobChangedHint::from(&snapshot)).expect("hint should serialize"),
        serde_json::json!({"jobId": "job-1", "sequence": 4, "updatedAt": 20})
    );
    assert_eq!(JOB_CHANGED_EVENT, "job://changed");
}
