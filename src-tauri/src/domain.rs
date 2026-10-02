use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X64,
    Arm64,
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
    pub version: String,
    pub architecture: Architecture,
    pub language: Option<String>,
    pub market: String,
    pub format: PackageFormat,
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
