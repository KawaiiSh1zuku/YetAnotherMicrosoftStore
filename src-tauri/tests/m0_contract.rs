#![cfg(windows)]

use std::path::PathBuf;

use yet_another_microsoft_store_lib::broker::{BrokerRequest, BrokerValidationError};
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
fn broker_does_not_accept_all_users_scope_before_uac_path_exists() {
    let request = BrokerRequest::new(
        DeploymentScope::AllUsers,
        PathBuf::from(r"C:\packages\app.msix"),
        Vec::new(),
    );

    assert_eq!(
        request.validate(),
        Err(BrokerValidationError::AllUsersBrokerNotReady)
    );
}

#[test]
fn deployment_probe_keeps_all_users_scope_gated() {
    let probe = WindowsDeploymentBackend::probe().expect("probe should produce a result");

    assert_eq!(probe.current_user, CapabilityStatus::Available);
    assert_eq!(probe.all_users, CapabilityStatus::RequiresElevation);
}
