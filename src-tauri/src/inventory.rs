use std::{
    collections::HashSet,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InventorySource {
    CurrentUser,
    AllUsersElevated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackageKind {
    Main,
    Framework,
    Resource,
    Optional,
    Bundle,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageInventoryRecord {
    pub identity_name: String,
    pub publisher: String,
    pub package_family_name: String,
    pub package_full_name: String,
    pub version: [u16; 4],
    pub architecture: String,
    pub resource_id: String,
    pub package_kind: PackageKind,
    pub signature_kind: String,
    pub status: String,
    pub installed_for_current_user: bool,
    pub installed_user_count: u32,
    pub has_other_users: bool,
    pub provisioned_for_future_users: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventorySnapshot {
    pub source: InventorySource,
    pub captured_at: String,
    pub os_build: String,
    pub complete: bool,
    pub records: Vec<PackageInventoryRecord>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InventoryError {
    UnsupportedPlatform,
    AccessDenied,
    WindowsApi(String),
}

impl fmt::Display for InventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => formatter.write_str("package inventory requires Windows"),
            Self::AccessDenied => formatter.write_str("package inventory requires elevation"),
            Self::WindowsApi(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for InventoryError {}

pub struct WindowsInventory;

impl WindowsInventory {
    pub fn scan_current_user() -> Result<InventorySnapshot, InventoryError> {
        #[cfg(windows)]
        {
            let manager = windows::Management::Deployment::PackageManager::new()
                .map_err(map_windows_error)?;
            let packages = manager
                .FindPackagesByUserSecurityId(&windows::core::HSTRING::new())
                .map_err(map_windows_error)?;
            let records = packages
                .into_iter()
                .map(|package| package_record(&package, true, 1, false))
                .collect::<Result<Vec<_>, _>>()?;

            Ok(snapshot(
                InventorySource::CurrentUser,
                true,
                records,
                Vec::new(),
            ))
        }

        #[cfg(not(windows))]
        {
            Err(InventoryError::UnsupportedPlatform)
        }
    }

    pub fn scan_all_users() -> Result<InventorySnapshot, InventoryError> {
        #[cfg(windows)]
        {
            let current = Self::scan_current_user()?;
            let current_names = current
                .records
                .iter()
                .map(|record| record.package_full_name.clone())
                .collect::<HashSet<_>>();
            let manager = windows::Management::Deployment::PackageManager::new()
                .map_err(map_windows_error)?;
            let packages = manager.FindPackages().map_err(map_windows_error)?;
            let mut warnings = Vec::new();
            let mut complete = true;
            let provisioned_names = match manager.FindProvisionedPackages() {
                Ok(provisioned) => provisioned
                    .into_iter()
                    .filter_map(|package| package.Id().ok()?.FullName().ok())
                    .map(|name| name.to_string_lossy())
                    .collect::<HashSet<_>>(),
                Err(error) => {
                    complete = false;
                    warnings.push(format!("FindProvisionedPackages failed: {error}"));
                    HashSet::new()
                }
            };

            let mut records = Vec::new();
            for package in packages {
                let id = package.Id().map_err(map_windows_error)?;
                let full_name = id.FullName().map_err(map_windows_error)?.to_string_lossy();
                let installed_for_current_user = current_names.contains(&full_name);
                let (installed_user_count, user_query_ok) = match manager
                    .FindUsers(&windows::core::HSTRING::from(&full_name))
                {
                    Ok(users) => {
                        let count = users
                            .into_iter()
                            .filter_map(|user| user.InstallState().ok())
                            .filter(|state| {
                                *state
                                    == windows::Management::Deployment::PackageInstallState::Installed
                            })
                            .count() as u32;
                        (count, true)
                    }
                    Err(error) => {
                        warnings.push(format!("FindUsers failed for package: {error}"));
                        (0, false)
                    }
                };
                if !user_query_ok {
                    complete = false;
                }
                records.push(package_record(
                    &package,
                    installed_for_current_user,
                    installed_user_count,
                    provisioned_names.contains(&full_name),
                )?);
            }

            Ok(snapshot(
                InventorySource::AllUsersElevated,
                complete,
                records,
                warnings,
            ))
        }

        #[cfg(not(windows))]
        {
            Err(InventoryError::UnsupportedPlatform)
        }
    }
}

fn snapshot(
    source: InventorySource,
    complete: bool,
    records: Vec<PackageInventoryRecord>,
    warnings: Vec<String>,
) -> InventorySnapshot {
    InventorySnapshot {
        source,
        captured_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs().to_string())
            .unwrap_or_else(|_| "0".to_owned()),
        os_build: std::env::consts::OS.to_owned(),
        complete,
        records,
        warnings,
    }
}

#[cfg(windows)]
fn package_record(
    package: &windows::ApplicationModel::Package,
    installed_for_current_user: bool,
    installed_user_count: u32,
    provisioned_for_future_users: bool,
) -> Result<PackageInventoryRecord, InventoryError> {
    let id = package.Id().map_err(map_windows_error)?;
    let version = id.Version().map_err(map_windows_error)?;
    let package_kind = if package.IsResourcePackage().map_err(map_windows_error)? {
        PackageKind::Resource
    } else if package.IsFramework().map_err(map_windows_error)? {
        PackageKind::Framework
    } else {
        PackageKind::Main
    };
    let full_name = id.FullName().map_err(map_windows_error)?.to_string_lossy();
    Ok(PackageInventoryRecord {
        identity_name: id.Name().map_err(map_windows_error)?.to_string_lossy(),
        publisher: id.Publisher().map_err(map_windows_error)?.to_string_lossy(),
        package_family_name: id
            .FamilyName()
            .map_err(map_windows_error)?
            .to_string_lossy(),
        package_full_name: full_name,
        version: [
            version.Major,
            version.Minor,
            version.Build,
            version.Revision,
        ],
        architecture: architecture_name(id.Architecture().map_err(map_windows_error)?),
        resource_id: id
            .ResourceId()
            .map_err(map_windows_error)?
            .to_string_lossy(),
        package_kind,
        signature_kind: signature_name(package.SignatureKind().map_err(map_windows_error)?),
        status: format!("{:?}", package.Status().map_err(map_windows_error)?),
        installed_for_current_user,
        installed_user_count,
        has_other_users: installed_user_count > u32::from(installed_for_current_user),
        provisioned_for_future_users,
    })
}

#[cfg(windows)]
fn architecture_name(architecture: windows::System::ProcessorArchitecture) -> String {
    use windows::System::ProcessorArchitecture;
    match architecture {
        ProcessorArchitecture::X86 => "x86",
        ProcessorArchitecture::Arm => "arm",
        ProcessorArchitecture::X64 => "x64",
        ProcessorArchitecture::Neutral => "neutral",
        ProcessorArchitecture::Arm64 => "arm64",
        ProcessorArchitecture::X86OnArm64 => "x86-on-arm64",
        ProcessorArchitecture::Unknown => "unknown",
        _ => "unknown",
    }
    .to_owned()
}

#[cfg(windows)]
fn signature_name(signature: windows::ApplicationModel::PackageSignatureKind) -> String {
    use windows::ApplicationModel::PackageSignatureKind;
    match signature {
        PackageSignatureKind::None => "None",
        PackageSignatureKind::Developer => "Developer",
        PackageSignatureKind::Enterprise => "Enterprise",
        PackageSignatureKind::Store => "Store",
        PackageSignatureKind::System => "System",
        _ => "Unknown",
    }
    .to_owned()
}

#[cfg(windows)]
fn map_windows_error(error: windows::core::Error) -> InventoryError {
    if error.code().0 as u32 == 0x8007_0005 {
        InventoryError::AccessDenied
    } else {
        InventoryError::WindowsApi(error.to_string())
    }
}
