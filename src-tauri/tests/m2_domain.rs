use serde_json::json;
use yet_another_microsoft_store_lib::{
    catalog::CatalogError,
    deployment::DeploymentScope,
    domain::{
        Architecture, DiagnosticEvent, DiagnosticOperation, InstallSource, PackageFormat,
        PackageKind, PackageRecord, PackageVersion,
    },
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    jobs::{Job, JobKind, JobStage, RecoveryAction},
};

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
        version: Some("1.2.3.4".to_owned()),
        architecture: Some(Architecture::X64),
        language: Some("zh-CN".to_owned()),
        requires_elevation: false,
        error: None,
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
        ErrorCode::DownloadUrlExpired,
        ErrorCode::HashMismatch,
        ErrorCode::SignatureInvalid,
        ErrorCode::ElevationCancelled,
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
            "download_url_expired",
            "hash_mismatch",
            "signature_invalid",
            "elevation_cancelled",
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
