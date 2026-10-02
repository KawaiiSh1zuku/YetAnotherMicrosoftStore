#![cfg(windows)]

use std::path::PathBuf;

use serde_json::json;
use yet_another_microsoft_store_lib::broker::{BrokerRequest, BrokerValidationError};
use yet_another_microsoft_store_lib::broker_protocol::{
    BrokerErrorCode, BrokerOperation, BrokerRequest as ElevatedBrokerRequest, PackageFileRequest,
    BROKER_PROTOCOL_VERSION,
};
use yet_another_microsoft_store_lib::deployment::{
    CapabilityStatus, DeploymentScope, WindowsDeploymentBackend,
};

#[test]
fn broker_rejects_non_msix_payloads() {
    let request = BrokerRequest::new(
        DeploymentScope::CurrentUser,
        PathBuf::from(r"C:\packages\installer.exe"),
        Vec::new(),
    );

    assert_eq!(
        request.validate(),
        Err(BrokerValidationError::UnsupportedPackageFormat)
    );
}

#[test]
fn broker_accepts_a_supported_package_graph() {
    let request = BrokerRequest::new(
        DeploymentScope::CurrentUser,
        PathBuf::from(r"C:\packages\app.msix"),
        vec![PathBuf::from(r"C:\packages\framework.appx")],
    );

    assert_eq!(request.validate(), Ok(()));
}

#[test]
fn broker_accepts_all_users_request_shape_for_uac_dispatch() {
    let request = BrokerRequest::new(
        DeploymentScope::AllUsers,
        PathBuf::from(r"C:\packages\app.msix"),
        Vec::new(),
    );

    assert_eq!(request.validate(), Ok(()));
}

#[test]
fn deployment_probe_keeps_all_users_scope_gated() {
    let probe = WindowsDeploymentBackend::probe().expect("probe should produce a result");

    assert_eq!(probe.current_user, CapabilityStatus::Available);
    assert_eq!(probe.all_users, CapabilityStatus::RequiresElevation);
}

#[test]
fn deployment_scope_serializes_both_supported_scopes() {
    assert_eq!(
        serde_json::from_value::<DeploymentScope>(json!("CurrentUser")).unwrap(),
        DeploymentScope::CurrentUser
    );
    assert_eq!(
        serde_json::from_value::<DeploymentScope>(json!("AllUsers")).unwrap(),
        DeploymentScope::AllUsers
    );
}

#[test]
fn elevated_broker_request_round_trips_with_version_and_nonce() {
    let request = ElevatedBrokerRequest::scan("request-1", 1234, 77, "nonce-1");
    let encoded = serde_json::to_vec(&request).unwrap();
    let decoded: ElevatedBrokerRequest = serde_json::from_slice(&encoded).unwrap();

    assert_eq!(decoded, request);
    assert_eq!(decoded.protocol_version, BROKER_PROTOCOL_VERSION);
    assert_eq!(decoded.operation, BrokerOperation::ScanAllUsers);
    assert_eq!(decoded.validate_shape(), Ok(()));
}

#[test]
fn elevated_broker_request_rejects_missing_nonce_and_bad_version() {
    let mut missing_nonce = ElevatedBrokerRequest::scan("request-1", 1234, 77, "nonce-1");
    missing_nonce.nonce.clear();
    assert_eq!(
        missing_nonce.validate_shape(),
        Err(BrokerErrorCode::InvalidRequest)
    );

    let mut bad_version = ElevatedBrokerRequest::scan("request-1", 1234, 77, "nonce-1");
    bad_version.protocol_version = BROKER_PROTOCOL_VERSION + 1;
    assert_eq!(
        bad_version.validate_shape(),
        Err(BrokerErrorCode::ProtocolMismatch)
    );
}

#[test]
fn package_request_rejects_unsupported_extension() {
    let package = PackageFileRequest {
        path: PathBuf::from(r"C:\packages\installer.exe"),
        sha256_hex: "00".repeat(32),
        expected_identity: None,
    };

    assert_eq!(
        package.validate_shape(),
        Err(BrokerErrorCode::UnsupportedPackageFormat)
    );
}
