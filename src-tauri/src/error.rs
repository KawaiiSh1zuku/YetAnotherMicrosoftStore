use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    applicability::ApplicabilityError,
    catalog::CatalogError,
    deployment_coordinator::CoordinatorError,
    deployment_plan::DeploymentPlanError,
    download::{DownloadError, DownloadTransportError},
    package_validation::ValidationError,
    resolver::ResolverError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    CatalogNotFound,
    CatalogUnavailable,
    LicenseRequired,
    MarketUnavailable,
    NoCompatiblePackage,
    DependencyUnresolved,
    DownloadFailed,
    DownloadProxyFailed,
    DownloadProxyAuthRequired,
    DownloadTimeout,
    DownloadConnectionFailed,
    DownloadResponseFailed,
    DownloadHttpStatus,
    DownloadRedirectRejected,
    DownloadIoFailed,
    DownloadUrlExpired,
    HashMismatch,
    SignatureInvalid,
    DeploymentDenied,
    DeploymentFailed,
    PackageInUse,
    StoreEntitlementMissing,
    StoreChannelUnavailable,
    SourceIdentityMismatch,
    VersionAheadOfCatalog,
    MsixvcCapabilityUnavailable,
    UnsupportedPackageType,
    PackageNotInstalled,
}

impl ErrorCode {
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::CatalogNotFound => "errors.catalogNotFound",
            Self::CatalogUnavailable => "errors.catalogUnavailable",
            Self::LicenseRequired => "errors.licenseRequired",
            Self::MarketUnavailable => "errors.marketUnavailable",
            Self::NoCompatiblePackage => "errors.noCompatiblePackage",
            Self::DependencyUnresolved => "errors.dependencyUnresolved",
            Self::DownloadFailed => "errors.downloadFailed",
            Self::DownloadProxyFailed => "errors.downloadProxyFailed",
            Self::DownloadProxyAuthRequired => "errors.downloadProxyAuthRequired",
            Self::DownloadTimeout => "errors.downloadTimeout",
            Self::DownloadConnectionFailed => "errors.downloadConnectionFailed",
            Self::DownloadResponseFailed => "errors.downloadResponseFailed",
            Self::DownloadHttpStatus => "errors.downloadHttpStatus",
            Self::DownloadRedirectRejected => "errors.downloadRedirectRejected",
            Self::DownloadIoFailed => "errors.downloadIoFailed",
            Self::DownloadUrlExpired => "errors.downloadUrlExpired",
            Self::HashMismatch => "errors.hashMismatch",
            Self::SignatureInvalid => "errors.signatureInvalid",
            Self::DeploymentDenied => "errors.deploymentDenied",
            Self::DeploymentFailed => "errors.deploymentFailed",
            Self::PackageInUse => "errors.packageInUse",
            Self::StoreEntitlementMissing => "errors.storeEntitlementMissing",
            Self::StoreChannelUnavailable => "errors.storeChannelUnavailable",
            Self::SourceIdentityMismatch => "errors.sourceIdentityMismatch",
            Self::VersionAheadOfCatalog => "errors.versionAheadOfCatalog",
            Self::MsixvcCapabilityUnavailable => "errors.msixvcCapabilityUnavailable",
            Self::UnsupportedPackageType => "errors.unsupportedPackageType",
            Self::PackageNotInstalled => "errors.packageNotInstalled",
        }
    }
}

pub const fn classify_deployment_hresult(code: i32) -> ErrorCode {
    match code as u32 {
        0x8007_3D02 => ErrorCode::PackageInUse,
        0x8007_0005 => ErrorCode::DeploymentDenied,
        _ => ErrorCode::DeploymentFailed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryAdvice {
    Never,
    Retry,
    ReResolve,
    ReconcileInventory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafeField {
    Product,
    ProductId,
    PackageUri,
    PackageMoniker,
    UpdateId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SafeErrorDetail {
    Field { field: SafeField },
    HttpStatus { status: u16 },
    Redacted,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurrentSafeErrorDetail {
    Field { field: SafeField },
    HttpStatus { status: u16 },
    Redacted,
}

#[derive(Deserialize)]
struct LegacySafeErrorDetail {
    key: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SafeErrorDetailWire {
    Current(CurrentSafeErrorDetail),
    Legacy(LegacySafeErrorDetail),
}

impl<'de> Deserialize<'de> for SafeErrorDetail {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match SafeErrorDetailWire::deserialize(deserializer)? {
            SafeErrorDetailWire::Current(CurrentSafeErrorDetail::Field { field }) => {
                Self::Field { field }
            }
            SafeErrorDetailWire::Current(CurrentSafeErrorDetail::HttpStatus { status }) => {
                if (100..=599).contains(&status) {
                    Self::HttpStatus { status }
                } else {
                    Self::Redacted
                }
            }
            SafeErrorDetailWire::Current(CurrentSafeErrorDetail::Redacted) => Self::Redacted,
            SafeErrorDetailWire::Legacy(detail) => {
                if detail.key == "field" {
                    safe_field(&detail.value).map_or(Self::Redacted, |field| Self::Field { field })
                } else {
                    Self::Redacted
                }
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppErrorDto {
    pub code: ErrorCode,
    pub message_key: String,
    pub retry: RetryAdvice,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    pub details: Vec<SafeErrorDetail>,
}

impl AppErrorDto {
    pub fn new(code: ErrorCode, retry: RetryAdvice) -> Self {
        Self {
            code,
            message_key: code.message_key().to_owned(),
            retry,
            job_id: None,
            details: Vec::new(),
        }
    }

    pub fn with_safe_detail(mut self, detail: SafeErrorDetail) -> Self {
        self.details.push(detail);
        self
    }
}

fn safe_field(value: &str) -> Option<SafeField> {
    match value {
        "product" => Some(SafeField::Product),
        "productId" => Some(SafeField::ProductId),
        "packageUri" => Some(SafeField::PackageUri),
        "packageMoniker" => Some(SafeField::PackageMoniker),
        "updateId" => Some(SafeField::UpdateId),
        _ => None,
    }
}

impl From<&CatalogError> for AppErrorDto {
    fn from(error: &CatalogError) -> Self {
        match error {
            CatalogError::InvalidUrl { field } => {
                let error = Self::new(ErrorCode::CatalogUnavailable, RetryAdvice::ReResolve);
                safe_field(field).map_or(error.clone(), |field| {
                    error.with_safe_detail(SafeErrorDetail::Field { field })
                })
            }
            CatalogError::MalformedFixture(_)
            | CatalogError::MissingField(_)
            | CatalogError::UnsupportedLocale
            | CatalogError::StoreLib => {
                Self::new(ErrorCode::CatalogUnavailable, RetryAdvice::Retry)
            }
        }
    }
}

impl From<&ResolverError> for AppErrorDto {
    fn from(error: &ResolverError) -> Self {
        match error {
            ResolverError::StoreLib => Self::new(ErrorCode::CatalogUnavailable, RetryAdvice::Retry),
            ResolverError::MissingField(field) => {
                let error = Self::new(ErrorCode::DependencyUnresolved, RetryAdvice::ReResolve);
                safe_field(field).map_or(error.clone(), |field| {
                    error.with_safe_detail(SafeErrorDetail::Field { field })
                })
            }
            ResolverError::MalformedFixture(_)
            | ResolverError::InvalidPackageSize
            | ResolverError::InvalidPackageDigest
            | ResolverError::ConflictingPackageDigest
            | ResolverError::InvalidPackageUrl
            | ResolverError::InvalidPackageMoniker
            | ResolverError::UnsupportedPackageFormat
            | ResolverError::InvalidMinimumOsVersion
            | ResolverError::InvalidFrameworkVersion
            | ResolverError::UnsupportedLocale => {
                Self::new(ErrorCode::DependencyUnresolved, RetryAdvice::ReResolve)
            }
        }
    }
}

impl From<&ApplicabilityError> for AppErrorDto {
    fn from(error: &ApplicabilityError) -> Self {
        match error {
            ApplicabilityError::MarketMismatch => {
                Self::new(ErrorCode::MarketUnavailable, RetryAdvice::Never)
            }
            ApplicabilityError::NoCompatiblePackage => {
                Self::new(ErrorCode::NoCompatiblePackage, RetryAdvice::Never)
            }
            ApplicabilityError::DependencyUnresolved { .. } => {
                Self::new(ErrorCode::DependencyUnresolved, RetryAdvice::ReResolve)
            }
            ApplicabilityError::DependencyCycle { .. } => {
                Self::new(ErrorCode::DependencyUnresolved, RetryAdvice::ReResolve)
            }
            ApplicabilityError::VersionAheadOfCatalog { .. } => {
                Self::new(ErrorCode::VersionAheadOfCatalog, RetryAdvice::Never)
            }
            ApplicabilityError::PackageNotInstalled { .. } => Self::new(
                ErrorCode::PackageNotInstalled,
                RetryAdvice::ReconcileInventory,
            ),
        }
    }
}

impl From<&DownloadError> for AppErrorDto {
    fn from(error: &DownloadError) -> Self {
        match error {
            DownloadError::UrlExpired | DownloadError::InvalidResumeResponse => {
                Self::new(ErrorCode::DownloadUrlExpired, RetryAdvice::ReResolve)
            }
            DownloadError::UrlExpiredStatus(status) => {
                Self::new(ErrorCode::DownloadUrlExpired, RetryAdvice::ReResolve)
                    .with_safe_detail(SafeErrorDetail::HttpStatus { status: *status })
            }
            DownloadError::SizeMismatch | DownloadError::HashMismatch => {
                Self::new(ErrorCode::HashMismatch, RetryAdvice::ReResolve)
            }
            DownloadError::Transport(DownloadTransportError::ProxyConnection) => {
                Self::new(ErrorCode::DownloadProxyFailed, RetryAdvice::Retry)
            }
            DownloadError::Transport(DownloadTransportError::Timeout) => {
                Self::new(ErrorCode::DownloadTimeout, RetryAdvice::Retry)
            }
            DownloadError::Transport(DownloadTransportError::Connection)
            | DownloadError::Transport(DownloadTransportError::Request) => {
                Self::new(ErrorCode::DownloadConnectionFailed, RetryAdvice::Retry)
            }
            DownloadError::Transport(DownloadTransportError::ResponseBody) => {
                Self::new(ErrorCode::DownloadResponseFailed, RetryAdvice::Retry)
            }
            DownloadError::HttpStatus(407) => {
                Self::new(ErrorCode::DownloadProxyAuthRequired, RetryAdvice::Never)
                    .with_safe_detail(SafeErrorDetail::HttpStatus { status: 407 })
            }
            DownloadError::HttpStatus(status) => {
                Self::new(ErrorCode::DownloadHttpStatus, RetryAdvice::Retry)
                    .with_safe_detail(SafeErrorDetail::HttpStatus { status: *status })
            }
            DownloadError::RedirectRejected => {
                Self::new(ErrorCode::DownloadRedirectRejected, RetryAdvice::Never)
            }
            DownloadError::Io => Self::new(ErrorCode::DownloadIoFailed, RetryAdvice::Retry),
            DownloadError::InvalidRequest
            | DownloadError::InvalidNetworkPolicy
            | DownloadError::Cancelled => Self::new(ErrorCode::DownloadFailed, RetryAdvice::Never),
        }
    }
}

impl From<&DeploymentPlanError> for AppErrorDto {
    fn from(error: &DeploymentPlanError) -> Self {
        match error {
            DeploymentPlanError::UnsupportedPackageFormat { .. } => {
                Self::new(ErrorCode::UnsupportedPackageType, RetryAdvice::Never)
            }
            DeploymentPlanError::DependencyCycle { .. } => {
                Self::new(ErrorCode::DependencyUnresolved, RetryAdvice::ReResolve)
            }
            DeploymentPlanError::MissingVerifiedCache { .. }
            | DeploymentPlanError::CacheNotVerified { .. } => {
                Self::new(ErrorCode::DownloadFailed, RetryAdvice::ReResolve)
            }
            DeploymentPlanError::PackageValidation { error, .. } => Self::from(error),
            DeploymentPlanError::MissingProductId
            | DeploymentPlanError::MissingMainPackage
            | DeploymentPlanError::AmbiguousMainPackage
            | DeploymentPlanError::MissingPackageIdentity { .. }
            | DeploymentPlanError::CacheMetadataMismatch { .. }
            | DeploymentPlanError::UnsafeCachePath { .. }
            | DeploymentPlanError::CheckpointInvalid => {
                Self::new(ErrorCode::SourceIdentityMismatch, RetryAdvice::ReResolve)
            }
        }
    }
}

impl From<&ValidationError> for AppErrorDto {
    fn from(error: &ValidationError) -> Self {
        match error {
            ValidationError::HashMismatch => {
                Self::new(ErrorCode::HashMismatch, RetryAdvice::ReResolve)
            }
            ValidationError::IdentityMismatch | ValidationError::Manifest(_) => {
                Self::new(ErrorCode::SourceIdentityMismatch, RetryAdvice::ReResolve)
            }
            ValidationError::SignatureInvalid => {
                Self::new(ErrorCode::SignatureInvalid, RetryAdvice::Never)
            }
            ValidationError::UnsupportedPackageFormat => {
                Self::new(ErrorCode::UnsupportedPackageType, RetryAdvice::Never)
            }
            ValidationError::InvalidPath | ValidationError::RootEscape | ValidationError::Io(_) => {
                Self::new(ErrorCode::DeploymentFailed, RetryAdvice::Retry)
            }
        }
    }
}

impl From<&CoordinatorError> for AppErrorDto {
    fn from(error: &CoordinatorError) -> Self {
        match error.code.as_str() {
            "inventory_access_denied" | "deployment_denied" => {
                Self::new(ErrorCode::DeploymentDenied, RetryAdvice::Never)
            }
            "postcondition_missing" | "postcondition_residual" | "incomplete_inventory" => {
                Self::new(ErrorCode::DeploymentFailed, RetryAdvice::ReconcileInventory)
            }
            "signature_invalid" => Self::new(ErrorCode::SignatureInvalid, RetryAdvice::Never),
            "hash_mismatch" => Self::new(ErrorCode::HashMismatch, RetryAdvice::ReResolve),
            "source_identity_mismatch" => {
                Self::new(ErrorCode::SourceIdentityMismatch, RetryAdvice::ReResolve)
            }
            "unsupported_package_type" => {
                Self::new(ErrorCode::UnsupportedPackageType, RetryAdvice::Never)
            }
            "package_in_use" => Self::new(ErrorCode::PackageInUse, RetryAdvice::Retry),
            "package_not_installed" => Self::new(
                ErrorCode::PackageNotInstalled,
                RetryAdvice::ReconcileInventory,
            ),
            _ => Self::new(ErrorCode::DeploymentFailed, RetryAdvice::Retry),
        }
    }
}
