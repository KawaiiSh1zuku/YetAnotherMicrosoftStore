#![cfg(windows)]

use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use yet_another_microsoft_store_lib::{
    download::{
        CancellationToken, DownloadError, DownloadManager, DownloadRequest, DownloadTransportError,
        VerifiedDownload,
    },
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    settings::{NetworkPolicy, ProxyRoute},
    verification::{verify_and_promote_cancellable, VerificationError},
};

#[derive(Clone)]
struct ResponseSpec {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
    delay: Duration,
}

impl ResponseSpec {
    fn ok(body: &[u8]) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: body.to_vec(),
            delay: Duration::ZERO,
        }
    }

    fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

struct TestServer {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    max_active: Arc<AtomicUsize>,
    shutdown: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl TestServer {
    fn start(responses: Vec<ResponseSpec>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture server");
        listener
            .set_nonblocking(true)
            .expect("set fixture listener nonblocking");
        let address = listener.local_addr().expect("fixture address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let active = Arc::new(AtomicUsize::new(0));
        let active_for_thread = Arc::clone(&active);
        let max_active = Arc::new(AtomicUsize::new(0));
        let max_for_thread = Arc::clone(&max_active);
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_for_thread = Arc::clone(&shutdown);
        let join = thread::spawn(move || {
            let mut handlers = Vec::new();
            while !shutdown_for_thread.load(Ordering::Acquire) {
                let (stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("accept fixture request: {error}"),
                };
                stream
                    .set_nonblocking(false)
                    .expect("set fixture stream blocking");
                let response = responses.lock().expect("response queue").pop_front();
                let Some(response) = response else { break };
                let captured = Arc::clone(&captured);
                let active = Arc::clone(&active_for_thread);
                let max_active = Arc::clone(&max_for_thread);
                handlers.push(thread::spawn(move || {
                    let now_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_active.fetch_max(now_active, Ordering::SeqCst);
                    handle_request(stream, response, captured);
                    active.fetch_sub(1, Ordering::SeqCst);
                }));
            }
            for handler in handlers {
                handler.join().expect("fixture handler");
            }
        });
        Self {
            address,
            requests,
            max_active,
            shutdown,
            join: Some(join),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.address, path)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("captured requests").clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.shutdown.store(true, Ordering::Release);
            join.join().expect("join fixture server");
        }
    }
}

fn handle_request(
    mut stream: TcpStream,
    response: ResponseSpec,
    captured: Arc<Mutex<Vec<String>>>,
) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).expect("read fixture request");
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    captured
        .lock()
        .expect("capture request")
        .push(String::from_utf8_lossy(&request).into_owned());
    thread::sleep(response.delay);
    let reason = match response.status {
        200 => "OK",
        206 => "Partial Content",
        302 => "Found",
        403 => "Forbidden",
        _ => "Fixture",
    };
    let chunk_delay = response
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("X-Fixture-Chunk-Delay"))
        .and_then(|(_, value)| value.parse::<u64>().ok())
        .map(Duration::from_millis);
    let mut headers = format!(
        "HTTP/1.1 {} {}\r\nConnection: close\r\n",
        response.status, reason
    );
    if chunk_delay.is_some() {
        headers.push_str("Transfer-Encoding: chunked\r\n");
    } else {
        headers.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    }
    for (name, value) in response
        .headers
        .into_iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("X-Fixture-Chunk-Delay"))
    {
        headers.push_str(&format!("{name}: {value}\r\n"));
    }
    headers.push_str("\r\n");
    if stream.write_all(headers.as_bytes()).is_ok() {
        if let Some(delay) = chunk_delay {
            let _ = write!(stream, "{:X}\r\n", response.body.len());
            if stream.write_all(&response.body).is_ok() && stream.write_all(b"\r\n").is_ok() {
                thread::sleep(delay);
                let _ = stream.write_all(b"0\r\n\r\n");
            }
        } else {
            let _ = stream.write_all(&response.body);
        }
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("yamstore-m4-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create test cache");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn request(root: &Path, url: String, bytes: &[u8], key: &str) -> DownloadRequest {
    DownloadRequest {
        job_id: Some("job-download".to_owned()),
        update_id: "update-main".to_owned(),
        cache_key: key.to_owned(),
        url,
        file_name: "package.msix".to_owned(),
        expected_size: bytes.len() as u64,
        expected_sha256: sha256(bytes),
        cache_root: root.to_path_buf(),
    }
}

fn manager(max_concurrent: usize, bytes_per_second: Option<u64>) -> DownloadManager {
    DownloadManager::new(
        ProxyRoute::Disabled,
        NetworkPolicy::loopback_fixture(5),
        max_concurrent,
        bytes_per_second,
    )
    .expect("download manager")
}

fn seed_partial(root: &Path, request: &DownloadRequest, bytes: &[u8], etag: &str) {
    let directory = root.join("partial");
    fs::create_dir_all(&directory).expect("partial directory");
    fs::write(directory.join(format!("{}.part", request.cache_key)), bytes).expect("write partial");
    let metadata = serde_json::json!({
        "updateId": request.update_id,
        "etag": etag,
        "expectedSize": request.expected_size,
        "expectedSha256": request.expected_sha256,
        "lastAccessedAt": 100
    });
    fs::write(
        directory.join(format!("{}.json", request.cache_key)),
        serde_json::to_vec(&metadata).expect("metadata json"),
    )
    .expect("write partial metadata");
}

fn seed_partial_without_etag(root: &Path, request: &DownloadRequest, bytes: &[u8]) {
    let directory = root.join("partial");
    fs::create_dir_all(&directory).expect("partial directory");
    fs::write(directory.join(format!("{}.part", request.cache_key)), bytes).expect("write partial");
    let metadata = serde_json::json!({
        "updateId": request.update_id,
        "etag": null,
        "expectedSize": request.expected_size,
        "expectedSha256": request.expected_sha256,
        "lastAccessedAt": 100
    });
    fs::write(
        directory.join(format!("{}.json", request.cache_key)),
        serde_json::to_vec(&metadata).expect("metadata json"),
    )
    .expect("write partial metadata");
}

#[tokio::test(flavor = "multi_thread")]
async fn fresh_download_streams_to_a_hash_named_verified_file() {
    let payload = b"verified package payload";
    let server = TestServer::start(vec![ResponseSpec::ok(payload).with_header("ETag", "v1")]);
    let cache = TestDirectory::new("fresh");
    let request = request(&cache.0, server.url("/package.msix"), payload, "fresh-key");

    let verified = manager(2, None)
        .download(request, CancellationToken::new())
        .await
        .expect("download succeeds");

    assert_eq!(verified.size, payload.len() as u64);
    assert_eq!(verified.sha256, sha256(payload));
    assert_eq!(fs::read(&verified.path).expect("verified payload"), payload);
    assert!(verified.path.starts_with(cache.0.join("verified")));
    assert!(!cache.0.join("partial/fresh-key.part").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn fresh_download_reports_stream_progress_before_returning() {
    let payload = vec![7_u8; 16 * 1024];
    let server = TestServer::start(vec![ResponseSpec::ok(&payload).with_header("ETag", "v1")]);
    let cache = TestDirectory::new("progress");
    let request = request(
        &cache.0,
        server.url("/package.msix"),
        &payload,
        "progress-key",
    );
    let mut progress = Vec::new();

    manager(1, None)
        .download_with_progress(request, CancellationToken::new(), |done, total| {
            progress.push((done, total));
        })
        .await
        .expect("download succeeds");

    assert!(!progress.is_empty());
    assert_eq!(
        progress.last(),
        Some(&(payload.len() as u64, payload.len() as u64))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn custom_http_proxy_downloads_an_allowed_microsoft_package_url() {
    let payload = b"package returned by the configured proxy";
    let proxy = TestServer::start(vec![ResponseSpec::ok(payload).with_header("ETag", "v1")]);
    let cache = TestDirectory::new("http-proxy");
    let request = request(
        &cache.0,
        "http://dl.delivery.mp.microsoft.com/package.msix".to_owned(),
        payload,
        "http-proxy-key",
    );
    let manager = DownloadManager::new(
        ProxyRoute::Custom {
            endpoint: format!("http://{}/", proxy.address),
            credentials: None,
        },
        NetworkPolicy::production(["dl.delivery.mp.microsoft.com"]).expect("production policy"),
        1,
        None,
    )
    .expect("proxied manager");

    let verified = manager
        .download(request, CancellationToken::new())
        .await
        .expect("proxy download succeeds");

    assert_eq!(fs::read(verified.path).expect("proxy payload"), payload);
    assert!(proxy.requests()[0]
        .starts_with("GET http://dl.delivery.mp.microsoft.com/package.msix HTTP/1.1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn matching_etag_resumes_with_range_and_if_range() {
    let payload = b"0123456789abcdef";
    let cache = TestDirectory::new("resume");
    let server = TestServer::start(vec![ResponseSpec {
        status: 206,
        headers: vec![
            ("ETag", "v1".to_owned()),
            ("Content-Range", "bytes 8-15/16".to_owned()),
        ],
        body: payload[8..].to_vec(),
        delay: Duration::ZERO,
    }]);
    let request = request(&cache.0, server.url("/package.msix"), payload, "resume-key");
    seed_partial(&cache.0, &request, &payload[..8], "v1");

    let verified = manager(1, None)
        .download(request, CancellationToken::new())
        .await
        .expect("resume succeeds");

    assert_eq!(fs::read(verified.path).expect("resumed payload"), payload);
    let captured = server.requests().join("\n").to_ascii_lowercase();
    assert!(captured.contains("range: bytes=8-"));
    assert!(captured.contains("if-range: v1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn partial_without_etag_restarts_instead_of_sending_an_unconditional_range() {
    let payload = b"complete replacement payload";
    let cache = TestDirectory::new("missing-etag");
    let server = TestServer::start(vec![ResponseSpec::ok(payload).with_header("ETag", "v2")]);
    let request = request(
        &cache.0,
        server.url("/package.msix"),
        payload,
        "missing-etag-key",
    );
    seed_partial_without_etag(&cache.0, &request, b"old partial");

    let verified = manager(1, None)
        .download(request, CancellationToken::new())
        .await
        .expect("missing validator restarts from zero");

    assert_eq!(fs::read(verified.path).expect("replacement"), payload);
    assert!(!server.requests()[0].to_ascii_lowercase().contains("range:"));
}

#[tokio::test(flavor = "multi_thread")]
async fn changed_etag_discards_partial_and_restarts_without_range() {
    let payload = b"new complete payload";
    let cache = TestDirectory::new("etag-change");
    let server = TestServer::start(vec![
        ResponseSpec {
            status: 206,
            headers: vec![
                ("ETag", "v2".to_owned()),
                (
                    "Content-Range",
                    format!("bytes 4-{}/{}", payload.len() - 1, payload.len()),
                ),
            ],
            body: payload[4..].to_vec(),
            delay: Duration::ZERO,
        },
        ResponseSpec::ok(payload).with_header("ETag", "v2"),
    ]);
    let request = request(&cache.0, server.url("/package.msix"), payload, "etag-key");
    seed_partial(&cache.0, &request, b"old!", "v1");

    let verified = manager(1, None)
        .download(request, CancellationToken::new())
        .await
        .expect("restart succeeds");

    assert_eq!(fs::read(verified.path).expect("restarted payload"), payload);
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests[0].to_ascii_lowercase().contains("range: bytes=4-"));
    assert!(!requests[1].to_ascii_lowercase().contains("range:"));
}

#[tokio::test(flavor = "multi_thread")]
async fn ignored_range_or_wrong_content_range_restarts_from_zero() {
    let payload = b"complete replacement";
    let cache = TestDirectory::new("range-restart");
    let ignored = TestServer::start(vec![ResponseSpec::ok(payload)]);
    let ignored_request = request(
        &cache.0,
        ignored.url("/ignored-range"),
        payload,
        "ignored-key",
    );
    seed_partial(&cache.0, &ignored_request, b"old", "v1");
    let verified = manager(1, None)
        .download(ignored_request, CancellationToken::new())
        .await
        .expect("200 response restarts from zero");
    assert_eq!(fs::read(verified.path).expect("replacement"), payload);

    let wrong_range = TestServer::start(vec![
        ResponseSpec {
            status: 206,
            headers: vec![
                ("ETag", "v1".to_owned()),
                (
                    "Content-Range",
                    format!("bytes 1-{}/{}", payload.len() - 1, payload.len()),
                ),
            ],
            body: payload[3..].to_vec(),
            delay: Duration::ZERO,
        },
        ResponseSpec::ok(payload).with_header("ETag", "v1"),
    ]);
    let wrong_request = request(
        &cache.0,
        wrong_range.url("/wrong-range"),
        payload,
        "wrong-key",
    );
    seed_partial(&cache.0, &wrong_request, b"old", "v1");
    let verified = manager(1, None)
        .download(wrong_request, CancellationToken::new())
        .await
        .expect("wrong Content-Range restarts from zero");
    assert_eq!(fs::read(verified.path).expect("range replacement"), payload);
    assert_eq!(wrong_range.requests().len(), 2);

    let changed_length = TestServer::start(vec![
        ResponseSpec {
            status: 206,
            headers: vec![
                ("ETag", "v1".to_owned()),
                (
                    "Content-Range",
                    format!("bytes 3-{}/999", payload.len() - 1),
                ),
            ],
            body: payload[3..].to_vec(),
            delay: Duration::ZERO,
        },
        ResponseSpec::ok(payload).with_header("ETag", "v1"),
    ]);
    let length_request = request(
        &cache.0,
        changed_length.url("/changed-length"),
        payload,
        "length-key",
    );
    seed_partial(&cache.0, &length_request, b"old", "v1");
    manager(1, None)
        .download(length_request, CancellationToken::new())
        .await
        .expect("changed total length restarts from zero");
    assert_eq!(changed_length.requests().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn expired_url_is_refreshed_once_without_persisting_it() {
    let payload = b"refreshed payload";
    let expired = TestServer::start(vec![ResponseSpec {
        status: 403,
        headers: Vec::new(),
        body: Vec::new(),
        delay: Duration::ZERO,
    }]);
    let fresh = TestServer::start(vec![ResponseSpec::ok(payload)]);
    let cache = TestDirectory::new("refresh");
    let request = request(
        &cache.0,
        expired.url("/expired?token=secret"),
        payload,
        "refresh-key",
    );
    let debug = format!("{request:?}");
    assert!(!debug.contains("token=secret"));
    assert!(!debug.contains(&request.url));
    let refreshes = Arc::new(AtomicUsize::new(0));
    let refresh_count = Arc::clone(&refreshes);
    let fresh_url = fresh.url("/fresh?token=other-secret");

    let verified = manager(1, None)
        .download_with_refresh(request, CancellationToken::new(), move |_| {
            refresh_count.fetch_add(1, Ordering::SeqCst);
            let value = fresh_url.clone();
            async move { Ok(value) }
        })
        .await
        .expect("refreshed download");

    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(fs::read(verified.path).expect("refreshed payload"), payload);
    let metadata = fs::read_dir(cache.0.join("partial"))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read(entry.path()).ok())
        .flatten()
        .collect::<Vec<_>>();
    let metadata = String::from_utf8_lossy(&metadata);
    assert!(!metadata.contains("token=secret"));
    assert!(!metadata.contains("token=other-secret"));
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_preserves_partial_and_hash_mismatch_never_promotes() {
    let payload = b"expected payload";
    let cache = TestDirectory::new("cancel-hash");
    let cancelled_request = request(
        &cache.0,
        "http://127.0.0.1:9/not-contacted".to_owned(),
        payload,
        "cancel-key",
    );
    seed_partial(&cache.0, &cancelled_request, b"partial", "v1");
    let token = CancellationToken::new();
    token.cancel();

    let error = manager(1, None)
        .download(cancelled_request, token)
        .await
        .expect_err("cancelled before network");
    assert_eq!(error, DownloadError::Cancelled);
    assert!(cache.0.join("partial/cancel-key.part").exists());

    let corrupt = b"wrong bytes here";
    let server = TestServer::start(vec![ResponseSpec::ok(corrupt)]);
    let hash_request = request(&cache.0, server.url("/package.msix"), payload, "hash-key");
    let error = manager(1, None)
        .download(hash_request, CancellationToken::new())
        .await
        .expect_err("hash mismatch");
    assert_eq!(error, DownloadError::HashMismatch);
    assert!(!cache
        .0
        .join("verified")
        .join(format!("{}.msix", sha256(payload)))
        .exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_interrupts_waiting_for_response_headers() {
    let payload = b"late response";
    let server = TestServer::start(vec![ResponseSpec {
        delay: Duration::from_millis(500),
        ..ResponseSpec::ok(payload)
    }]);
    let cache = TestDirectory::new("cancel-headers");
    let request = request(&cache.0, server.url("/slow"), payload, "slow-key");
    seed_partial(&cache.0, &request, b"late", "v1");
    let token = CancellationToken::new();
    let cancellation = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancellation.cancel();
    });

    let started = Instant::now();
    let error = manager(1, None)
        .download(request, token)
        .await
        .expect_err("slow response should be cancellable");

    assert_eq!(error, DownloadError::Cancelled);
    assert!(started.elapsed() < Duration::from_millis(250));
    assert!(cache.0.join("partial/slow-key.part").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_interrupts_rate_limit_waits() {
    let payload = vec![5_u8; 2048];
    let server = TestServer::start(vec![ResponseSpec::ok(&payload).with_header("ETag", "v1")]);
    let cache = TestDirectory::new("cancel-rate-limit");
    let request = request(&cache.0, server.url("/slow-rate"), &payload, "rate-key");
    let token = CancellationToken::new();
    let cancellation = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        cancellation.cancel();
    });

    let started = Instant::now();
    let error = manager(1, Some(1024))
        .download(request, token)
        .await
        .expect_err("rate limit wait should be cancellable");

    assert_eq!(error, DownloadError::Cancelled);
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(cache.0.join("partial/rate-key.part").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limit_does_not_accumulate_unbounded_idle_credit() {
    let payload = vec![9_u8; 1024];
    let server = TestServer::start(vec![ResponseSpec::ok(&payload).with_header("ETag", "v1")]);
    let cache = TestDirectory::new("rate-idle");
    let request = request(
        &cache.0,
        server.url("/rate-idle"),
        &payload,
        "rate-idle-key",
    );
    let manager = manager(1, Some(2048));
    tokio::time::sleep(Duration::from_millis(600)).await;

    let started = Instant::now();
    manager
        .download(request, CancellationToken::new())
        .await
        .expect("idle manager remains rate limited");

    assert!(started.elapsed() >= Duration::from_millis(400));
}

#[tokio::test(flavor = "multi_thread")]
async fn chunked_response_stops_as_soon_as_expected_size_is_exceeded() {
    let expected = b"expected";
    let oversized = vec![7_u8; 4096];
    let server = TestServer::start(vec![ResponseSpec::ok(&oversized)
        .with_header("ETag", "v1")
        .with_header("X-Fixture-Chunk-Delay", "500")]);
    let cache = TestDirectory::new("chunked-overflow");
    let request = request(
        &cache.0,
        server.url("/chunked-overflow"),
        expected,
        "overflow-key",
    );

    let started = Instant::now();
    let error = manager(1, None)
        .download(request, CancellationToken::new())
        .await
        .expect_err("oversized stream must stop before EOF");

    assert_eq!(error, DownloadError::SizeMismatch);
    assert!(started.elapsed() < Duration::from_millis(250));
    assert!(!cache.0.join("partial/overflow-key.part").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_prevents_verification_promotion() {
    let directory = TestDirectory::new("cancel-verification");
    let partial = directory.0.join("partial.msix.part");
    let verified = directory.0.join("verified.msix");
    let payload = vec![9_u8; 2 * 1024 * 1024];
    fs::write(&partial, &payload).expect("partial payload");
    let cancellation_checks = AtomicUsize::new(0);

    let error = verify_and_promote_cancellable(
        &partial,
        &verified,
        payload.len() as u64,
        &sha256(&payload),
        || cancellation_checks.fetch_add(1, Ordering::SeqCst) >= 2,
    )
    .await
    .expect_err("cancelled verification must not promote");

    assert_eq!(error, VerificationError::Cancelled);
    assert!(cancellation_checks.load(Ordering::SeqCst) >= 3);
    assert!(partial.exists());
    assert!(!verified.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn case_aliases_of_the_same_cache_key_are_serialized() {
    let payload = vec![3_u8; 512];
    let server = TestServer::start(vec![
        ResponseSpec {
            delay: Duration::from_millis(100),
            ..ResponseSpec::ok(&payload).with_header("ETag", "v1")
        },
        ResponseSpec {
            delay: Duration::from_millis(100),
            ..ResponseSpec::ok(&payload).with_header("ETag", "v1")
        },
    ]);
    let cache = TestDirectory::new("duplicate-key");
    let requests = vec![
        request(&cache.0, server.url("/first"), &payload, "shared-key"),
        request(&cache.0, server.url("/second"), &payload, "SHARED-KEY"),
    ];

    let results = manager(2, None)
        .download_many(requests, CancellationToken::new())
        .await;

    assert!(results.iter().all(Result::is_ok));
    assert_eq!(server.max_active.load(Ordering::SeqCst), 1);
}

#[test]
fn download_errors_map_to_stable_redacted_frontend_codes() {
    let expired = AppErrorDto::from(&DownloadError::UrlExpired);
    assert_eq!(expired.code, ErrorCode::DownloadUrlExpired);
    assert_eq!(expired.retry, RetryAdvice::ReResolve);
    assert!(expired.details.is_empty());

    let expired_status = AppErrorDto::from(&DownloadError::UrlExpiredStatus(403));
    assert_eq!(expired_status.code, ErrorCode::DownloadUrlExpired);
    assert_eq!(expired_status.retry, RetryAdvice::ReResolve);
    assert_eq!(
        serde_json::to_value(expired_status.details).expect("safe expired status detail"),
        serde_json::json!([{ "kind": "http_status", "status": 403 }])
    );

    let hash = AppErrorDto::from(&DownloadError::HashMismatch);
    assert_eq!(hash.code, ErrorCode::HashMismatch);
    assert_eq!(hash.retry, RetryAdvice::ReResolve);
    assert!(hash.details.is_empty());

    for (transport, expected_code) in [
        (
            DownloadTransportError::ProxyConnection,
            ErrorCode::DownloadProxyFailed,
        ),
        (DownloadTransportError::Timeout, ErrorCode::DownloadTimeout),
        (
            DownloadTransportError::Connection,
            ErrorCode::DownloadConnectionFailed,
        ),
        (
            DownloadTransportError::Request,
            ErrorCode::DownloadConnectionFailed,
        ),
        (
            DownloadTransportError::ResponseBody,
            ErrorCode::DownloadResponseFailed,
        ),
    ] {
        let error = AppErrorDto::from(&DownloadError::Transport(transport));
        assert_eq!(error.code, expected_code);
        assert_eq!(error.retry, RetryAdvice::Retry);
        assert!(error.details.is_empty());
    }

    let proxy_auth = AppErrorDto::from(&DownloadError::HttpStatus(407));
    assert_eq!(proxy_auth.code, ErrorCode::DownloadProxyAuthRequired);
    assert_eq!(proxy_auth.retry, RetryAdvice::Never);
    assert_eq!(
        serde_json::to_value(proxy_auth.details).expect("safe proxy status detail"),
        serde_json::json!([{ "kind": "http_status", "status": 407 }])
    );

    let status = AppErrorDto::from(&DownloadError::HttpStatus(502));
    assert_eq!(status.code, ErrorCode::DownloadHttpStatus);
    assert_eq!(status.retry, RetryAdvice::Retry);
    assert_eq!(
        serde_json::to_value(status.details).expect("safe status detail"),
        serde_json::json!([{ "kind": "http_status", "status": 502 }])
    );

    for (source, expected_code, expected_retry) in [
        (
            DownloadError::RedirectRejected,
            ErrorCode::DownloadRedirectRejected,
            RetryAdvice::Never,
        ),
        (
            DownloadError::Io,
            ErrorCode::DownloadIoFailed,
            RetryAdvice::Retry,
        ),
        (
            DownloadError::InvalidRequest,
            ErrorCode::DownloadFailed,
            RetryAdvice::Never,
        ),
    ] {
        let error = AppErrorDto::from(&source);
        assert_eq!(error.code, expected_code);
        assert_eq!(error.retry, expected_retry);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn redirect_policy_concurrency_and_rate_limit_are_enforced() {
    let redirect = TestServer::start(vec![ResponseSpec {
        status: 302,
        headers: vec![(
            "Location",
            "https://attacker.example/package.msix".to_owned(),
        )],
        body: Vec::new(),
        delay: Duration::ZERO,
    }]);
    let cache = TestDirectory::new("limits");
    let redirect_request = request(
        &cache.0,
        redirect.url("/redirect"),
        b"unused",
        "redirect-key",
    );
    assert_eq!(
        manager(1, None)
            .download(redirect_request, CancellationToken::new())
            .await
            .expect_err("redirect must be rejected"),
        DownloadError::RedirectRejected
    );

    let payload = vec![7_u8; 2048];
    let slow_server = TestServer::start(vec![
        ResponseSpec {
            delay: Duration::from_millis(100),
            ..ResponseSpec::ok(&payload)
        },
        ResponseSpec {
            delay: Duration::from_millis(100),
            ..ResponseSpec::ok(&payload)
        },
    ]);
    let requests = vec![
        request(&cache.0, slow_server.url("/one"), &payload, "one"),
        request(&cache.0, slow_server.url("/two"), &payload, "two"),
    ];
    let started = Instant::now();
    let results: Vec<Result<VerifiedDownload, DownloadError>> = manager(1, Some(4096))
        .download_many(requests, CancellationToken::new())
        .await;

    assert!(results.iter().all(Result::is_ok));
    assert_eq!(slow_server.max_active.load(Ordering::SeqCst), 1);
    assert!(started.elapsed() >= Duration::from_millis(900));
}
