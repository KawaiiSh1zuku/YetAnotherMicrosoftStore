use serde::{Deserialize, Serialize};

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
}

impl std::fmt::Display for DeploymentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedPlatform => formatter.write_str("deployment probing requires Windows"),
            Self::WindowsApi(message) => formatter.write_str(message),
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
}
