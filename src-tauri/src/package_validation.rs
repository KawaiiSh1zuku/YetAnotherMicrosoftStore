use std::{
    fmt,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::broker_protocol::{PackageFileRequest, PackageIdentity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedPackageFile {
    pub path: PathBuf,
    pub sha256_hex: String,
    pub identity: Option<PackageIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedPackageSet {
    pub main: PackageFileRequest,
    pub dependencies: Vec<PackageFileRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    InvalidPath,
    UnsupportedPackageFormat,
    HashMismatch,
    IdentityMismatch,
    SignatureInvalid,
    RootEscape,
    Io(String),
    Manifest(String),
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => formatter.write_str("invalid package path"),
            Self::UnsupportedPackageFormat => formatter.write_str("unsupported package format"),
            Self::HashMismatch => formatter.write_str("package hash mismatch"),
            Self::IdentityMismatch => formatter.write_str("package identity mismatch"),
            Self::SignatureInvalid => formatter.write_str("package signature is invalid"),
            Self::RootEscape => formatter.write_str("protected root escape"),
            Self::Io(message) | Self::Manifest(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ValidationError {}

pub fn copy_and_verify_to_protected_root(
    source: &PackageFileRequest,
    protected_root: &Path,
) -> Result<ProtectedPackageFile, ValidationError> {
    validate_source_path(&source.path)?;
    let source_path = source
        .path
        .canonicalize()
        .map_err(|error| ValidationError::Io(error.to_string()))?;
    if !source_path.is_file() || has_reparse_component(&source_path)? {
        return Err(ValidationError::InvalidPath);
    }

    fs::create_dir_all(protected_root).map_err(|error| ValidationError::Io(error.to_string()))?;
    let root = protected_root
        .canonicalize()
        .map_err(|error| ValidationError::Io(error.to_string()))?;
    if root.is_file() || has_reparse_component(&root)? {
        return Err(ValidationError::RootEscape);
    }

    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or(ValidationError::UnsupportedPackageFormat)?
        .to_ascii_lowercase();
    let destination = root.join(format!(
        "package-{}.{}",
        &source.sha256_hex[..16],
        extension
    ));
    if !destination.starts_with(&root) {
        return Err(ValidationError::RootEscape);
    }

    let mut input =
        File::open(&source_path).map_err(|error| ValidationError::Io(error.to_string()))?;
    let mut output =
        File::create(&destination).map_err(|error| ValidationError::Io(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = input
            .read(&mut buffer)
            .map_err(|error| ValidationError::Io(error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        output
            .write_all(&buffer[..read])
            .map_err(|error| ValidationError::Io(error.to_string()))?;
    }
    output
        .sync_all()
        .map_err(|error| ValidationError::Io(error.to_string()))?;

    let actual_hash = format!("{:x}", hasher.finalize());
    if !actual_hash.eq_ignore_ascii_case(&source.sha256_hex) {
        let _ = fs::remove_file(&destination);
        return Err(ValidationError::HashMismatch);
    }

    let identity = read_manifest_identity(&destination, source.expected_identity.as_ref())?;
    Ok(ProtectedPackageFile {
        path: destination,
        sha256_hex: actual_hash,
        identity,
    })
}

pub fn verify_package_request(source: &PackageFileRequest) -> Result<(), ValidationError> {
    validate_source_path(&source.path)?;
    let source_path = source
        .path
        .canonicalize()
        .map_err(|error| ValidationError::Io(error.to_string()))?;
    if !source_path.is_file() || has_reparse_component(&source_path)? {
        return Err(ValidationError::InvalidPath);
    }
    let mut file =
        File::open(&source_path).map_err(|error| ValidationError::Io(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| ValidationError::Io(error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual_hash = format!("{:x}", hasher.finalize());
    if !actual_hash.eq_ignore_ascii_case(&source.sha256_hex) {
        return Err(ValidationError::HashMismatch);
    }
    read_manifest_identity(&source_path, source.expected_identity.as_ref())?;
    verify_package_signature(&source_path)
}

#[cfg(windows)]
pub fn verify_package_signature(path: &Path) -> Result<(), ValidationError> {
    use std::os::windows::ffi::OsStrExt;

    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::HWND,
            Security::WinTrust::{
                WinVerifyTrustEx, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA,
                WINTRUST_DATA_0, WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE,
                WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
            },
        },
    };

    validate_source_path(path)?;
    if !path.is_file() {
        return Err(ValidationError::InvalidPath);
    }
    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide_path.as_ptr()),
        ..Default::default()
    };
    let mut trust_data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut file_info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;

    // SAFETY: The UTF-16 path and both WinTrust structures remain alive for both calls.
    // Their size, tagged union choice, and file pointer are initialized as required by WinTrust.
    let status = unsafe { WinVerifyTrustEx(HWND::default(), &mut action, &mut trust_data) };
    trust_data.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: This closes only the state handle created by the preceding verification call.
    let _ = unsafe { WinVerifyTrustEx(HWND::default(), &mut action, &mut trust_data) };

    if status == 0 {
        Ok(())
    } else {
        Err(ValidationError::SignatureInvalid)
    }
}

#[cfg(not(windows))]
pub fn verify_package_signature(_path: &Path) -> Result<(), ValidationError> {
    Err(ValidationError::SignatureInvalid)
}

fn validate_source_path(path: &Path) -> Result<(), ValidationError> {
    if !path.is_absolute() || is_unc_or_device_path(path) {
        return Err(ValidationError::InvalidPath);
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    if !extension.as_deref().is_some_and(|value| {
        matches!(
            value,
            "msix" | "appx" | "msixbundle" | "appxbundle" | "eappx" | "eappxbundle"
        )
    }) {
        return Err(ValidationError::UnsupportedPackageFormat);
    }
    Ok(())
}

#[cfg(windows)]
fn is_unc_or_device_path(path: &Path) -> bool {
    use std::path::{Component, Prefix};

    match path.components().next() {
        Some(Component::Prefix(prefix)) => {
            !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        }
        _ => true,
    }
}

#[cfg(not(windows))]
fn is_unc_or_device_path(path: &Path) -> bool {
    let value = path.to_string_lossy();
    value.starts_with(r"\\") || value.starts_with(r"\\?\") || value.starts_with(r"\\.\")
}

fn has_reparse_component(path: &Path) -> Result<bool, ValidationError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|error| ValidationError::Io(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn read_manifest_identity(
    package_path: &Path,
    expected: Option<&PackageIdentity>,
) -> Result<Option<PackageIdentity>, ValidationError> {
    let Some(expected) = expected else {
        return Ok(None);
    };
    let file = File::open(package_path).map_err(|error| ValidationError::Io(error.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| ValidationError::Manifest(error.to_string()))?;
    let (manifest_index, bundle) = match archive.index_for_name("AppxManifest.xml") {
        Some(index) => (index, false),
        None => (
            archive
                .index_for_name("AppxMetadata/AppxBundleManifest.xml")
                .ok_or_else(|| {
                    ValidationError::Manifest("package manifest is missing".to_owned())
                })?,
            true,
        ),
    };
    let mut manifest = archive
        .by_index(manifest_index)
        .map_err(|error| ValidationError::Manifest(error.to_string()))?;
    let mut xml = String::new();
    manifest
        .read_to_string(&mut xml)
        .map_err(|error| ValidationError::Manifest(error.to_string()))?;
    let document = roxmltree::Document::parse(&xml)
        .map_err(|error| ValidationError::Manifest(error.to_string()))?;
    let identity_node = document
        .descendants()
        .find(|node| node.has_tag_name("Identity"))
        .ok_or_else(|| ValidationError::Manifest("Identity element missing".to_owned()))?;
    let identity = PackageIdentity {
        name: identity_node
            .attribute("Name")
            .unwrap_or_default()
            .to_owned(),
        publisher: identity_node
            .attribute("Publisher")
            .unwrap_or_default()
            .to_owned(),
        version: parse_version(identity_node.attribute("Version").unwrap_or_default())?,
        architecture: if bundle {
            "neutral".to_owned()
        } else {
            identity_node
                .attribute("ProcessorArchitecture")
                .unwrap_or_default()
                .to_owned()
        },
        resource_id: if bundle {
            String::new()
        } else {
            identity_node
                .attribute("ResourceId")
                .unwrap_or_default()
                .to_owned()
        },
    };
    if &identity != expected {
        return Err(ValidationError::IdentityMismatch);
    }
    Ok(Some(identity))
}

fn parse_version(value: &str) -> Result<[u16; 4], ValidationError> {
    let parts = value
        .split('.')
        .map(|part| part.parse::<u16>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ValidationError::Manifest("invalid package version".to_owned()))?;
    if parts.len() != 4 {
        return Err(ValidationError::Manifest(
            "invalid package version".to_owned(),
        ));
    }
    Ok([parts[0], parts[1], parts[2], parts[3]])
}

impl From<io::Error> for ValidationError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}
