use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use futures_util::{future::join_all, StreamExt};
use reqwest::{
    header::{CONTENT_RANGE, ETAG, IF_RANGE, RANGE},
    redirect, Client, StatusCode,
};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
    sync::{Mutex, Notify, Semaphore},
    time::sleep,
};

use crate::{
    cache::{CacheError, CacheManager},
    settings::{NetworkPolicy, ProxyRoute, SettingsError},
    verification::{verify_and_promote_cancellable, VerificationError},
};

#[derive(Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    pub job_id: Option<String>,
    pub update_id: String,
    pub cache_key: String,
    pub url: String,
    pub file_name: String,
    pub expected_size: u64,
    pub expected_sha256: String,
    pub cache_root: PathBuf,
}

impl fmt::Debug for DownloadRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DownloadRequest")
            .field("job_id", &self.job_id)
            .field("update_id", &self.update_id)
            .field("cache_key", &self.cache_key)
            .field("url", &"[redacted]")
            .field("file_name", &self.file_name)
            .field("expected_size", &self.expected_size)
            .field("expected_sha256", &self.expected_sha256)
            .field("cache_root", &self.cache_root)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedDownload {
    pub update_id: String,
    pub cache_key: String,
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadError {
    InvalidRequest,
    InvalidNetworkPolicy,
    RedirectRejected,
    UrlExpired,
    HttpStatus,
    Transport,
    Io,
    InvalidResumeResponse,
    Cancelled,
    SizeMismatch,
    HashMismatch,
}

impl fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest => formatter.write_str("download request is invalid"),
            Self::InvalidNetworkPolicy => formatter.write_str("download URL is not permitted"),
            Self::RedirectRejected => formatter.write_str("download redirect was rejected"),
            Self::UrlExpired => formatter.write_str("download URL must be resolved again"),
            Self::HttpStatus => formatter.write_str("download server returned an error"),
            Self::Transport => formatter.write_str("download transport failed"),
            Self::Io => formatter.write_str("download file operation failed"),
            Self::InvalidResumeResponse => {
                formatter.write_str("download resume response is invalid")
            }
            Self::Cancelled => formatter.write_str("download was cancelled"),
            Self::SizeMismatch => formatter.write_str("download size does not match"),
            Self::HashMismatch => formatter.write_str("download hash does not match"),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<SettingsError> for DownloadError {
    fn from(_: SettingsError) -> Self {
        Self::InvalidRequest
    }
}

impl From<VerificationError> for DownloadError {
    fn from(error: VerificationError) -> Self {
        match error {
            VerificationError::Io => Self::Io,
            VerificationError::Cancelled => Self::Cancelled,
            VerificationError::SizeMismatch => Self::SizeMismatch,
            VerificationError::HashMismatch => Self::HashMismatch,
        }
    }
}

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notify: Notify,
}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<CancellationState>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        if !self.0.cancelled.swap(true, Ordering::AcqRel) {
            self.0.notify.notify_waiters();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        let notified = self.0.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

#[derive(Debug)]
struct RateState {
    next_available: Instant,
}

#[derive(Debug)]
struct AggregateRateLimiter {
    bytes_per_second: Option<u64>,
    state: Mutex<RateState>,
}

impl AggregateRateLimiter {
    fn new(bytes_per_second: Option<u64>) -> Result<Self, DownloadError> {
        if bytes_per_second == Some(0) {
            return Err(DownloadError::InvalidRequest);
        }
        Ok(Self {
            bytes_per_second,
            state: Mutex::new(RateState {
                next_available: Instant::now(),
            }),
        })
    }

    async fn consume(
        &self,
        bytes: usize,
        cancellation: &CancellationToken,
    ) -> Result<(), DownloadError> {
        let Some(limit) = self.bytes_per_second else {
            return Ok(());
        };
        let wait = {
            let mut state = self.state.lock().await;
            let now = Instant::now();
            let start = state.next_available.max(now);
            let duration = Duration::from_secs_f64(bytes as f64 / limit as f64);
            state.next_available = start + duration;
            state.next_available.saturating_duration_since(now)
        };
        if !wait.is_zero() {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(DownloadError::Cancelled),
                _ = sleep(wait) => {}
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct DownloadManager {
    client: Client,
    network_policy: NetworkPolicy,
    semaphore: Arc<Semaphore>,
    rate_limiter: Arc<AggregateRateLimiter>,
    cache_key_locks: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
}

impl DownloadManager {
    pub fn new(
        proxy: ProxyRoute,
        network_policy: NetworkPolicy,
        max_concurrent: usize,
        bytes_per_second: Option<u64>,
    ) -> Result<Self, DownloadError> {
        if max_concurrent == 0 {
            return Err(DownloadError::InvalidRequest);
        }
        let redirect_policy = network_policy.clone();
        let client = proxy
            .apply(
                Client::builder()
                    .no_gzip()
                    .no_brotli()
                    .no_deflate()
                    .no_zstd()
                    .redirect(redirect::Policy::custom(move |attempt| {
                        let previous = attempt
                            .previous()
                            .last()
                            .map(reqwest::Url::as_str)
                            .unwrap_or_default();
                        if redirect_policy
                            .validate_redirect(
                                previous,
                                attempt.url().as_str(),
                                attempt.previous().len(),
                            )
                            .is_ok()
                        {
                            attempt.follow()
                        } else {
                            attempt.error("redirect rejected by download policy")
                        }
                    })),
            )?
            .build()
            .map_err(|_| DownloadError::InvalidRequest)?;
        Ok(Self {
            client,
            network_policy,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            rate_limiter: Arc::new(AggregateRateLimiter::new(bytes_per_second)?),
            cache_key_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn download(
        &self,
        request: DownloadRequest,
        cancellation: CancellationToken,
    ) -> Result<VerifiedDownload, DownloadError> {
        validate_request(&request)?;
        let cache_key_lock = self.cache_key_lock(&request.cache_key).await;
        let _cache_key_guard = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(DownloadError::Cancelled),
            guard = cache_key_lock.lock_owned() => guard,
        };
        let _permit = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(DownloadError::Cancelled),
            permit = self.semaphore.acquire() => permit.map_err(|_| DownloadError::Cancelled)?,
        };
        self.perform_download(&request, &cancellation).await
    }

    pub async fn download_with_refresh<F, Fut>(
        &self,
        mut request: DownloadRequest,
        cancellation: CancellationToken,
        mut refresh: F,
    ) -> Result<VerifiedDownload, DownloadError>
    where
        F: FnMut(&str) -> Fut,
        Fut: std::future::Future<Output = Result<String, DownloadError>>,
    {
        validate_request(&request)?;
        let cache_key_lock = self.cache_key_lock(&request.cache_key).await;
        let _cache_key_guard = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(DownloadError::Cancelled),
            guard = cache_key_lock.lock_owned() => guard,
        };
        let _permit = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(DownloadError::Cancelled),
            permit = self.semaphore.acquire() => permit.map_err(|_| DownloadError::Cancelled)?,
        };
        match self.perform_download(&request, &cancellation).await {
            Err(DownloadError::UrlExpired) => {
                request.url = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Err(DownloadError::Cancelled),
                    url = refresh(&request.update_id) => url?,
                };
                self.perform_download(&request, &cancellation).await
            }
            result => result,
        }
    }

    async fn cache_key_lock(&self, cache_key: &str) -> Arc<Mutex<()>> {
        let physical_key = cache_key.to_ascii_lowercase();
        let mut locks = self.cache_key_locks.lock().await;
        if let Some(lock) = locks.get(&physical_key).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(physical_key, Arc::downgrade(&lock));
        lock
    }

    pub async fn download_many(
        &self,
        requests: Vec<DownloadRequest>,
        cancellation: CancellationToken,
    ) -> Vec<Result<VerifiedDownload, DownloadError>> {
        join_all(requests.into_iter().map(|request| {
            let manager = self.clone();
            let cancellation = cancellation.clone();
            async move { manager.download(request, cancellation).await }
        }))
        .await
    }

    async fn perform_download(
        &self,
        request: &DownloadRequest,
        cancellation: &CancellationToken,
    ) -> Result<VerifiedDownload, DownloadError> {
        validate_request(request)?;
        self.network_policy
            .validate_url(&request.url)
            .map_err(|_| DownloadError::InvalidNetworkPolicy)?;
        if cancellation.is_cancelled() {
            return Err(DownloadError::Cancelled);
        }

        let cache = CacheManager::new(&request.cache_root).map_err(|error| match error {
            CacheError::InvalidRoot => DownloadError::InvalidRequest,
            CacheError::Io | CacheError::Persistence(_) => DownloadError::Io,
        })?;
        let partial_directory = cache.partial_root();
        let verified_directory = cache.verified_root();
        let partial_path = partial_directory.join(format!("{}.part", request.cache_key));
        let metadata_path = partial_directory.join(format!("{}.json", request.cache_key));
        let extension = Path::new(&request.file_name)
            .extension()
            .and_then(|value| value.to_str())
            .ok_or(DownloadError::InvalidRequest)?
            .to_ascii_lowercase();
        let verified_path = verified_directory.join(format!(
            "{}.{}",
            request.expected_sha256.to_ascii_lowercase(),
            extension
        ));
        let temporary_metadata_path = metadata_path.with_extension("json.tmp");
        for path in [
            &partial_path,
            &metadata_path,
            &temporary_metadata_path,
            &verified_path,
        ] {
            cache
                .validate_candidate_path(path)
                .map_err(map_cache_path_error)?;
        }

        let (mut offset, mut prior_etag) =
            resume_state(request, &partial_path, &metadata_path).await?;
        let mut restarted = false;
        loop {
            if cancellation.is_cancelled() {
                return Err(DownloadError::Cancelled);
            }
            let mut builder = self.client.get(&request.url);
            if offset > 0 {
                builder = builder.header(RANGE, format!("bytes={offset}-"));
                if let Some(etag) = prior_etag.as_deref() {
                    builder = builder.header(IF_RANGE, etag);
                }
            }
            let response = tokio::select! {
                biased;
                _ = cancellation.cancelled() => return Err(DownloadError::Cancelled),
                response = builder.send() => response,
            }
            .map_err(|error| {
                if error.is_redirect() {
                    DownloadError::RedirectRejected
                } else {
                    DownloadError::Transport
                }
            })?;
            if matches!(
                response.status(),
                StatusCode::UNAUTHORIZED
                    | StatusCode::FORBIDDEN
                    | StatusCode::NOT_FOUND
                    | StatusCode::GONE
            ) {
                return Err(DownloadError::UrlExpired);
            }
            if offset > 0 && response.status() == StatusCode::PARTIAL_CONTENT {
                let response_etag = header_text(response.headers().get(ETAG));
                let content_range = header_text(response.headers().get(CONTENT_RANGE))
                    .as_deref()
                    .and_then(parse_content_range);
                let valid_range = content_range.is_some_and(|range| {
                    range.start == offset
                        && range.total == request.expected_size
                        && range.end.saturating_add(1) == range.total
                });
                if !valid_range || response_etag.as_deref() != prior_etag.as_deref() {
                    if restarted {
                        return Err(DownloadError::InvalidResumeResponse);
                    }
                    clear_partial(&partial_path, &metadata_path).await;
                    offset = 0;
                    prior_etag = None;
                    restarted = true;
                    continue;
                }
            } else if response.status() == StatusCode::OK {
                if offset > 0 {
                    offset = 0;
                }
            } else {
                return Err(DownloadError::HttpStatus);
            }

            if response.content_length().is_some_and(|length| {
                offset
                    .checked_add(length)
                    .is_none_or(|total| total != request.expected_size)
            }) {
                clear_partial(&partial_path, &metadata_path).await;
                return Err(DownloadError::SizeMismatch);
            }

            let response_etag = header_text(response.headers().get(ETAG));
            cache
                .validate_candidate_path(&metadata_path)
                .map_err(map_cache_path_error)?;
            cache
                .validate_candidate_path(&temporary_metadata_path)
                .map_err(map_cache_path_error)?;
            write_metadata(
                &metadata_path,
                &PartialMetadata {
                    job_id: request.job_id.clone(),
                    update_id: request.update_id.clone(),
                    etag: response_etag,
                    expected_size: request.expected_size,
                    expected_sha256: request.expected_sha256.to_ascii_lowercase(),
                    last_accessed_at: now_unix_seconds(),
                },
            )
            .await?;
            cache
                .validate_candidate_path(&partial_path)
                .map_err(map_cache_path_error)?;
            let mut output = OpenOptions::new()
                .create(true)
                .write(true)
                .append(offset > 0)
                .truncate(offset == 0)
                .open(&partial_path)
                .await
                .map_err(|_| DownloadError::Io)?;
            let mut stream = response.bytes_stream();
            let mut written = offset;
            loop {
                let next = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => {
                        output.sync_all().await.map_err(|_| DownloadError::Io)?;
                        return Err(DownloadError::Cancelled);
                    }
                    next = stream.next() => next,
                };
                let Some(chunk) = next else { break };
                let chunk = chunk.map_err(|_| DownloadError::Transport)?;
                let next_written = written
                    .checked_add(chunk.len() as u64)
                    .ok_or(DownloadError::SizeMismatch)?;
                if next_written > request.expected_size {
                    drop(output);
                    clear_partial(&partial_path, &metadata_path).await;
                    return Err(DownloadError::SizeMismatch);
                }
                output
                    .write_all(&chunk)
                    .await
                    .map_err(|_| DownloadError::Io)?;
                written = next_written;
                self.rate_limiter.consume(chunk.len(), cancellation).await?;
            }
            output.sync_all().await.map_err(|_| DownloadError::Io)?;
            drop(output);

            cache
                .validate_candidate_path(&partial_path)
                .map_err(map_cache_path_error)?;
            cache
                .validate_candidate_path(&verified_path)
                .map_err(map_cache_path_error)?;
            let verification = verify_and_promote_cancellable(
                &partial_path,
                &verified_path,
                request.expected_size,
                &request.expected_sha256,
                || cancellation.is_cancelled(),
            )
            .await;
            if let Err(error) = verification {
                if matches!(
                    error,
                    VerificationError::SizeMismatch | VerificationError::HashMismatch
                ) {
                    clear_partial(&partial_path, &metadata_path).await;
                }
                return Err(error.into());
            }
            let _ = fs::remove_file(&metadata_path).await;
            return Ok(VerifiedDownload {
                update_id: request.update_id.clone(),
                cache_key: request.cache_key.clone(),
                path: verified_path,
                size: request.expected_size,
                sha256: request.expected_sha256.to_ascii_lowercase(),
            });
        }
    }
}

fn map_cache_path_error(error: CacheError) -> DownloadError {
    match error {
        CacheError::InvalidRoot => DownloadError::InvalidRequest,
        CacheError::Io | CacheError::Persistence(_) => DownloadError::Io,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartialMetadata {
    #[serde(default)]
    job_id: Option<String>,
    update_id: String,
    etag: Option<String>,
    expected_size: u64,
    expected_sha256: String,
    last_accessed_at: i64,
}

async fn resume_state(
    request: &DownloadRequest,
    partial_path: &Path,
    metadata_path: &Path,
) -> Result<(u64, Option<String>), DownloadError> {
    let metadata_bytes = match fs::read(metadata_path).await {
        Ok(bytes) => bytes,
        Err(_) => return Ok((0, None)),
    };
    let metadata: PartialMetadata = match serde_json::from_slice(&metadata_bytes) {
        Ok(metadata) => metadata,
        Err(_) => {
            clear_partial(partial_path, metadata_path).await;
            return Ok((0, None));
        }
    };
    let length = fs::metadata(partial_path)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if metadata.update_id != request.update_id
        || metadata.expected_size != request.expected_size
        || !metadata
            .expected_sha256
            .eq_ignore_ascii_case(&request.expected_sha256)
        || length == 0
        || length >= request.expected_size
        || metadata.etag.as_deref().is_none_or(str::is_empty)
    {
        clear_partial(partial_path, metadata_path).await;
        return Ok((0, None));
    }
    Ok((length, metadata.etag))
}

async fn write_metadata(path: &Path, metadata: &PartialMetadata) -> Result<(), DownloadError> {
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(metadata).map_err(|_| DownloadError::Io)?;
    fs::write(&temporary, bytes)
        .await
        .map_err(|_| DownloadError::Io)?;
    if fs::try_exists(path).await.map_err(|_| DownloadError::Io)? {
        fs::remove_file(path).await.map_err(|_| DownloadError::Io)?;
    }
    fs::rename(temporary, path)
        .await
        .map_err(|_| DownloadError::Io)
}

async fn clear_partial(partial_path: &Path, metadata_path: &Path) {
    let _ = fs::remove_file(partial_path).await;
    let _ = fs::remove_file(metadata_path).await;
}

fn validate_request(request: &DownloadRequest) -> Result<(), DownloadError> {
    if request.update_id.trim().is_empty()
        || request.cache_key.is_empty()
        || !request.cache_key.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        || request.expected_size == 0
        || request.expected_sha256.len() != 64
        || !request
            .expected_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !request.cache_root.is_absolute()
    {
        return Err(DownloadError::InvalidRequest);
    }
    let extension = Path::new(&request.file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    if !extension.as_deref().is_some_and(|extension| {
        matches!(
            extension,
            "msix" | "appx" | "msixbundle" | "appxbundle" | "eappx" | "eappxbundle"
        )
    }) {
        return Err(DownloadError::InvalidRequest);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContentRange {
    start: u64,
    end: u64,
    total: u64,
}

fn parse_content_range(value: &str) -> Option<ContentRange> {
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let range = ContentRange {
        start: start.parse().ok()?,
        end: end.parse().ok()?,
        total: total.parse().ok()?,
    };
    (range.start <= range.end && range.end < range.total).then_some(range)
}

fn header_text(value: Option<&reqwest::header::HeaderValue>) -> Option<String> {
    value?.to_str().ok().map(str::to_owned)
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}
