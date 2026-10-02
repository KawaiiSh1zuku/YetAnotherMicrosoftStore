use std::{fmt, path::Path};

use sha2::{Digest, Sha256};
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationError {
    Io,
    Cancelled,
    SizeMismatch,
    HashMismatch,
}

impl fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io => formatter.write_str("package verification I/O failed"),
            Self::Cancelled => formatter.write_str("package verification was cancelled"),
            Self::SizeMismatch => formatter.write_str("package size does not match"),
            Self::HashMismatch => formatter.write_str("package hash does not match"),
        }
    }
}

impl std::error::Error for VerificationError {}

pub async fn hash_file(path: &Path) -> Result<(u64, String), VerificationError> {
    hash_file_cancellable(path, &|| false).await
}

async fn hash_file_cancellable<F>(
    path: &Path,
    cancelled: &F,
) -> Result<(u64, String), VerificationError>
where
    F: Fn() -> bool,
{
    if cancelled() {
        return Err(VerificationError::Cancelled);
    }
    let mut file = File::open(path).await.map_err(|_| VerificationError::Io)?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        if cancelled() {
            return Err(VerificationError::Cancelled);
        }
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|_| VerificationError::Io)?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .ok_or(VerificationError::SizeMismatch)?;
        hasher.update(&buffer[..read]);
    }
    Ok((size, format!("{:x}", hasher.finalize())))
}

pub async fn verify_and_promote(
    partial_path: &Path,
    verified_path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), VerificationError> {
    verify_and_promote_cancellable(
        partial_path,
        verified_path,
        expected_size,
        expected_sha256,
        || false,
    )
    .await
}

pub async fn verify_and_promote_cancellable<F>(
    partial_path: &Path,
    verified_path: &Path,
    expected_size: u64,
    expected_sha256: &str,
    cancelled: F,
) -> Result<(), VerificationError>
where
    F: Fn() -> bool,
{
    let (actual_size, actual_sha256) = hash_file_cancellable(partial_path, &cancelled).await?;
    if actual_size != expected_size {
        return Err(VerificationError::SizeMismatch);
    }
    if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(VerificationError::HashMismatch);
    }
    if let Some(parent) = verified_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|_| VerificationError::Io)?;
    }
    if fs::try_exists(verified_path)
        .await
        .map_err(|_| VerificationError::Io)?
    {
        let (size, hash) = hash_file_cancellable(verified_path, &cancelled).await?;
        if size == expected_size && hash.eq_ignore_ascii_case(expected_sha256) {
            if cancelled() {
                return Err(VerificationError::Cancelled);
            }
            fs::remove_file(partial_path)
                .await
                .map_err(|_| VerificationError::Io)?;
            return Ok(());
        }
        fs::remove_file(verified_path)
            .await
            .map_err(|_| VerificationError::Io)?;
    }
    if cancelled() {
        return Err(VerificationError::Cancelled);
    }
    fs::rename(partial_path, verified_path)
        .await
        .map_err(|_| VerificationError::Io)
}
