use std::{fmt, str::FromStr};

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageVersion([u16; 4]);

impl PackageVersion {
    pub const fn new(major: u16, minor: u16, build: u16, revision: u16) -> Self {
        Self([major, minor, build, revision])
    }

    pub const fn components(self) -> [u16; 4] {
        self.0
    }

    pub const fn from_packed(value: u64) -> Self {
        Self([
            (value >> 48) as u16,
            (value >> 32) as u16,
            (value >> 16) as u16,
            value as u16,
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageVersionParseError;

impl fmt::Display for PackageVersionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("package version must contain four unsigned 16-bit components")
    }
}

impl std::error::Error for PackageVersionParseError {}

impl FromStr for PackageVersion {
    type Err = PackageVersionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut components = value.split('.');
        let mut parsed = [0_u16; 4];
        for component in &mut parsed {
            *component = components
                .next()
                .ok_or(PackageVersionParseError)?
                .parse()
                .map_err(|_| PackageVersionParseError)?;
        }
        if components.next().is_some() {
            return Err(PackageVersionParseError);
        }
        Ok(Self(parsed))
    }
}

impl fmt::Display for PackageVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}.{}.{}.{}",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

impl Serialize for PackageVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PackageVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X64,
    Arm64,
    Arm,
    X86,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageFormat {
    Msix,
    Appx,
    MsixBundle,
    AppxBundle,
    Eappx,
    EappxBundle,
    Msixvc,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageKind {
    Main,
    Framework,
    Resource,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallSource {
    MicrosoftStore,
    ThisClient,
    Other,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageRecord {
    pub update_id: String,
    pub product_id: String,
    pub package_family_name: Option<String>,
    pub package_moniker: String,
    pub identity_name: Option<String>,
    pub publisher: Option<String>,
    pub resource_id: Option<String>,
    pub package_kind: PackageKind,
    pub version: PackageVersion,
    pub architecture: Architecture,
    pub language: Option<String>,
    pub market: String,
    pub format: PackageFormat,
    pub minimum_os_version: Option<PackageVersion>,
    pub is_neutral: Option<bool>,
    pub content_id: Option<String>,
    pub file_size: Option<u64>,
    pub sha256: Option<String>,
    pub install_source: InstallSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductRecord {
    pub product_id: String,
    pub package_family_name: Option<String>,
    pub title: Option<String>,
    pub publisher: Option<String>,
    pub market: String,
    pub languages: Vec<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Prerequisite,
    Bundled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDependency {
    pub source_update_id: String,
    pub target_update_id: String,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheState {
    Partial,
    Verified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub cache_key: String,
    pub job_id: Option<String>,
    pub update_id: String,
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub state: CacheState,
    pub last_accessed_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    Disabled,
    System,
    Http,
    Https,
    Socks5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyCredentialPolicy {
    PromptEveryTime,
    WindowsCredentialManager,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub region: String,
    pub market: String,
    pub preferred_architectures: Vec<Architecture>,
    pub preferred_languages: Vec<String>,
    pub proxy_mode: ProxyMode,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
    pub proxy_credentials: ProxyCredentialPolicy,
    pub cache_enabled: bool,
    pub cache_directory: String,
    pub max_cache_bytes: u64,
    pub retention_days: u32,
    pub keep_installed_payloads: bool,
    pub max_concurrent_downloads: u32,
    #[serde(default)]
    pub theme: ThemeMode,
    #[serde(default)]
    pub diagnostics_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallObservation {
    pub package_family_name: String,
    pub product_id: Option<String>,
    pub source: InstallSource,
    pub observed_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticOperation {
    Catalog,
    Resolve,
    Select,
    Download,
    Verify,
    Deploy,
    Inventory,
    Storage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    pub job_id: Option<String>,
    pub code: crate::error::ErrorCode,
    pub stage: Option<crate::jobs::JobStage>,
    pub operation: DiagnosticOperation,
    pub os_error_code: Option<i64>,
    pub retryable: bool,
    pub occurred_at: i64,
}
