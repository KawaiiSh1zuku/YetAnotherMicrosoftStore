use std::{
    collections::HashSet,
    fmt, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{
    domain::{CacheEntry, CacheState},
    download::VerifiedDownload,
    persistence::{Persistence, PersistenceError},
    verification::{hash_file, VerificationError},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheReport {
    pub recovered_partials: usize,
    pub removed_missing: usize,
    pub removed_corrupt: usize,
    pub removed_unsafe: usize,
    pub removed_orphan_verified: usize,
    pub evicted_by_age: usize,
    pub evicted_by_size: usize,
    pub bytes_retained: u64,
}

#[derive(Debug)]
pub enum CacheError {
    InvalidRoot,
    Io,
    Persistence(PersistenceError),
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRoot => formatter.write_str("cache root is invalid"),
            Self::Io => formatter.write_str("cache file operation failed"),
            Self::Persistence(_) => formatter.write_str("cache metadata operation failed"),
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Persistence(error) => Some(error),
            Self::InvalidRoot | Self::Io => None,
        }
    }
}

impl From<PersistenceError> for CacheError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

pub struct CacheManager {
    root: PathBuf,
    canonical_root: PathBuf,
}

impl CacheManager {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, CacheError> {
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(CacheError::InvalidRoot);
        }
        if has_reparse_ancestor(root)? {
            return Err(CacheError::InvalidRoot);
        }
        fs::create_dir_all(root).map_err(|_| CacheError::Io)?;
        if has_reparse_ancestor(root)? {
            return Err(CacheError::InvalidRoot);
        }
        fs::create_dir_all(root.join("partial")).map_err(|_| CacheError::Io)?;
        fs::create_dir_all(root.join("verified")).map_err(|_| CacheError::Io)?;
        let canonical_root = root.canonicalize().map_err(|_| CacheError::InvalidRoot)?;
        let manager = Self {
            root: root.to_path_buf(),
            canonical_root,
        };
        if !manager.safe_existing_path(&manager.partial_root())?
            || !manager.safe_existing_path(&manager.verified_root())?
        {
            return Err(CacheError::InvalidRoot);
        }
        Ok(manager)
    }

    pub(crate) fn partial_root(&self) -> PathBuf {
        self.root.join("partial")
    }

    pub(crate) fn verified_root(&self) -> PathBuf {
        self.root.join("verified")
    }

    pub(crate) fn validate_candidate_path(&self, path: &Path) -> Result<(), CacheError> {
        if !path.starts_with(&self.root) {
            return Err(CacheError::InvalidRoot);
        }
        let parent = path.parent().ok_or(CacheError::InvalidRoot)?;
        if !parent.exists()
            || has_reparse_component(parent, &self.root)?
            || !parent
                .canonicalize()
                .map_err(|_| CacheError::InvalidRoot)?
                .starts_with(&self.canonical_root)
        {
            return Err(CacheError::InvalidRoot);
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata_is_reparse(&metadata)
                    || !path
                        .canonicalize()
                        .map_err(|_| CacheError::InvalidRoot)?
                        .starts_with(&self.canonical_root)
                {
                    return Err(CacheError::InvalidRoot);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(CacheError::Io),
        }
        Ok(())
    }

    pub(crate) fn validate_verified_path(&self, path: &Path) -> Result<(), CacheError> {
        if !path.is_file() || !self.safe_existing_path(path)? {
            return Err(CacheError::InvalidRoot);
        }
        Ok(())
    }

    pub fn record_verified(
        &self,
        persistence: &Persistence,
        download: &VerifiedDownload,
        job_id: Option<&str>,
        last_accessed_at: i64,
    ) -> Result<(), CacheError> {
        if !self.safe_existing_path(&download.path)? {
            return Err(CacheError::InvalidRoot);
        }
        persistence.upsert_cache_entry(&CacheEntry {
            cache_key: download.cache_key.clone(),
            job_id: job_id.map(str::to_owned),
            update_id: download.update_id.clone(),
            path: download.path.to_string_lossy().into_owned(),
            size: download.size,
            sha256: download.sha256.clone(),
            state: CacheState::Verified,
            last_accessed_at,
        })?;
        Ok(())
    }

    pub async fn reconcile_and_evict(
        &self,
        persistence: &Persistence,
        now: i64,
        max_bytes: u64,
        retention_days: u32,
        active_job_ids: &HashSet<String>,
    ) -> Result<CacheReport, CacheError> {
        let mut report = CacheReport {
            recovered_partials: self.recover_partial_sidecars(persistence)?,
            ..CacheReport::default()
        };
        if active_job_ids.is_empty() {
            report.removed_orphan_verified = self.remove_orphan_verified(persistence)?;
        }

        for entry in persistence.cache_entries()? {
            let path = PathBuf::from(&entry.path);
            if !path.exists() {
                persistence.delete_cache_entry(&entry.cache_key)?;
                report.removed_missing += 1;
                continue;
            }
            if !self.safe_existing_path(&path)? {
                persistence.delete_cache_entry(&entry.cache_key)?;
                report.removed_unsafe += 1;
                continue;
            }
            if entry.state == CacheState::Verified {
                let valid = hash_file(&path).await.is_ok_and(|(size, sha256)| {
                    size == entry.size && sha256.eq_ignore_ascii_case(&entry.sha256)
                });
                if !valid {
                    remove_file_if_exists(&path)?;
                    persistence.delete_cache_entry(&entry.cache_key)?;
                    report.removed_corrupt += 1;
                }
            }
        }

        let retention_seconds = i64::from(retention_days).saturating_mul(24 * 60 * 60);
        let cutoff = now.saturating_sub(retention_seconds);
        if retention_days > 0 {
            for entry in persistence.cache_entries()? {
                if protected_partial(&entry, active_job_ids) {
                    continue;
                }
                if entry.last_accessed_at < cutoff {
                    self.remove_entry(persistence, &entry)?;
                    report.evicted_by_age += 1;
                }
            }
        }

        let mut entries = persistence.cache_entries()?;
        let mut retained = physical_bytes(&entries)?;
        entries.sort_by_key(|entry| entry.last_accessed_at);
        for entry in entries {
            if retained <= max_bytes {
                break;
            }
            if protected_partial(&entry, active_job_ids) {
                continue;
            }
            self.remove_entry(persistence, &entry)?;
            retained = physical_bytes(&persistence.cache_entries()?)?;
            report.evicted_by_size += 1;
        }
        report.bytes_retained = physical_bytes(&persistence.cache_entries()?)?;
        Ok(report)
    }

    fn remove_orphan_verified(&self, persistence: &Persistence) -> Result<usize, CacheError> {
        let indexed = persistence
            .cache_entries()?
            .into_iter()
            .filter(|entry| entry.state == CacheState::Verified)
            .filter_map(|entry| PathBuf::from(entry.path).canonicalize().ok())
            .collect::<HashSet<_>>();
        let mut removed = 0;
        for result in fs::read_dir(self.root.join("verified")).map_err(|_| CacheError::Io)? {
            let path = result.map_err(|_| CacheError::Io)?.path();
            if !path.is_file() || !self.safe_existing_path(&path)? {
                continue;
            }
            let canonical = path.canonicalize().map_err(|_| CacheError::Io)?;
            if !indexed.contains(&canonical) {
                remove_file_if_exists(&path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn recover_partial_sidecars(&self, persistence: &Persistence) -> Result<usize, CacheError> {
        let partial_root = self.root.join("partial");
        let mut recovered = 0;
        for result in fs::read_dir(&partial_root).map_err(|_| CacheError::Io)? {
            let entry = result.map_err(|_| CacheError::Io)?;
            let metadata_path = entry.path();
            if metadata_path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Some(cache_key) = metadata_path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if !valid_cache_key(cache_key) {
                continue;
            }
            let partial_path = partial_root.join(format!("{cache_key}.part"));
            if !partial_path.exists() || !self.safe_existing_path(&partial_path)? {
                continue;
            }
            let metadata: RecoveryMetadata = match fs::read(&metadata_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            {
                Some(metadata) => metadata,
                None => continue,
            };
            let size = partial_path.metadata().map_err(|_| CacheError::Io)?.len();
            persistence.upsert_cache_entry(&CacheEntry {
                cache_key: cache_key.to_owned(),
                job_id: metadata.job_id,
                update_id: metadata.update_id,
                path: partial_path.to_string_lossy().into_owned(),
                size,
                sha256: metadata.expected_sha256,
                state: CacheState::Partial,
                last_accessed_at: metadata.last_accessed_at,
            })?;
            recovered += 1;
        }
        Ok(recovered)
    }

    fn remove_entry(
        &self,
        persistence: &Persistence,
        entry: &CacheEntry,
    ) -> Result<(), CacheError> {
        let path = PathBuf::from(&entry.path);
        let shared = if path.exists() && self.safe_existing_path(&path)? {
            let canonical = path.canonicalize().map_err(|_| CacheError::Io)?;
            persistence.cache_entries()?.into_iter().any(|candidate| {
                candidate.cache_key != entry.cache_key
                    && PathBuf::from(candidate.path).canonicalize().ok().as_ref()
                        == Some(&canonical)
            })
        } else {
            false
        };
        if !shared && path.exists() && self.safe_existing_path(&path)? {
            remove_file_if_exists(&path)?;
            if entry.state == CacheState::Partial {
                let metadata_path = path.with_extension("json");
                remove_file_if_exists(&metadata_path)?;
            }
        }
        persistence.delete_cache_entry(&entry.cache_key)?;
        Ok(())
    }

    fn safe_existing_path(&self, path: &Path) -> Result<bool, CacheError> {
        if !path.starts_with(&self.root) || has_reparse_component(path, &self.root)? {
            return Ok(false);
        }
        let canonical = path.canonicalize().map_err(|_| CacheError::Io)?;
        if !canonical.starts_with(&self.canonical_root) {
            return Ok(false);
        }
        Ok(!has_reparse_component(&canonical, &self.canonical_root)?)
    }
}

fn physical_bytes(entries: &[CacheEntry]) -> Result<u64, CacheError> {
    let mut paths = HashSet::new();
    let mut bytes = 0_u64;
    for entry in entries {
        let path = PathBuf::from(&entry.path)
            .canonicalize()
            .map_err(|_| CacheError::Io)?;
        if paths.insert(path) {
            bytes = bytes.saturating_add(entry.size);
        }
    }
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryMetadata {
    #[serde(default)]
    job_id: Option<String>,
    update_id: String,
    expected_sha256: String,
    last_accessed_at: i64,
}

fn valid_cache_key(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

fn protected_partial(entry: &CacheEntry, active_job_ids: &HashSet<String>) -> bool {
    entry.state == CacheState::Partial
        && entry
            .job_id
            .as_ref()
            .is_some_and(|job_id| active_job_ids.contains(job_id))
}

fn remove_file_if_exists(path: &Path) -> Result<(), CacheError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(CacheError::Io),
    }
}

#[cfg(windows)]
fn has_reparse_component(path: &Path, root: &Path) -> Result<bool, CacheError> {
    for ancestor in path.ancestors() {
        if !ancestor.starts_with(root) {
            break;
        }
        let metadata = fs::symlink_metadata(ancestor).map_err(|_| CacheError::Io)?;
        if metadata_is_reparse(&metadata) {
            return Ok(true);
        }
        if ancestor == root {
            break;
        }
    }
    Ok(false)
}

#[cfg(windows)]
fn has_reparse_ancestor(path: &Path) -> Result<bool, CacheError> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata_is_reparse(&metadata) => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(CacheError::Io),
        }
    }
    Ok(false)
}

#[cfg(windows)]
fn metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn has_reparse_component(path: &Path, root: &Path) -> Result<bool, CacheError> {
    for ancestor in path.ancestors() {
        if !ancestor.starts_with(root) {
            break;
        }
        if metadata_is_reparse(&fs::symlink_metadata(ancestor).map_err(|_| CacheError::Io)?) {
            return Ok(true);
        }
        if ancestor == root {
            break;
        }
    }
    Ok(false)
}

#[cfg(not(windows))]
fn has_reparse_ancestor(path: &Path) -> Result<bool, CacheError> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata_is_reparse(&metadata) => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(CacheError::Io),
        }
    }
    Ok(false)
}

#[cfg(not(windows))]
fn metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

impl From<VerificationError> for CacheError {
    fn from(_: VerificationError) -> Self {
        Self::Io
    }
}
