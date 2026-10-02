use std::{path::PathBuf, sync::OnceLock};

use serde::{Deserialize, Serialize};

use crate::deployment::DeploymentScope;

pub const BROKER_PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

const SUPPORTED_PACKAGE_EXTENSIONS: &[&str] = &[
    "msix",
    "appx",
    "msixbundle",
    "appxbundle",
    "eappx",
    "eappxbundle",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrokerOperation {
    InstallAllUsers,
    UninstallAllUsers,
    ScanAllUsers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrokerErrorCode {
    ProtocolMismatch,
    InvalidRequest,
    UnsupportedPackageFormat,
    UnsupportedOperation,
    FrameTooLarge,
    InvalidFrame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageIdentity {
    pub name: String,
    pub publisher: String,
    pub version: [u16; 4],
    pub architecture: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageFileRequest {
    pub path: PathBuf,
    pub sha256_hex: String,
    pub expected_identity: Option<PackageIdentity>,
}

impl PackageFileRequest {
    pub fn validate_shape(&self) -> Result<(), BrokerErrorCode> {
        if !self.path.is_absolute() {
            return Err(BrokerErrorCode::InvalidRequest);
        }

        let extension = self
            .path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        if !extension
            .as_deref()
            .is_some_and(|value| SUPPORTED_PACKAGE_EXTENSIONS.contains(&value))
        {
            return Err(BrokerErrorCode::UnsupportedPackageFormat);
        }

        if self.sha256_hex.len() != 64
            || !self.sha256_hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(BrokerErrorCode::InvalidRequest);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllUsersRemovalRequest {
    pub package_family_name: String,
    pub package_full_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrokerPayload {
    None,
    Install { packages: Vec<PackageFileRequest> },
    Uninstall { target: AllUsersRemovalRequest },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerRequest {
    pub protocol_version: u16,
    pub request_id: String,
    pub operation: BrokerOperation,
    pub parent_pid: u32,
    pub session_id: u32,
    pub nonce: String,
    pub payload: BrokerPayload,
}

impl BrokerRequest {
    pub fn scan(
        request_id: impl Into<String>,
        parent_pid: u32,
        session_id: u32,
        nonce: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: BROKER_PROTOCOL_VERSION,
            request_id: request_id.into(),
            operation: BrokerOperation::ScanAllUsers,
            parent_pid,
            session_id,
            nonce: nonce.into(),
            payload: BrokerPayload::None,
        }
    }

    pub fn validate_shape(&self) -> Result<(), BrokerErrorCode> {
        if self.protocol_version != BROKER_PROTOCOL_VERSION {
            return Err(BrokerErrorCode::ProtocolMismatch);
        }
        if self.request_id.trim().is_empty()
            || self.nonce.trim().is_empty()
            || self.parent_pid == 0
            || self.session_id == 0
        {
            return Err(BrokerErrorCode::InvalidRequest);
        }

        match (&self.operation, &self.payload) {
            (BrokerOperation::ScanAllUsers, BrokerPayload::None) => Ok(()),
            (BrokerOperation::InstallAllUsers, BrokerPayload::Install { packages })
                if !packages.is_empty() =>
            {
                packages
                    .iter()
                    .try_for_each(PackageFileRequest::validate_shape)
            }
            (BrokerOperation::UninstallAllUsers, BrokerPayload::Uninstall { target })
                if !target.package_family_name.trim().is_empty()
                    && !target.package_full_names.is_empty() =>
            {
                Ok(())
            }
            _ => Err(BrokerErrorCode::UnsupportedOperation),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerResponse {
    pub protocol_version: u16,
    pub request_id: String,
    pub error: Option<BrokerErrorCode>,
    pub stage: String,
    pub hresult: Option<i32>,
    pub message: String,
}

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, BrokerErrorCode> {
    let payload = serde_json::to_vec(value).map_err(|_| BrokerErrorCode::InvalidFrame)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(BrokerErrorCode::FrameTooLarge);
    }

    let length = u32::try_from(payload.len()).map_err(|_| BrokerErrorCode::FrameTooLarge)?;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame<T: for<'de> Deserialize<'de>>(frame: &[u8]) -> Result<T, BrokerErrorCode> {
    if frame.len() < 4 {
        return Err(BrokerErrorCode::InvalidFrame);
    }
    let declared = u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize;
    if declared > MAX_FRAME_BYTES || declared != frame.len() - 4 {
        return Err(if declared > MAX_FRAME_BYTES {
            BrokerErrorCode::FrameTooLarge
        } else {
            BrokerErrorCode::InvalidFrame
        });
    }
    serde_json::from_slice(&frame[4..]).map_err(|_| BrokerErrorCode::InvalidFrame)
}

pub fn supported_package_extensions() -> &'static [&'static str] {
    static EXTENSIONS: OnceLock<&'static [&'static str]> = OnceLock::new();
    EXTENSIONS.get_or_init(|| SUPPORTED_PACKAGE_EXTENSIONS)
}

pub fn scope_requires_broker(scope: DeploymentScope) -> bool {
    scope == DeploymentScope::AllUsers
}
