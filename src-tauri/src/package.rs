use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
