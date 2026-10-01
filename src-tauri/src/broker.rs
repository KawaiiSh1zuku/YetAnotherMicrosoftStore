use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::deployment::DeploymentScope;

const SUPPORTED_PACKAGE_EXTENSIONS: &[&str] = &[
    "msix",
    "appx",
    "msixbundle",
    "appxbundle",
    "eappx",
    "eappxbundle",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerRequest {
    pub scope: DeploymentScope,
    pub package_path: PathBuf,
    pub dependency_paths: Vec<PathBuf>,
}

impl BrokerRequest {
    pub fn new(
        scope: DeploymentScope,
        package_path: PathBuf,
        dependency_paths: Vec<PathBuf>,
    ) -> Self {
        Self {
            scope,
            package_path,
            dependency_paths,
        }
    }

    /// Validate the inert M0 request shape.
    ///
    /// This is not an authorization or deployment check. Before any IPC/UAC
    /// wiring, the broker must add trusted staging-root, identity, signature,
    /// hash, and safe file-opening checks.
    pub fn validate(&self) -> Result<(), BrokerValidationError> {
        if self.scope == DeploymentScope::AllUsers {
            return Err(BrokerValidationError::AllUsersBrokerNotReady);
        }

        validate_package_path(&self.package_path)?;
        for dependency in &self.dependency_paths {
            validate_package_path(dependency)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerValidationError {
    AllUsersBrokerNotReady,
    NonAbsolutePath,
    UnsupportedPackageFormat,
}

impl fmt::Display for BrokerValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllUsersBrokerNotReady => {
                formatter.write_str("all-users deployment requires the reviewed UAC broker")
            }
            Self::NonAbsolutePath => formatter.write_str("broker paths must be absolute"),
            Self::UnsupportedPackageFormat => {
                formatter.write_str("broker accepts only supported MSIX/AppX package formats")
            }
        }
    }
}

impl std::error::Error for BrokerValidationError {}

fn validate_package_path(path: &std::path::Path) -> Result<(), BrokerValidationError> {
    if !path.is_absolute() {
        return Err(BrokerValidationError::NonAbsolutePath);
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);

    if extension
        .as_deref()
        .is_some_and(|value| SUPPORTED_PACKAGE_EXTENSIONS.contains(&value))
    {
        Ok(())
    } else {
        Err(BrokerValidationError::UnsupportedPackageFormat)
    }
}
