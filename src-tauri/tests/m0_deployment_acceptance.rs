#![cfg(windows)]

use std::{env, path::PathBuf};

use yet_another_microsoft_store_lib::{
    broker_protocol::{PackageFileRequest, PackageIdentity},
    deployment::{DeploymentScope, WindowsDeploymentBackend},
    deployment_coordinator::DeploymentCoordinator,
    package_validation::VerifiedPackageSet,
};

struct PackageCleanup(Option<String>);

impl Drop for PackageCleanup {
    fn drop(&mut self) {
        if let Some(full_name) = self.0.take() {
            let _ = WindowsDeploymentBackend::remove_current_user(&full_name);
        }
    }
}

#[test]
#[ignore = "requires a signed test package and M0_PACKAGE_PATH/M0_PACKAGE_FULL_NAME"]
fn current_user_install_and_uninstall_round_trip() {
    let package_path = PathBuf::from(
        env::var_os("M0_PACKAGE_PATH").expect("M0_PACKAGE_PATH must point to a signed .msix"),
    );
    let package_full_name = env::var("M0_PACKAGE_FULL_NAME")
        .expect("M0_PACKAGE_FULL_NAME must match the signed package full name");

    let package = VerifiedPackageSet {
        main: PackageFileRequest {
            path: package_path,
            sha256_hex: env::var("M0_PACKAGE_SHA256")
                .expect("M0_PACKAGE_SHA256 must match the signed package"),
            expected_identity: None,
        },
        dependencies: Vec::new(),
    };

    WindowsDeploymentBackend::install_current_user(&package)
        .expect("PackageManager current-user install should succeed");

    let mut cleanup = PackageCleanup(Some(package_full_name.clone()));

    WindowsDeploymentBackend::remove_current_user(&package_full_name)
        .expect("PackageManager current-user uninstall should succeed");
    cleanup.0 = None;
}

#[test]
#[ignore = "requires an elevated broker package and M0 all-users environment variables"]
fn all_users_stage_provision_deprovision_and_remove_round_trip() {
    let package_path = PathBuf::from(
        env::var_os("M0_PACKAGE_PATH").expect("M0_PACKAGE_PATH must point to a signed .msix"),
    );
    let package_full_name = env::var("M0_PACKAGE_FULL_NAME")
        .expect("M0_PACKAGE_FULL_NAME must match the signed package full name");
    let package_family_name = env::var("M0_PACKAGE_FAMILY_NAME")
        .expect("M0_PACKAGE_FAMILY_NAME must match the signed package family name");
    let package = VerifiedPackageSet {
        main: PackageFileRequest {
            path: package_path,
            sha256_hex: env::var("M0_PACKAGE_SHA256")
                .expect("M0_PACKAGE_SHA256 must match the signed package"),
            expected_identity: Some(PackageIdentity {
                name: env::var("M0_PACKAGE_IDENTITY_NAME")
                    .expect("M0_PACKAGE_IDENTITY_NAME is required"),
                publisher: env::var("M0_PACKAGE_PUBLISHER")
                    .expect("M0_PACKAGE_PUBLISHER is required"),
                version: env::var("M0_PACKAGE_VERSION")
                    .expect("M0_PACKAGE_VERSION is required")
                    .split('.')
                    .map(|part| part.parse().expect("version component must be numeric"))
                    .collect::<Vec<u16>>()
                    .try_into()
                    .expect("M0_PACKAGE_VERSION must have four components"),
                architecture: env::var("M0_PACKAGE_ARCHITECTURE")
                    .expect("M0_PACKAGE_ARCHITECTURE is required"),
                resource_id: env::var("M0_PACKAGE_RESOURCE_ID").unwrap_or_default(),
            }),
        },
        dependencies: Vec::new(),
    };

    DeploymentCoordinator::install(DeploymentScope::AllUsers, &package)
        .expect("all-users broker stage and provision should succeed");
    DeploymentCoordinator::uninstall(
        DeploymentScope::AllUsers,
        &package_family_name,
        std::slice::from_ref(&package_full_name),
    )
    .expect("all-users broker deprovision and removal should succeed");
}

#[test]
#[ignore = "requires an already registered current-user package and M0_PACKAGE_FULL_NAME"]
fn current_user_uninstall_existing_package() {
    let package_full_name = env::var("M0_PACKAGE_FULL_NAME")
        .expect("M0_PACKAGE_FULL_NAME must match the registered package full name");

    WindowsDeploymentBackend::remove_current_user(&package_full_name)
        .expect("PackageManager current-user uninstall should succeed");
}
