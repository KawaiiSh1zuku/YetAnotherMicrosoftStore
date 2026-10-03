# M4 Download, Cache, and Proxy Implementation Plan

> **Status:** Complete on 2026-10-02 at E1 plus a controlled online DCAT/FE3 adapter smoke. Real Microsoft CDN payload download, package signature validation, and deployment were not performed.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the M4 network boundary: explicit live Store smoke, runtime-only proxy configuration, resumable verified downloads, and recoverable cache state without persisting signed URLs or credentials.

**Architecture:** Keep Store protocol access behind the existing project adapters. Add focused `settings`, `download`, `verification`, and `cache` modules; the downloader owns HTTP and partial files, verification owns size/hash and atomic promotion, and cache reconciliation owns SQLite/filesystem convergence and eviction. Production accepts HTTPS hosts from an explicit allowlist; local tests may opt into loopback HTTP only.

**Tech Stack:** Rust 2021, `reqwest 0.12.28`, Tokio, SHA-256, rusqlite, local `TcpListener` HTTP fixtures, existing `storelib_rs 0.1.11` adapters.

**Spec:** `docs/superpowers/specs/2026-10-01-third-party-store-client-design.md`

## Global Constraints

- Windows 10 x64 remains the only real deployment evidence baseline; M4 must not mutate installed packages.
- `storelib_rs` remains isolated behind project-owned adapters and DTOs.
- Only HTTPS package URLs on an explicit host allowlist are accepted in production; every redirect is rechecked.
- Signed URLs, response bodies, tokens, proxy passwords, and credential-manager secrets never enter SQLite, frontend DTOs, or logs.
- Proxy modes are disabled, current-user Windows static system proxy, custom HTTP(S), and SOCKS5; PAC/WPAD is not claimed by this milestone and pure auto-configuration is rejected explicitly.
- MSIXVC, EXE, and MSI remain outside the first-stage download/deployment path.
- M4 evidence must distinguish local fixture coverage from the opt-in live Store smoke.

## Review Focus

- A resumed response with a changed ETag or invalid `Content-Range` must restart safely rather than append unrelated bytes; Task 2 tests this.
- A redirect from an allowed CDN to an unlisted host must fail before the redirected request; Task 1 tests this.
- Cancellation must preserve a resumable partial file but never promote it to verified; Task 2 tests this.
- Reconciliation must delete or downgrade stale verified metadata when files are missing or corrupt, without deleting valid entries; Task 3 tests this.
- Cache eviction must never remove partial files owned by active jobs and must apply age before least-recently-used size pressure; Task 3 tests this.

---

### Task 1: Runtime Proxy, URL Policy, and Live Smoke

**Files:**
- Create: `src-tauri/src/settings.rs`
- Create: `src-tauri/tests/m4_network_policy.rs`
- Create: `src-tauri/tests/m4_live_store_smoke.rs`
- Modify: `src-tauri/src/catalog.rs`
- Modify: `src-tauri/src/resolver.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/Cargo.lock`

**Interfaces:**
- Consumes: `domain::AppSettings`, `domain::ProxyMode`, `catalog::CatalogProvider`, `resolver::PackageResolver`.
- Produces: `ProxyProvider::configure(&AppSettings, Option<ProxyCredentials>, reqwest::ClientBuilder) -> Result<reqwest::ClientBuilder, SettingsError>`; `NetworkPolicy::validate_url(&Url) -> Result<(), NetworkPolicyError>`; locale-aware production adapter constructors.

- [x] **Step 1: Write the failing policy and proxy tests**

Cover disabled/system/custom HTTP(S)/SOCKS5, missing runtime credentials, invalid host/port, initial URL policy, redirect policy, and redacted debug/error output.

- [x] **Step 2: Verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m4_network_policy`

Expected: FAIL because `settings` and policy interfaces do not exist.

- [x] **Step 3: Implement the minimal settings and network-policy boundary**

Custom credentials exist only in `ProxyCredentials` at runtime. `System` reads the current user's static WinHTTP proxy configuration and uses only a general or explicit HTTPS route; HTTP-only, PAC, WPAD, and automatic-detection configurations are not silently reused. A test-only constructor permits HTTP only for loopback addresses.

- [x] **Step 4: Add the opt-in live smoke**

The ignored test requires `M4_LIVE_SMOKE=1`, `M4_PRODUCT_ID`, `M4_MARKET`, and `M4_LANGUAGE`, prints only those fields plus UTC time and normalized counts, and asserts that product lookup and package resolution return non-empty project DTOs. It never prints or snapshots package URLs.

- [x] **Step 5: Verify GREEN**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m4_network_policy`

Expected: all focused tests PASS; live smoke remains ignored unless explicitly enabled.

### Task 2: Resumable Download and Streaming Verification

**Files:**
- Create: `src-tauri/src/download.rs`
- Create: `src-tauri/src/verification.rs`
- Create: `src-tauri/tests/m4_download.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `settings::NetworkPolicy`, a selected `resolver::ResolvedPackage`, and ephemeral URL refresh callbacks.
- Produces: `DownloadManager::download(DownloadRequest, CancellationToken) -> Result<VerifiedDownload, DownloadError>` and bounded `download_many`; `VerifiedDownload` exposes only local path, size, hash, update ID, and cache key.

- [x] **Step 1: Write the failing local HTTP fixture tests**

Cover fresh download, valid Range resume, server ignoring Range, changed ETag, wrong `Content-Range`, expired URL refresh once, cancellation, size/hash mismatch, redirect rejection, concurrency bound, and byte-rate configuration.

- [x] **Step 2: Verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m4_download`

Expected: FAIL because download and verification modules do not exist.

- [x] **Step 3: Implement minimal resumable transfer**

Keep `<cache>/partial/<cache-key>.part` and a sidecar containing only update ID, ETag, expected size/hash, and access time. Send `Range` plus `If-Range`; append only for a matching 206 response, otherwise truncate and restart. Treat 401/403/404/410 as one re-resolution request.

- [x] **Step 4: Implement streaming verification and atomic promotion**

Hash existing resumed bytes and incoming chunks, enforce the exact expected length/hash, flush/sync the partial, then atomically rename into `<cache>/verified/<sha256>.<extension>`. Cancellation retains partial state and never creates a verified file.

- [x] **Step 5: Implement bounded scheduling**

Use a Tokio semaphore for `max_concurrent_downloads`; apply the configured aggregate byte-per-second cap in the streaming loop without holding SQLite transactions or blocking the async runtime.

- [x] **Step 6: Verify GREEN**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m4_download`

Expected: all focused tests PASS.

### Task 3: Cache Reconciliation, Eviction, and Milestone Evidence

**Files:**
- Create: `src-tauri/src/cache.rs`
- Create: `src-tauri/tests/m4_cache.rs`
- Modify: `src-tauri/src/persistence.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `task_plan.md`
- Modify: `README.md`
- Modify: `docs/support-matrix.md`
- Modify: `docs/superpowers/plans/2026-10-01-third-party-store-client-implementation-plan.md`
- Modify: `progress.md`
- Modify: `findings.md`

**Interfaces:**
- Consumes: M2 `CacheEntry`/`CacheState` and Task 2 `VerifiedDownload`.
- Produces: persistence list/delete/touch operations and `CacheManager::reconcile_and_evict(now, max_bytes, retention_days, active_job_ids) -> Result<CacheReport, CacheError>`.

- [x] **Step 1: Write the failing recovery and eviction tests**

Cover missing/corrupt verified files, orphan partials, valid restart recovery, age eviction, LRU size eviction, active-job protection, and a database text scan proving no temporary URL is stored.

- [x] **Step 2: Verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m4_cache`

Expected: FAIL because cache reconciliation and repository operations do not exist.

- [x] **Step 3: Implement reconciliation and eviction**

Reconcile metadata against canonical paths under the configured cache root, refuse root escapes/reparse points, hash verified files before trusting them, preserve active partials, then apply retention and LRU limits to inactive entries.

- [x] **Step 4: Verify focused and full suites**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m4_cache`

Expected: all focused tests PASS.

Run: `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`

Expected: all automated tests PASS and only the documented M0/live tests are ignored.

- [x] **Step 5: Update documentation and evidence boundaries**

Mark M4 complete only if both local HTTP fixtures and the opt-in live smoke pass. If live access is blocked, record M4 as locally implemented with the E2 smoke gate open; do not claim production Store/download acceptance.

- [x] **Step 6: Run final gates**

Run: `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`

Run: `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`

Run: `cargo check --manifest-path src-tauri/broker/Cargo.toml`

Run: `pnpm build`

Run: `git diff --check && git diff --cached --check`

Expected: all commands exit 0; no generated Broker/frontend artifact is staged.

## Completion Review

- The controlled live smoke succeeded for product `9WZDNCRFJ3TJ`, market `US`, language `en`, observing 20 packages and 81 dependency edges without logging or persisting signed URLs.
- Windows live FE3 data exposed ARM32 monikers; `Architecture::Arm` and an offline regression fixture were added instead of misclassifying them as ARM64 or rejecting the entire graph.
- Local socket fixtures cover fresh and resumed transfers, missing/changed ETags, invalid `Content-Range` start/total, ignored Range, redirect rejection, URL refresh, cancellation during headers/stream/rate waits, aggregate throttling, concurrency, size/SHA-256 failure, and verified promotion.
- System proxy behavior is intentionally limited to current-user static WinHTTP/Internet Options values. PAC, auto-detect, and WPAD require a future per-URL Windows resolver and are not silently approximated.
- Cache tests cover orphan partial recovery, verified-file reconciliation, root confinement, active partial protection, age then LRU eviction, and a SQLite byte scan proving signed URLs are absent.
- Independent review regressions cover reparse-point confinement, verification-time cancellation, bounded idle rate credit, shared physical blobs, case-insensitive same-key serialization, early chunked overflow rejection, and startup orphan cleanup. The final local suite is 15 download, 6 network-policy, and 7 cache tests.
