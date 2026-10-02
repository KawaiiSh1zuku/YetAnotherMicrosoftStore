use serde::{Deserialize, Serialize};

use crate::{catalog::CatalogError, resolver::ResolverError};

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
    DownloadUrlExpired,
    HashMismatch,
    SignatureInvalid,
    ElevationCancelled,
    DeploymentDenied,
    DeploymentFailed,
    PackageInUse,
    StoreEntitlementMissing,
    StoreChannelUnavailable,
    SourceIdentityMismatch,
    VersionAheadOfCatalog,
    MsixvcCapabilityUnavailable,
    UnsupportedPackageType,
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
            Self::DownloadUrlExpired => "errors.downloadUrlExpired",
            Self::HashMismatch => "errors.hashMismatch",
            Self::SignatureInvalid => "errors.signatureInvalid",
            Self::ElevationCancelled => "errors.elevationCancelled",
            Self::DeploymentDenied => "errors.deploymentDenied",
            Self::DeploymentFailed => "errors.deploymentFailed",
            Self::PackageInUse => "errors.packageInUse",
            Self::StoreEntitlementMissing => "errors.storeEntitlementMissing",
            Self::StoreChannelUnavailable => "errors.storeChannelUnavailable",
            Self::SourceIdentityMismatch => "errors.sourceIdentityMismatch",
            Self::VersionAheadOfCatalog => "errors.versionAheadOfCatalog",
            Self::MsixvcCapabilityUnavailable => "errors.msixvcCapabilityUnavailable",
            Self::UnsupportedPackageType => "errors.unsupportedPackageType",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryAdvice {
    Never,
    Retry,
    ReResolve,
    RequestElevation,
    ReconcileInventory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafeErrorDetail {
    pub key: String,
    pub value: String,
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

    pub fn with_safe_detail(mut self, key: &str, value: &str) -> Self {
        self.details.push(SafeErrorDetail {
            key: key.to_owned(),
            value: value.to_owned(),
        });
        self
    }
}

impl From<&CatalogError> for AppErrorDto {
    fn from(error: &CatalogError) -> Self {
        match error {
            CatalogError::InvalidUrl { field } => {
                Self::new(ErrorCode::CatalogUnavailable, RetryAdvice::ReResolve)
                    .with_safe_detail("field", field)
            }
            CatalogError::MalformedFixture(_)
            | CatalogError::MissingField(_)
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
                Self::new(ErrorCode::DependencyUnresolved, RetryAdvice::ReResolve)
                    .with_safe_detail("field", field)
            }
            ResolverError::MalformedFixture(_) | ResolverError::InvalidPackageSize => {
                Self::new(ErrorCode::DependencyUnresolved, RetryAdvice::ReResolve)
            }
        }
    }
}
