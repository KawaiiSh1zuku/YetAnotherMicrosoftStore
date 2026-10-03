use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{
    broker_launcher::{BrokerLaunchError, BrokerLauncher},
    broker_protocol::{AllUsersRemovalRequest, BrokerOperation, BrokerPayload, BrokerRequest},
    deployment::{DeploymentScope, WindowsDeploymentBackend},
    inventory::{InventorySnapshot, WindowsInventory},
    package_validation::{verify_package_request, ValidationError, VerifiedPackageSet},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentRoute {
    CurrentUserDirect,
    AllUsersBroker,
}

pub fn route_for_scope(scope: DeploymentScope) -> DeploymentRoute {
    match scope {
        DeploymentScope::CurrentUser => DeploymentRoute::CurrentUserDirect,
        DeploymentScope::AllUsers => DeploymentRoute::AllUsersBroker,
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
            DeploymentRoute::AllUsersBroker => {
                let response =
                    BrokerLauncher::scan_all_users(scan_request()).map_err(broker_error)?;
                serde_json::from_str(&response.message).map_err(|error| CoordinatorError {
                    code: "broker_invalid_snapshot".to_owned(),
                    message: error.to_string(),
                })
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
            DeploymentRoute::AllUsersBroker => {
                let mut packages = Vec::with_capacity(1 + package.dependencies.len());
                packages.push(package.main.clone());
                packages.extend(package.dependencies.clone());
                let request = BrokerRequest {
                    protocol_version: crate::broker_protocol::BROKER_PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    operation: BrokerOperation::InstallAllUsers,
                    parent_pid: current_pid(),
                    session_id: current_session(),
                    nonce: uuid::Uuid::new_v4().to_string(),
                    payload: BrokerPayload::Install { packages },
                };
                BrokerLauncher::install_all_users(request).map_err(broker_error)?;
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
            DeploymentRoute::AllUsersBroker => {
                let request = BrokerRequest {
                    protocol_version: crate::broker_protocol::BROKER_PROTOCOL_VERSION,
                    request_id: uuid::Uuid::new_v4().to_string(),
                    operation: BrokerOperation::UninstallAllUsers,
                    parent_pid: current_pid(),
                    session_id: current_session(),
                    nonce: uuid::Uuid::new_v4().to_string(),
                    payload: BrokerPayload::Uninstall {
                        target: AllUsersRemovalRequest {
                            package_family_name: package_family_name.to_owned(),
                            package_full_names: package_full_names.to_vec(),
                        },
                    },
                };
                BrokerLauncher::uninstall_all_users(request).map_err(broker_error)?;
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

fn scan_request() -> BrokerRequest {
    BrokerRequest::scan(
        uuid::Uuid::new_v4().to_string(),
        current_pid(),
        current_session(),
        uuid::Uuid::new_v4().to_string(),
    )
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

fn current_pid() -> u32 {
    #[cfg(windows)]
    {
        unsafe { windows::Win32::System::Threading::GetCurrentProcessId() }
    }
    #[cfg(not(windows))]
    {
        std::process::id()
    }
}

fn current_session() -> u32 {
    #[cfg(windows)]
    {
        let mut session = 0;
        if unsafe {
            windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(current_pid(), &mut session)
        }
        .is_ok()
        {
            return session;
        }
    }
    1
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

fn broker_error(error: BrokerLaunchError) -> CoordinatorError {
    CoordinatorError {
        code: match error {
            BrokerLaunchError::UacCancelled => "uac_cancelled",
            BrokerLaunchError::CallerContextMismatch => "caller_context_mismatch",
            BrokerLaunchError::Timeout => "broker_timeout",
            BrokerLaunchError::UnsupportedPlatform => "unsupported_platform",
            BrokerLaunchError::InvalidRequest(_) => "broker_invalid_request",
            BrokerLaunchError::BrokerRejected => "broker_rejected",
            BrokerLaunchError::BrokerUnavailable => "broker_unavailable",
            BrokerLaunchError::PipeFailure => "broker_pipe_failure",
        }
        .to_owned(),
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
