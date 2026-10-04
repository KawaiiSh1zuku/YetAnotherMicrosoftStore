use serde_json::json;
use yet_another_microsoft_store_lib::{
    catalog::CatalogError,
    deployment::DeploymentScope,
    domain::{
        AppSettings, Architecture, DiagnosticEvent, DiagnosticOperation, InstallSource,
        PackageFormat, PackageKind, PackageRecord, PackageVersion, ThemeMode,
    },
    error::{classify_deployment_hresult, AppErrorDto, ErrorCode, RetryAdvice},
    jobs::{Job, JobKind, JobStage, RecoveryAction},
    package_process::{ProcessDescriptor, TerminatePackageProcessesResult},
};

#[test]
fn package_process_termination_result_serializes_bounded_descriptors() {
    let first = ProcessDescriptor {
        pid: 420,
        name: "Example.exe".to_owned(),
    };
    let second = ProcessDescriptor {
        pid: 421,
        name: "Background.exe".to_owned(),
    };
    let result = TerminatePackageProcessesResult {
        matched: vec![first.clone(), second.clone()],
        terminated: vec![first],
        remaining: vec![second],
    };

    assert_eq!(
        serde_json::to_value(result).expect("process descriptors should serialize"),
        json!({
            "matched": [
                {"pid": 420, "name": "Example.exe"},
                {"pid": 421, "name": "Background.exe"}
            ],
            "terminated": [{"pid": 420, "name": "Example.exe"}],
            "remaining": [{"pid": 421, "name": "Background.exe"}]
        })
    );
}

#[test]
fn non_package_in_use_hresults_keep_their_existing_deployment_classification() {
    assert_eq!(
        classify_deployment_hresult(0x80070005_u32 as i32),
        ErrorCode::DeploymentDenied
    );
    assert_eq!(
        classify_deployment_hresult(0x80004005_u32 as i32),
        ErrorCode::DeploymentFailed
    );
}

#[test]
fn legacy_settings_default_new_ui_preferences_without_rewriting_old_json() {
    let settings: AppSettings = serde_json::from_str(
        r#"{
            "region":"US","market":"US","preferredArchitectures":["x64"],
            "preferredLanguages":["en-US"],"proxyMode":"disabled",
            "proxyHost":null,"proxyPort":null,"proxyCredentials":"prompt_every_time",
            "cacheEnabled":true,"cacheDirectory":"C:\\\\Cache","maxCacheBytes":1024,
            "retentionDays":30,"keepInstalledPayloads":false,"maxConcurrentDownloads":2
        }"#,
    )
    .expect("legacy settings remain readable");

    assert_eq!(settings.theme, ThemeMode::System);
    assert!(!settings.diagnostics_enabled);
}

fn downloading_job() -> Job {
    Job {
        job_id: "job-001".to_owned(),
        kind: JobKind::Install,
        product_id: "9WZDNCRFJ3Q8".to_owned(),
        requested_market: "CN".to_owned(),
        requested_architectures: vec![Architecture::X64],
        requested_languages: vec!["zh-CN".to_owned()],
        deployment_scope: DeploymentScope::CurrentUser,
        selected_update_id: Some("update-main".to_owned()),
        package_family_name: Some("Example.App_123".to_owned()),
        stage: JobStage::Downloading,
        bytes_done: 512,
        bytes_total: Some(1024),
        deployment_progress: None,
        version: Some("1.2.3.4".to_owned()),
        architecture: Some(Architecture::X64),
        language: Some("zh-CN".to_owned()),
        error: None,
        blocked_processes: Vec::new(),
        created_at: 100,
        updated_at: 200,
    }
}

#[test]
fn package_domain_serializes_identity_selection_and_source_fields() {
    let package = PackageRecord {
        update_id: "update-main".to_owned(),
        product_id: "9WZDNCRFJ3Q8".to_owned(),
        package_family_name: Some("Example.App_123".to_owned()),
        package_moniker: "example-main".to_owned(),
        identity_name: Some("Example.App".to_owned()),
        publisher: Some("CN=Example".to_owned()),
        resource_id: None,
        package_kind: PackageKind::Main,
        version: PackageVersion::new(1, 2, 3, 4),
        architecture: Architecture::X64,
        language: Some("zh-CN".to_owned()),
        market: "CN".to_owned(),
        format: PackageFormat::MsixBundle,
        minimum_os_version: Some(PackageVersion::new(10, 0, 19045, 0)),
        is_neutral: Some(true),
        content_id: Some("content-main".to_owned()),
        file_size: Some(4096),
        sha256: Some("abcdef".to_owned()),
        install_source: InstallSource::MicrosoftStore,
    };

    assert_eq!(
        serde_json::to_value(package).expect("package should serialize"),
        json!({
            "updateId": "update-main",
            "productId": "9WZDNCRFJ3Q8",
            "packageFamilyName": "Example.App_123",
            "packageMoniker": "example-main",
            "identityName": "Example.App",
            "publisher": "CN=Example",
            "resourceId": null,
            "packageKind": "main",
            "version": "1.2.3.4",
            "architecture": "x64",
            "language": "zh-CN",
            "market": "CN",
            "format": "msix_bundle",
            "minimumOsVersion": "10.0.19045.0",
            "isNeutral": true,
            "contentId": "content-main",
            "fileSize": 4096,
            "sha256": "abcdef",
            "installSource": "microsoft_store"
        })
    );
}

#[test]
fn frontend_error_serialization_is_stable_and_does_not_expose_protocol_details() {
    let error = AppErrorDto::from(&CatalogError::InvalidUrl {
        field: "packageUri",
    });

    assert_eq!(error.code, ErrorCode::CatalogUnavailable);
    assert_eq!(error.retry, RetryAdvice::ReResolve);
    assert_eq!(
        serde_json::to_value(error).expect("error should serialize"),
        json!({
            "code": "catalog_unavailable",
            "messageKey": "errors.catalogUnavailable",
            "retry": "re_resolve",
            "details": [{"kind": "field", "field": "package_uri"}]
        })
    );
}

#[test]
fn missing_product_payload_is_not_misreported_as_catalog_not_found() {
    let error = AppErrorDto::from(&CatalogError::MissingField("product"));

    assert_eq!(error.code, ErrorCode::CatalogUnavailable);
    assert_eq!(error.retry, RetryAdvice::Retry);
}

#[test]
fn diagnostic_operation_is_a_closed_safe_enum() {
    let diagnostic = DiagnosticEvent {
        job_id: Some("job-001".to_owned()),
        code: ErrorCode::DownloadFailed,
        stage: Some(JobStage::Downloading),
        operation: DiagnosticOperation::Download,
        os_error_code: Some(12029),
        retryable: true,
        occurred_at: 300,
    };

    assert_eq!(
        serde_json::to_value(diagnostic).expect("diagnostic should serialize"),
        json!({
            "jobId": "job-001",
            "code": "download_failed",
            "stage": "downloading",
            "operation": "download",
            "osErrorCode": 12029,
            "retryable": true,
            "occurredAt": 300
        })
    );
}

#[test]
fn documented_error_code_set_is_stable() {
    let codes = [
        ErrorCode::CatalogNotFound,
        ErrorCode::CatalogUnavailable,
        ErrorCode::LicenseRequired,
        ErrorCode::MarketUnavailable,
        ErrorCode::NoCompatiblePackage,
        ErrorCode::DependencyUnresolved,
        ErrorCode::DownloadFailed,
        ErrorCode::DownloadProxyFailed,
        ErrorCode::DownloadProxyAuthRequired,
        ErrorCode::DownloadTimeout,
        ErrorCode::DownloadConnectionFailed,
        ErrorCode::DownloadResponseFailed,
        ErrorCode::DownloadHttpStatus,
        ErrorCode::DownloadRedirectRejected,
        ErrorCode::DownloadIoFailed,
        ErrorCode::DownloadUrlExpired,
        ErrorCode::HashMismatch,
        ErrorCode::SignatureInvalid,
        ErrorCode::DeploymentDenied,
        ErrorCode::DeploymentFailed,
        ErrorCode::PackageInUse,
        ErrorCode::StoreEntitlementMissing,
        ErrorCode::StoreChannelUnavailable,
        ErrorCode::SourceIdentityMismatch,
        ErrorCode::VersionAheadOfCatalog,
        ErrorCode::MsixvcCapabilityUnavailable,
        ErrorCode::UnsupportedPackageType,
        ErrorCode::PackageNotInstalled,
    ];

    assert_eq!(
        serde_json::to_value(codes).expect("codes should serialize"),
        json!([
            "catalog_not_found",
            "catalog_unavailable",
            "license_required",
            "market_unavailable",
            "no_compatible_package",
            "dependency_unresolved",
            "download_failed",
            "download_proxy_failed",
            "download_proxy_auth_required",
            "download_timeout",
            "download_connection_failed",
            "download_response_failed",
            "download_http_status",
            "download_redirect_rejected",
            "download_io_failed",
            "download_url_expired",
            "hash_mismatch",
            "signature_invalid",
            "deployment_denied",
            "deployment_failed",
            "package_in_use",
            "store_entitlement_missing",
            "store_channel_unavailable",
            "source_identity_mismatch",
            "version_ahead_of_catalog",
            "msixvc_capability_unavailable",
            "unsupported_package_type",
            "package_not_installed"
        ])
    );
}

#[test]
fn awaiting_elevation_is_not_a_valid_persisted_job_stage() {
    let parsed = serde_json::from_str::<JobStage>(r#""awaiting_elevation""#);

    assert!(parsed.is_err());
}

#[test]
fn active_download_becomes_interrupted_and_requires_re_resolution_after_restart() {
    let mut job = downloading_job();

    let recovery = job.recover_after_restart(300);

    assert_eq!(recovery, RecoveryAction::ReResolve);
    assert_eq!(job.stage, JobStage::Interrupted);
    assert_eq!(job.updated_at, 300);
}

#[test]
fn deploying_job_requires_inventory_reconciliation_after_restart() {
    let mut job = downloading_job();
    job.stage = JobStage::Deploying;

    let recovery = job.recover_after_restart(300);

    assert_eq!(recovery, RecoveryAction::ReconcileInventory);
    assert_eq!(job.stage, JobStage::NeedsReconciliation);
}

#[test]
fn awaiting_process_exit_is_active_cancellable_and_restart_stable() {
    let mut job = downloading_job();
    job.stage = JobStage::Deploying;
    job.transition_to(JobStage::AwaitingProcessExit, 250)
        .expect("deployment can wait for package processes");

    assert_eq!(
        serde_json::to_value(job.stage).expect("waiting stage should serialize"),
        "awaiting_process_exit"
    );
    assert_eq!(job.recover_after_restart(300), RecoveryAction::None);
    assert_eq!(job.stage, JobStage::AwaitingProcessExit);
    assert_eq!(job.updated_at, 250);
    job.transition_to(JobStage::Cancelled, 310)
        .expect("waiting deployment remains cancellable");
}

#[test]
fn retry_deployment_control_has_a_closed_wire_value() {
    assert_eq!(
        serde_json::to_value(
            yet_another_microsoft_store_lib::job_events::JobControl::RetryDeployment
        )
        .expect("retry deployment control should serialize"),
        "retry_deployment"
    );
}

#[test]
fn completed_job_rejects_further_state_changes() {
    let mut job = downloading_job();
    job.stage = JobStage::Completed;

    let error = job
        .transition_to(JobStage::Resolving, 300)
        .expect_err("completed jobs are terminal");

    assert_eq!(error.from, JobStage::Completed);
    assert_eq!(error.to, JobStage::Resolving);
    assert_eq!(job.updated_at, 200);
}
