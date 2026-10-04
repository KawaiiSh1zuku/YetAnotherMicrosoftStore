use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageApplicationError {
    InvalidPackageFamilyName,
    NotInstalled,
    NotLaunchable,
    UnsupportedPlatform,
    WindowsApi(String),
}

impl fmt::Display for PackageApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPackageFamilyName => formatter.write_str("invalid package family name"),
            Self::NotInstalled => formatter.write_str("package is not installed for this user"),
            Self::NotLaunchable => formatter.write_str("package has no launchable app entry"),
            Self::UnsupportedPlatform => formatter.write_str("package launch requires Windows"),
            Self::WindowsApi(message) => {
                write!(formatter, "Windows package launch failed: {message}")
            }
        }
    }
}

impl std::error::Error for PackageApplicationError {}

pub struct PackageApplicationManager;

impl PackageApplicationManager {
    pub fn is_launchable(package_family_name: &str) -> Result<bool, PackageApplicationError> {
        validate_package_family_name(package_family_name)?;

        #[cfg(windows)]
        {
            Ok(!app_entries(package_family_name)?.is_empty())
        }

        #[cfg(not(windows))]
        {
            Err(PackageApplicationError::UnsupportedPlatform)
        }
    }

    pub fn launch(package_family_name: &str) -> Result<(), PackageApplicationError> {
        validate_package_family_name(package_family_name)?;

        #[cfg(windows)]
        {
            let mut entries = app_entries(package_family_name)?;
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let (_, entry) = entries
                .into_iter()
                .next()
                .ok_or(PackageApplicationError::NotLaunchable)?;
            let launched = entry
                .LaunchAsync()
                .and_then(|operation| operation.join())
                .map_err(map_windows_error)?;
            if launched {
                Ok(())
            } else {
                Err(PackageApplicationError::NotLaunchable)
            }
        }

        #[cfg(not(windows))]
        {
            Err(PackageApplicationError::UnsupportedPlatform)
        }
    }
}

fn validate_package_family_name(value: &str) -> Result<(), PackageApplicationError> {
    valid_package_family_name(value)
        .then_some(())
        .ok_or(PackageApplicationError::InvalidPackageFamilyName)
}

fn valid_package_family_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[cfg(test)]
fn select_stable_entry_id(mut entries: Vec<String>) -> Option<String> {
    entries.sort();
    entries.into_iter().next()
}

#[cfg(windows)]
fn app_entries(
    package_family_name: &str,
) -> Result<Vec<(String, windows::ApplicationModel::Core::AppListEntry)>, PackageApplicationError> {
    let manager =
        windows::Management::Deployment::PackageManager::new().map_err(map_windows_error)?;
    let packages = manager
        .FindPackagesByUserSecurityIdPackageFamilyName(
            &windows::core::HSTRING::new(),
            &windows::core::HSTRING::from(package_family_name),
        )
        .map_err(map_windows_error)?;
    let mut package_found = false;
    let mut entries = Vec::new();
    for package in packages {
        package_found = true;
        let package_entries = package
            .GetAppListEntriesAsync()
            .and_then(|operation| operation.join())
            .map_err(map_windows_error)?;
        for entry in package_entries {
            let app_user_model_id = entry
                .AppUserModelId()
                .map_err(map_windows_error)?
                .to_string_lossy();
            entries.push((app_user_model_id, entry));
        }
    }
    if !package_found {
        return Err(PackageApplicationError::NotInstalled);
    }
    Ok(entries)
}

#[cfg(windows)]
fn map_windows_error(error: windows::core::Error) -> PackageApplicationError {
    PackageApplicationError::WindowsApi(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{select_stable_entry_id, valid_package_family_name};

    #[test]
    fn package_family_name_validation_is_closed() {
        assert!(valid_package_family_name(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe"
        ));
        assert!(!valid_package_family_name(""));
        assert!(!valid_package_family_name("Package Family"));
        assert!(!valid_package_family_name("Package/Family"));
    }

    #[test]
    fn app_entry_selection_is_empty_for_no_entries() {
        assert_eq!(select_stable_entry_id(Vec::new()), None);
    }

    #[test]
    fn app_entry_selection_uses_the_only_entry() {
        assert_eq!(
            select_stable_entry_id(vec!["Example.App_main".to_owned()]),
            Some("Example.App_main".to_owned())
        );
    }

    #[test]
    fn app_entry_selection_is_stable_for_multiple_entries() {
        assert_eq!(
            select_stable_entry_id(vec![
                "Example.App_zeta".to_owned(),
                "Example.App_alpha".to_owned(),
            ]),
            Some("Example.App_alpha".to_owned())
        );
    }
}
