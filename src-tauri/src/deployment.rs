use std::{fmt, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    broker_protocol::PackageIdentity,
    package_validation::{copy_and_verify_to_protected_root, VerifiedPackageSet},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentScope {
    CurrentUser,
    AllUsers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityStatus {
    Available,
    RequiresElevation,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentProbe {
    pub package_manager: CapabilityStatus,
    pub current_user: CapabilityStatus,
    pub all_users: CapabilityStatus,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentError {
    UnsupportedPlatform,
    WindowsApi(String),
    InvalidPackagePath(String),
    DeploymentFailed {
        operation: &'static str,
        message: String,
    },
    MissingPackageIdentity,
    PackageNotFound,
}

impl fmt::Display for DeploymentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => formatter.write_str("deployment probing requires Windows"),
            Self::WindowsApi(message) => formatter.write_str(message),
            Self::InvalidPackagePath(message) => formatter.write_str(message),
            Self::DeploymentFailed { operation, message } => {
                write!(formatter, "{operation} failed: {message}")
            }
            Self::MissingPackageIdentity => {
                formatter.write_str("package identity is required for all-users deployment")
            }
            Self::PackageNotFound => formatter.write_str("staged package was not found"),
        }
    }
}

impl std::error::Error for DeploymentError {}

pub struct WindowsDeploymentBackend;

impl WindowsDeploymentBackend {
    pub fn probe() -> Result<DeploymentProbe, DeploymentError> {
        #[cfg(windows)]
        {
            windows::Management::Deployment::PackageManager::new()
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;

            Ok(DeploymentProbe {
                package_manager: CapabilityStatus::Available,
                current_user: CapabilityStatus::Available,
                all_users: CapabilityStatus::RequiresElevation,
                notes: vec![
                    "PackageManager activation succeeded; no package was installed by the probe."
                        .to_owned(),
                    "All-users deployment remains gated behind a reviewed broker/UAC path."
                        .to_owned(),
                ],
            })
        }

        #[cfg(not(windows))]
        {
            Err(DeploymentError::UnsupportedPlatform)
        }
    }

    /// Install a signed MSIX/AppX package for the calling user.
    pub fn install_current_user(
        package: &VerifiedPackageSet,
    ) -> Result<DeploymentOutcome, DeploymentError> {
        #[cfg(windows)]
        {
            let package_uri = file_uri(&package.main.path)?;
            let dependency_uris: Vec<Option<windows::Foundation::Uri>> = package
                .dependencies
                .iter()
                .map(|dependency| file_uri(&dependency.path).map(Some))
                .collect::<Result<_, _>>()?;
            let dependency_uris = (!dependency_uris.is_empty())
                .then(|| windows_collections::IIterable::from(dependency_uris));
            let manager = windows::Management::Deployment::PackageManager::new()
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
            let result = manager
                .AddPackageAsync(
                    &package_uri,
                    dependency_uris.as_ref(),
                    windows::Management::Deployment::DeploymentOptions::None,
                )
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?
                .join()
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;

            deployment_result("current-user install", result)?;
            Ok(DeploymentOutcome {
                operation: "current-user install".to_owned(),
                package_family_name: None,
                package_full_names: Vec::new(),
            })
        }

        #[cfg(not(windows))]
        {
            let _ = package;
            Err(DeploymentError::UnsupportedPlatform)
        }
    }

    /// Remove a package full name from the calling user's package inventory.
    pub fn remove_current_user(
        package_full_name: &str,
    ) -> Result<DeploymentOutcome, DeploymentError> {
        #[cfg(windows)]
        {
            if package_full_name.trim().is_empty() {
                return Err(DeploymentError::InvalidPackagePath(
                    "package full name must not be empty".to_owned(),
                ));
            }

            let manager = windows::Management::Deployment::PackageManager::new()
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
            let result = manager
                .RemovePackageAsync(&windows::core::HSTRING::from(package_full_name))
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?
                .join()
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;

            deployment_result("current-user uninstall", result)?;
            Ok(DeploymentOutcome {
                operation: "current-user uninstall".to_owned(),
                package_family_name: None,
                package_full_names: vec![package_full_name.to_owned()],
            })
        }

        #[cfg(not(windows))]
        {
            let _ = package_full_name;
            Err(DeploymentError::UnsupportedPlatform)
        }
    }

    pub fn stage_and_provision_all_users(
        package: &VerifiedPackageSet,
        protected_root: &Path,
    ) -> Result<DeploymentOutcome, DeploymentError> {
        #[cfg(windows)]
        {
            let main = copy_and_verify_to_protected_root(&package.main, protected_root)
                .map_err(|error| DeploymentError::InvalidPackagePath(error.to_string()))?;
            let dependencies = package
                .dependencies
                .iter()
                .map(|dependency| {
                    copy_and_verify_to_protected_root(dependency, protected_root)
                        .map_err(|error| DeploymentError::InvalidPackagePath(error.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let package_uri = file_uri(&main.path)?;
            let dependency_uris: Vec<Option<windows::Foundation::Uri>> = dependencies
                .iter()
                .map(|dependency| file_uri(&dependency.path).map(Some))
                .collect::<Result<_, _>>()?;
            let dependency_uris = (!dependency_uris.is_empty())
                .then(|| windows_collections::IIterable::from(dependency_uris));
            let manager = windows::Management::Deployment::PackageManager::new()
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
            let stage_result = manager
                .StagePackageAsync(&package_uri, dependency_uris.as_ref())
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?
                .join()
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
            deployment_result("all-users stage", stage_result)?;

            let identity = package
                .main
                .expected_identity
                .as_ref()
                .ok_or(DeploymentError::MissingPackageIdentity)?;
            let package_family_name = find_package_family_name(&manager, identity)?;
            let provision_result = manager
                .ProvisionPackageForAllUsersAsync(&windows::core::HSTRING::from(
                    &package_family_name,
                ))
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?
                .join()
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
            deployment_result("all-users provision", provision_result)?;

            Ok(DeploymentOutcome {
                operation: "all-users provision".to_owned(),
                package_family_name: Some(package_family_name),
                package_full_names: Vec::new(),
            })
        }

        #[cfg(not(windows))]
        {
            let _ = (package, protected_root);
            Err(DeploymentError::UnsupportedPlatform)
        }
    }

    pub fn deprovision_and_remove_all_users(
        package_family_name: &str,
        package_full_names: &[String],
    ) -> Result<DeploymentOutcome, DeploymentError> {
        #[cfg(windows)]
        {
            if package_family_name.trim().is_empty() || package_full_names.is_empty() {
                return Err(DeploymentError::InvalidPackagePath(
                    "all-users removal target must not be empty".to_owned(),
                ));
            }
            let manager = windows::Management::Deployment::PackageManager::new()
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
            let deprovision_result = manager
                .DeprovisionPackageForAllUsersAsync(&windows::core::HSTRING::from(
                    package_family_name,
                ))
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?
                .join()
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
            deployment_result("all-users deprovision", deprovision_result)?;

            for package_full_name in package_full_names {
                let remove_result = manager
                    .RemovePackageWithOptionsAsync(
                        &windows::core::HSTRING::from(package_full_name),
                        windows::Management::Deployment::RemovalOptions::RemoveForAllUsers,
                    )
                    .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?
                    .join()
                    .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
                deployment_result("all-users uninstall", remove_result)?;
            }

            Ok(DeploymentOutcome {
                operation: "all-users uninstall".to_owned(),
                package_family_name: Some(package_family_name.to_owned()),
                package_full_names: package_full_names.to_vec(),
            })
        }

        #[cfg(not(windows))]
        {
            let _ = (package_family_name, package_full_names);
            Err(DeploymentError::UnsupportedPlatform)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentOutcome {
    pub operation: String,
    pub package_family_name: Option<String>,
    pub package_full_names: Vec<String>,
}

#[cfg(windows)]
fn file_uri(path: &Path) -> Result<windows::Foundation::Uri, DeploymentError> {
    if !path.is_absolute() {
        return Err(DeploymentError::InvalidPackagePath(
            "package paths must be absolute".to_owned(),
        ));
    }
    if !path.is_file() {
        return Err(DeploymentError::InvalidPackagePath(format!(
            "package path does not name a file: {}",
            path.display()
        )));
    }

    let path = path
        .canonicalize()
        .map_err(|error| DeploymentError::InvalidPackagePath(error.to_string()))?;
    let mut path = path.to_string_lossy().into_owned();
    if let Some(stripped) = path.strip_prefix("\\\\?\\") {
        path = stripped.to_owned();
    }
    if path.starts_with("UNC\\") {
        return Err(DeploymentError::InvalidPackagePath(
            "UNC package paths are not supported".to_owned(),
        ));
    }
    let path = path.replace('\\', "/");
    let uri = format!("file:///{path}");
    windows::Foundation::Uri::CreateUri(&windows::core::HSTRING::from(uri))
        .map_err(|error| DeploymentError::InvalidPackagePath(error.to_string()))
}

#[cfg(windows)]
fn deployment_result(
    operation: &'static str,
    result: windows::Management::Deployment::DeploymentResult,
) -> Result<(), DeploymentError> {
    let error_text = result
        .ErrorText()
        .map(|value| value.to_string_lossy())
        .unwrap_or_default();
    let extended_error_code = result
        .ExtendedErrorCode()
        .map(|value| value.0)
        .unwrap_or_default();

    if extended_error_code != 0 || !error_text.is_empty() {
        let message = if error_text.is_empty() {
            format!("HRESULT 0x{extended_error_code:08X}")
        } else if extended_error_code == 0 {
            error_text
        } else {
            format!("HRESULT 0x{extended_error_code:08X}: {error_text}")
        };
        Err(DeploymentError::DeploymentFailed { operation, message })
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn find_package_family_name(
    manager: &windows::Management::Deployment::PackageManager,
    identity: &PackageIdentity,
) -> Result<String, DeploymentError> {
    let packages = manager
        .FindPackagesByNamePublisher(
            &windows::core::HSTRING::from(&identity.name),
            &windows::core::HSTRING::from(&identity.publisher),
        )
        .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
    for package in packages {
        let id = package
            .Id()
            .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
        let version = id
            .Version()
            .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?;
        let resource_id = id
            .ResourceId()
            .map_err(|error| DeploymentError::WindowsApi(error.to_string()))?
            .to_string_lossy();
        if [
            version.Major,
            version.Minor,
            version.Build,
            version.Revision,
        ] == identity.version
            && resource_id == identity.resource_id
        {
            return id
                .FamilyName()
                .map(|value| value.to_string_lossy())
                .map_err(|error| DeploymentError::WindowsApi(error.to_string()));
        }
    }
    Err(DeploymentError::PackageNotFound)
}
