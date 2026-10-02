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
    let mut manifest = archive
        .by_name("AppxManifest.xml")
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
        architecture: identity_node
            .attribute("ProcessorArchitecture")
            .unwrap_or_default()
            .to_owned(),
        resource_id: identity_node
            .attribute("ResourceId")
            .unwrap_or_default()
            .to_owned(),
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
