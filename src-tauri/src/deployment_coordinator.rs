use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    deployment::{DeploymentScope, WindowsDeploymentBackend},
    inventory::{InventorySnapshot, WindowsInventory},
    package_validation::{verify_package_request, ValidationError, VerifiedPackageSet},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentRoute {
    CurrentUserDirect,
    AllUsersDirect,
}

pub fn route_for_scope(scope: DeploymentScope) -> DeploymentRoute {
    match scope {
        DeploymentScope::CurrentUser => DeploymentRoute::CurrentUserDirect,
        DeploymentScope::AllUsers => DeploymentRoute::AllUsersDirect,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinatorError {
    pub code: String,
    pub message: String,
}

impl fmt::Display for CoordinatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CoordinatorError {}

pub struct DeploymentCoordinator;

impl DeploymentCoordinator {
    pub fn scan(scope: DeploymentScope) -> Result<InventorySnapshot, CoordinatorError> {
        match route_for_scope(scope) {
            DeploymentRoute::CurrentUserDirect => {
                WindowsInventory::scan_current_user().map_err(inventory_error)
            }
            DeploymentRoute::AllUsersDirect => {
                WindowsInventory::scan_all_users().map_err(inventory_error)
            }
        }
    }

    pub fn install(
        scope: DeploymentScope,
        package: &VerifiedPackageSet,
    ) -> Result<InventorySnapshot, CoordinatorError> {
        verify_package_request(&package.main).map_err(validation_error)?;
        for dependency in &package.dependencies {
            verify_package_request(dependency).map_err(validation_error)?;
        }
        match route_for_scope(scope) {
            DeploymentRoute::CurrentUserDirect => {
                WindowsDeploymentBackend::install_current_user(package)
                    .map_err(deployment_error)?;
            }
            DeploymentRoute::AllUsersDirect => {
                let root = protected_root();
                let result =
                    WindowsDeploymentBackend::stage_and_provision_all_users(package, &root);
                let _ = std::fs::remove_dir_all(&root);
                result.map_err(deployment_error)?;
            }
        }
        let snapshot = Self::scan(scope)?;
        ensure_complete(&snapshot)?;
        if let Some(identity) = package.main.expected_identity.as_ref() {
            if !snapshot
                .records
                .iter()
                .any(|record| record.identity_name == identity.name)
            {
                return Err(CoordinatorError {
                    code: "postcondition_missing".to_owned(),
                    message: "installed package was not found in the postcondition inventory"
                        .to_owned(),
                });
            }
        }
        Ok(snapshot)
    }

    pub fn uninstall(
        scope: DeploymentScope,
        package_family_name: &str,
        package_full_names: &[String],
    ) -> Result<InventorySnapshot, CoordinatorError> {
        if package_family_name.trim().is_empty() || package_full_names.is_empty() {
            return Err(CoordinatorError {
                code: "invalid_target".to_owned(),
                message: "package family and full names are required".to_owned(),
            });
        }
        match route_for_scope(scope) {
            DeploymentRoute::CurrentUserDirect => {
                for full_name in package_full_names {
                    WindowsDeploymentBackend::remove_current_user(full_name)
                        .map_err(deployment_error)?;
                }
            }
            DeploymentRoute::AllUsersDirect => {
                WindowsDeploymentBackend::deprovision_and_remove_all_users(
                    package_family_name,
                    package_full_names,
                )
                .map_err(deployment_error)?;
            }
        }
        let snapshot = Self::scan(scope)?;
        ensure_complete(&snapshot)?;
        if snapshot.records.iter().any(|record| {
            record.package_family_name == package_family_name
                || package_full_names.contains(&record.package_full_name)
        }) {
            return Err(CoordinatorError {
                code: "postcondition_residual".to_owned(),
                message: "package remains in the postcondition inventory".to_owned(),
            });
        }
        Ok(snapshot)
    }
}

fn ensure_complete(snapshot: &InventorySnapshot) -> Result<(), CoordinatorError> {
    if snapshot.complete {
        Ok(())
    } else {
        Err(CoordinatorError {
            code: "incomplete_inventory".to_owned(),
            message: snapshot.warnings.join("; "),
        })
    }
}

fn protected_root() -> PathBuf {
    let base = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    base.join("YetAnotherMicrosoftStore")
        .join("Staging")
        .join(format!("{}-{}", std::process::id(), uuid::Uuid::new_v4()))
}

fn inventory_error(error: crate::inventory::InventoryError) -> CoordinatorError {
    CoordinatorError {
        code: match error {
            crate::inventory::InventoryError::AccessDenied => "inventory_access_denied",
            crate::inventory::InventoryError::UnsupportedPlatform => "unsupported_platform",
            crate::inventory::InventoryError::WindowsApi(_) => "inventory_windows_api",
        }
        .to_owned(),
        message: error.to_string(),
    }
}

fn deployment_error(error: crate::deployment::DeploymentError) -> CoordinatorError {
    CoordinatorError {
        code: "deployment_failed".to_owned(),
        message: error.to_string(),
    }
}

fn validation_error(error: ValidationError) -> CoordinatorError {
    CoordinatorError {
        code: match error {
            ValidationError::HashMismatch => "hash_mismatch",
            ValidationError::IdentityMismatch => "source_identity_mismatch",
            ValidationError::SignatureInvalid => "signature_invalid",
            ValidationError::UnsupportedPackageFormat => "unsupported_package_type",
            ValidationError::InvalidPath
            | ValidationError::RootEscape
            | ValidationError::Io(_)
            | ValidationError::Manifest(_) => "deployment_failed",
        }
        .to_owned(),
        message: error.to_string(),
    }
}
