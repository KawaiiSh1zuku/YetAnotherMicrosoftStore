#![cfg(windows)]

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use sha2::{Digest, Sha256};
use yet_another_microsoft_store_lib::{
    cache::CacheManager,
    domain::{CacheEntry, CacheState},
    download::VerifiedDownload,
    persistence::Persistence,
};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("yamstore-m4-cache-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn entry(
    key: &str,
    job_id: Option<&str>,
    path: &Path,
    bytes: &[u8],
    state: CacheState,
    last_accessed_at: i64,
) -> CacheEntry {
    CacheEntry {
        cache_key: key.to_owned(),
        job_id: job_id.map(str::to_owned),
        update_id: format!("update-{key}"),
        path: path.to_string_lossy().into_owned(),
        size: bytes.len() as u64,
        sha256: hash(bytes),
        state,
        last_accessed_at,
    }
}

fn write_entry(store: &Persistence, entry: &CacheEntry, bytes: &[u8]) {
    let path = Path::new(&entry.path);
    fs::create_dir_all(path.parent().expect("entry parent")).expect("create entry parent");
    fs::write(path, bytes).expect("write cache payload");
    store.upsert_cache_entry(entry).expect("save cache entry");
}

fn create_junction(link: &Path, target: &Path) {
    let output = Command::new("cmd")
        .arg("/C")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .expect("start mklink");
    assert!(
        output.status.success(),
        "create test junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cache_manager_rejects_preexisting_reparse_subdirectories() {
    let directory = TestDirectory::new("reparse-subdirectory");
    let cache_root = directory.0.join("cache");
    let outside = directory.0.join("outside");
    fs::create_dir_all(&cache_root).expect("cache root");
    fs::create_dir_all(&outside).expect("outside root");
    create_junction(&cache_root.join("partial"), &outside);

    assert!(CacheManager::new(&cache_root).is_err());
    assert!(fs::read_dir(&outside)
        .expect("outside remains readable")
        .next()
        .is_none());
}

#[test]
fn cache_manager_rejects_a_reparse_leaf_file() {
    let directory = TestDirectory::new("reparse-leaf");
    let database = directory.0.join("state.sqlite3");
    let cache_root = directory.0.join("cache");
    let outside = directory.0.join("outside");
    let manager = CacheManager::new(&cache_root).expect("cache manager");
    fs::create_dir_all(&outside).expect("outside root");
    let reparse_leaf = cache_root.join("verified").join("alias.msix");
    create_junction(&reparse_leaf, &outside);
    let store = Persistence::open(&database).expect("open database");
    let download = VerifiedDownload {
        update_id: "update-reparse".to_owned(),
        cache_key: "reparse".to_owned(),
        path: reparse_leaf,
        size: 0,
        sha256: hash(b""),
    };

    assert!(manager.record_verified(&store, &download, None, 0).is_err());
    assert!(store
        .cache_entry("reparse")
        .expect("cache lookup")
        .is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn reconciliation_recovers_partials_and_removes_missing_corrupt_or_escaped_entries() {
    let directory = TestDirectory::new("reconcile");
    let database = directory.0.join("state.sqlite3");
    let cache_root = directory.0.join("cache");
    let verified_root = cache_root.join("verified");
    let partial_root = cache_root.join("partial");
    fs::create_dir_all(&verified_root).expect("verified root");
    fs::create_dir_all(&partial_root).expect("partial root");
    let store = Persistence::open(&database).expect("open database");

    let valid_bytes = b"valid verified payload";
    let valid = entry(
        "valid",
        None,
        &verified_root.join(format!("{}.msix", hash(valid_bytes))),
        valid_bytes,
        CacheState::Verified,
        900,
    );
    write_entry(&store, &valid, valid_bytes);

    let missing = entry(
        "missing",
        None,
        &verified_root.join("missing.msix"),
        b"missing",
        CacheState::Verified,
        900,
    );
    store
        .upsert_cache_entry(&missing)
        .expect("save missing entry");

    let expected = b"expected bytes";
    let corrupt = entry(
        "corrupt",
        None,
        &verified_root.join("corrupt.msix"),
        expected,
        CacheState::Verified,
        900,
    );
    write_entry(&store, &corrupt, b"corrupt bytes");

    let outside = directory.0.join("outside.msix");
    fs::write(&outside, b"outside").expect("outside payload");
    let escaped = entry(
        "escaped",
        None,
        &outside,
        b"outside",
        CacheState::Verified,
        900,
    );
    store
        .upsert_cache_entry(&escaped)
        .expect("save escaped entry");

    let partial = b"partial bytes";
    fs::write(partial_root.join("recover-key.part"), partial).expect("partial payload");
    fs::write(
        partial_root.join("recover-key.json"),
        serde_json::to_vec(&serde_json::json!({
            "jobId": "active-job",
            "updateId": "update-recovered",
            "etag": "fixture-etag",
            "expectedSize": 4096,
            "expectedSha256": "a".repeat(64),
            "lastAccessedAt": 100
        }))
        .expect("partial metadata"),
    )
    .expect("write partial metadata");

    let manager = CacheManager::new(&cache_root).expect("cache manager");
    let report = manager
        .reconcile_and_evict(
            &store,
            1_000,
            1024 * 1024,
            30,
            &HashSet::from(["active-job".to_owned()]),
        )
        .await
        .expect("reconcile cache");

    assert_eq!(report.recovered_partials, 1);
    assert_eq!(report.removed_missing, 1);
    assert_eq!(report.removed_corrupt, 1);
    assert_eq!(report.removed_unsafe, 1);
    assert_eq!(report.removed_orphan_verified, 0);
    assert!(store.cache_entry("valid").expect("valid entry").is_some());
    let recovered = store
        .cache_entry("recover-key")
        .expect("recovered entry")
        .expect("partial recovered");
    assert_eq!(recovered.state, CacheState::Partial);
    assert_eq!(recovered.job_id.as_deref(), Some("active-job"));
    assert!(
        outside.exists(),
        "unsafe paths are deindexed but never deleted"
    );
    assert!(!Path::new(&corrupt.path).exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn startup_reconciliation_removes_unindexed_verified_files() {
    let directory = TestDirectory::new("orphan-verified");
    let database = directory.0.join("state.sqlite3");
    let cache_root = directory.0.join("cache");
    let orphan_verified = cache_root.join("verified").join("orphan.msix");
    fs::create_dir_all(orphan_verified.parent().expect("verified parent")).expect("verified root");
    fs::write(&orphan_verified, b"orphan").expect("orphan verified payload");
    let store = Persistence::open(&database).expect("open database");

    let report = CacheManager::new(&cache_root)
        .expect("cache manager")
        .reconcile_and_evict(&store, 1_000, 1024 * 1024, 30, &HashSet::new())
        .await
        .expect("reconcile startup cache");

    assert_eq!(report.removed_orphan_verified, 1);
    assert!(!orphan_verified.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_then_lru_eviction_preserves_active_partial_and_newest_verified() {
    let directory = TestDirectory::new("evict");
    let database = directory.0.join("state.sqlite3");
    let cache_root = directory.0.join("cache");
    let verified_root = cache_root.join("verified");
    let partial_root = cache_root.join("partial");
    let store = Persistence::open(&database).expect("open database");

    let expired = entry(
        "expired",
        None,
        &verified_root.join("expired.msix"),
        b"1111",
        CacheState::Verified,
        1,
    );
    let old = entry(
        "old",
        None,
        &verified_root.join("old.msix"),
        b"2222",
        CacheState::Verified,
        190_000,
    );
    let newest = entry(
        "newest",
        None,
        &verified_root.join("newest.msix"),
        b"3333",
        CacheState::Verified,
        199_000,
    );
    let active = entry(
        "active",
        Some("active-job"),
        &partial_root.join("active.part"),
        b"4444",
        CacheState::Partial,
        1,
    );
    for (entry, bytes) in [
        (&expired, b"1111".as_slice()),
        (&old, b"2222".as_slice()),
        (&newest, b"3333".as_slice()),
        (&active, b"4444".as_slice()),
    ] {
        write_entry(&store, entry, bytes);
    }

    let manager = CacheManager::new(&cache_root).expect("cache manager");
    let report = manager
        .reconcile_and_evict(
            &store,
            200_000,
            8,
            1,
            &HashSet::from(["active-job".to_owned()]),
        )
        .await
        .expect("evict cache");

    assert_eq!(report.evicted_by_age, 1);
    assert_eq!(report.evicted_by_size, 1);
    assert_eq!(report.bytes_retained, 8);
    assert!(store.cache_entry("expired").expect("expired").is_none());
    assert!(store.cache_entry("old").expect("old").is_none());
    assert!(store.cache_entry("newest").expect("newest").is_some());
    assert!(store.cache_entry("active").expect("active").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn evicting_one_cache_key_preserves_a_shared_content_file() {
    let directory = TestDirectory::new("shared-content");
    let database = directory.0.join("state.sqlite3");
    let cache_root = directory.0.join("cache");
    let store = Persistence::open(&database).expect("open database");
    let payload = b"shared verified payload";
    let shared_path = cache_root
        .join("verified")
        .join(format!("{}.msix", hash(payload)));
    let first = entry(
        "first",
        None,
        &shared_path,
        payload,
        CacheState::Verified,
        10,
    );
    let second = entry(
        "second",
        None,
        &shared_path,
        payload,
        CacheState::Verified,
        100_000,
    );
    write_entry(&store, &first, payload);
    store.upsert_cache_entry(&second).expect("save second key");

    let report = CacheManager::new(&cache_root)
        .expect("cache manager")
        .reconcile_and_evict(&store, 100_000, payload.len() as u64, 1, &HashSet::new())
        .await
        .expect("retain one physical blob");

    assert_eq!(report.evicted_by_age, 1);
    assert_eq!(report.evicted_by_size, 0);
    assert!(store.cache_entry("first").expect("first key").is_none());
    assert!(store.cache_entry("second").expect("second key").is_some());
    assert!(
        shared_path.exists(),
        "shared content remains for the live key"
    );
}

#[test]
fn verified_download_is_recorded_without_a_remote_url() {
    let directory = TestDirectory::new("record");
    let database = directory.0.join("state.sqlite3");
    let cache_root = directory.0.join("cache");
    let store = Persistence::open(&database).expect("open database");
    let bytes = b"verified";
    let path = cache_root
        .join("verified")
        .join(format!("{}.msix", hash(bytes)));
    fs::create_dir_all(path.parent().expect("verified parent")).expect("verified parent");
    fs::write(&path, bytes).expect("verified payload");
    let verified = VerifiedDownload {
        update_id: "update-main".to_owned(),
        cache_key: "recorded".to_owned(),
        path,
        size: bytes.len() as u64,
        sha256: hash(bytes),
    };

    CacheManager::new(&cache_root)
        .expect("cache manager")
        .record_verified(&store, &verified, Some("job-record"), 500)
        .expect("record verified download");

    let recorded = store
        .cache_entry("recorded")
        .expect("recorded entry")
        .expect("entry exists");
    assert_eq!(recorded.state, CacheState::Verified);
    assert_eq!(recorded.job_id.as_deref(), Some("job-record"));
    drop(store);
    let database_bytes = fs::read(database).expect("database bytes");
    let database_text = String::from_utf8_lossy(&database_bytes);
    assert!(!database_text.contains("https://"));
    assert!(!database_text.contains("token="));
}
