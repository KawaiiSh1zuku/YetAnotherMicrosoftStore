# Progress, Database, and Cache Maintenance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace append-only intermediate progress events with a lease-fenced single-row projection, add safe database history maintenance, and remove verified packages after successful installation unless retention is enabled.

**Architecture:** Immutable `job_events` remain the source of durable state transitions, while schema v2 adds one mutable `job_progress` row per active job. The worker coalesces progress writes, commits one final progress event per phase, and the frontend merges event `sequence` with `progressRevision`; maintenance and cache cleanup reuse existing persistence and path-safety boundaries.

**Tech Stack:** Rust 2021, rusqlite 0.40.2 bundled SQLite, Tokio, Tauri 2, React 19, TypeScript 6, Vitest, Testing Library.

**Spec:** `docs/superpowers/specs/2026-10-04-progress-database-cache-maintenance-design.md`

## Global Constraints

- Keep `src-tauri/migrations/0001_initial.sql` byte-for-byte unchanged; schema changes go in `0002_job_progress_and_maintenance.sql` and raise `CURRENT_SCHEMA_VERSION` to 2.
- Intermediate progress must not append rows to `job_events`; each active job has at most one `job_progress` row and each phase emits at most one final legacy progress event.
- Every progress mutation validates the worker lease, generation, expected event sequence, stage, phase, and monotonic value before writing.
- Event `sequence` remains the command concurrency token; `progressRevision` is display-only and never enters `expectedSequence`.
- Database cleanup preserves Failed and active jobs, settings, product/package metadata, install associations, and cache entries.
- Cache deletion retains root containment, reparse-point rejection, and shared canonical-path reference protection.
- Cache cleanup failure cannot turn a Windows-confirmed successful installation into a failed job.
- Do not add dependencies, persist URLs or credentials, commit, push, or perform live installation during implementation.
- After all checks pass, stage only reviewed task files and provide an English Conventional Commit command without executing it.

## Review Focus

- A stale worker or reused event sequence attempts progress upsert: reject it without changing `job_progress` or event history (Task 1 tests).
- A phase changes while a coalesced update is pending: force the last valid value, finalize once, and prevent a late callback from recreating the row (Task 2 tests).
- A verified file is shared by multiple cache keys: remove the successful job's index while preserving the physical file until the final reference is removed (Task 3 tests).
- Maintenance sees a live lease, pending command, or active job: reject before deleting any row or running `VACUUM` (Task 4 tests).
- An event page arrives after a newer polled progress snapshot with the same sequence: retain the larger `progressRevision` and current progress (Task 5 tests).

---

### Task 1: Schema V2 and Lease-Fenced Runtime Progress

**Files:**
- Create: `src-tauri/migrations/0002_job_progress_and_maintenance.sql`
- Modify: `src-tauri/src/job_events.rs`
- Modify: `src-tauri/src/job_store.rs`
- Modify: `src-tauri/src/persistence.rs`
- Modify: `src-tauri/tests/m6_event_store.rs`
- Modify: `src-tauri/tests/m2_persistence.rs`

**Interfaces:**
- Produces: `JobProgressPhase::{Downloading, Deploying}`.
- Produces: `JobProgressUpdate::{Download { bytes_done: u64, bytes_total: Option<u64> }, Deployment { percentage: u8 }}`.
- Produces: `JobProgress { job_id: String, phase: JobProgressPhase, revision: u64, update: JobProgressUpdate, updated_at: i64 }`.
- Produces: `Persistence::record_job_progress_leased(job_id, expected_sequence, update, occurred_at, lease, now) -> Result<JobProgress, PersistenceError>`.
- Produces: `Persistence::finalize_job_progress_leased(job_id, expected_sequence, phase, occurred_at, lease, now) -> Result<Option<JobSnapshot>, PersistenceError>`.
- Produces: `Persistence::job_snapshot_with_progress(job_id) -> Result<Option<(JobSnapshot, u64)>, PersistenceError>`.
- Produces: `Persistence::list_job_snapshots_with_progress() -> Result<Vec<(JobSnapshot, u64)>, PersistenceError>`.
- Consumes: existing `job_store::validate_lease`, event folding, projection rebuilding, and `WorkerLease` generation checks.

- [ ] **Step 1: Write the schema migration regression test**

Add `schema_v1_upgrades_to_v2_without_rewriting_history` in `m2_persistence.rs`. Build a v1 database with a product, settings, cache entry, job, and legacy progress event; reopen through `Persistence`; assert schema version 2, unchanged legacy event count, preserved records, and an empty `job_progress` table.

- [ ] **Step 2: Run the migration test and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m2_persistence schema_v1_upgrades_to_v2_without_rewriting_history -- --exact`

Expected: FAIL because schema version remains 1 and `job_progress` does not exist.

- [ ] **Step 3: Add migration 0002 and register schema version 2**

Create `job_progress(job_id, phase, revision, bytes_done, bytes_total, deployment_progress, updated_at)` with phase-specific CHECK constraints, positive revision, monotonic-valid value ranges, primary-key job ID, and `ON DELETE CASCADE`. Register migration 2 without changing migration 1.

- [ ] **Step 4: Run the migration test and verify GREEN**

Run the Step 2 command. Expected: PASS.

- [ ] **Step 5: Write lease, phase, monotonicity, and overlay tests**

In `m6_event_store.rs`, add tests asserting one row is overwritten with incremented revision, duplicate/regressing values are rejected, wrong stage/phase is rejected, stale lease/generation is rejected, event count stays unchanged during intermediate writes, and API-facing overlay changes bytes or deployment percentage without changing event sequence.

- [ ] **Step 6: Run focused event-store tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m6_event_store job_progress -- --nocapture`

Expected: FAIL because runtime progress types and persistence methods do not exist.

- [ ] **Step 7: Implement runtime progress persistence and overlay**

Add project-owned progress types, SQL row parsing, transactional lease/sequence/stage checks, phase-specific monotonic upsert, revision increment, final event append plus row deletion, and presentation-only overlay methods. Keep `job_store::snapshot` and rebuild validation on the event projection so mutable progress cannot corrupt event replay.

- [ ] **Step 8: Run focused persistence suites and verify GREEN**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test m6_event_store
cargo test --manifest-path src-tauri/Cargo.toml --test m2_persistence
```

Expected: both test binaries pass.

### Task 2: Worker Progress Coalescing and Finalization

**Files:**
- Modify: `src-tauri/src/job_worker.rs`
- Modify: `src-tauri/tests/m6_worker.rs`

**Interfaces:**
- Consumes: Task 1 progress types and `record_job_progress_leased` / `finalize_job_progress_leased`.
- Produces: `WorkerConfig::progress_flush_interval: Duration` fixed to 250 ms in production.
- Produces: download and deployment loops that maintain one pending latest update, flush on interval, and force finalization before returning.

- [ ] **Step 1: Write high-frequency worker regression tests**

Extend fake download/deployment ports to emit 0 through 100 rapidly. Assert that an observer can read a nonzero `progressRevision`, that intermediate callbacks create no events, and that each completed phase leaves no `job_progress` row and exactly one final progress event with the final value.

- [ ] **Step 2: Add phase-change, lease-loss, and no-callback tests**

Assert pending progress is flushed before normal completion, stale lease never writes a terminal event, late callbacks cannot recreate a finalized row, and a phase with no callback does not manufacture a progress event.

- [ ] **Step 3: Run focused worker tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m6_worker progress -- --nocapture`

Expected: FAIL because the worker still appends every callback as an event.

- [ ] **Step 4: Implement a private coalescing state machine**

Use Tokio interval ticks and a latest-value slot in `run_download` and `run_deployment`. Persist at most once per configured interval, drain and validate the final queued value when the future resolves, and call Task 1 finalization once before subsequent stage/error handling. Do not spawn detached tasks or add a second writer.

- [ ] **Step 5: Run worker and event-store suites and verify GREEN**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test m6_worker
cargo test --manifest-path src-tauri/Cargo.toml --test m6_event_store
```

Expected: both pass; existing stage ordering, blocking-process recovery, and restart tests remain green.

### Task 3: Successful-Install Verified Cache Lifecycle

**Files:**
- Modify: `src-tauri/src/cache.rs`
- Modify: `src-tauri/src/job_worker.rs`
- Modify: `src-tauri/src/app_runtime.rs`
- Modify: `src-tauri/tests/m4_cache.rs`
- Modify: `src-tauri/tests/m6_worker.rs`
- Modify: `src-tauri/tests/m6_runtime.rs`

**Interfaces:**
- Produces: `CacheManager::remove_verified_for_job(persistence, job_id) -> Result<usize, CacheError>`.
- Produces: `WorkerConfig::keep_installed_payloads: bool`, populated from `AppSettings`.
- Consumes: existing `CacheManager::remove_entry` shared-path protection and task completion/reconciliation paths.
- Consumes: existing `Persistence::record_diagnostic` with `DiagnosticOperation::Storage` for a sanitized cleanup failure after Windows convergence.

- [ ] **Step 1: Write cache reference-lifecycle tests**

In `m4_cache.rs`, assert job-scoped removal deletes matching verified indexes, deletes an unshared physical file, preserves partial entries and other jobs, and retains a shared physical file until its final cache key is removed.

- [ ] **Step 2: Run cache tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m4_cache remove_verified_for_job -- --nocapture`

Expected: FAIL because the method does not exist.

- [ ] **Step 3: Implement job-scoped verified removal**

Filter exact `job_id` plus `CacheState::Verified`, reuse `remove_entry`, and return the number of removed index rows. Preserve all existing path/reparse validation.

- [ ] **Step 4: Write worker success-policy tests**

Assert normal deployment and reconciliation convergence remove verified entries/files when retention is false, retain them when true, and retain them on failure, cancellation, package-in-use, or retry paths. Add a cleanup-failure fixture showing the job still completes after Windows convergence.

- [ ] **Step 5: Run worker policy tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m6_worker installed_payload -- --nocapture`

Expected: FAIL because worker configuration does not consume the setting and successful completion does not release cache.

- [ ] **Step 6: Wire the setting and successful cleanup**

Populate `keep_installed_payloads` in production/test worker configs. Invoke cache release only after confirmed deployment or reconciliation convergence and before returning the final processed outcome; record/report maintenance failure without changing the completed job to Failed.

- [ ] **Step 7: Run cache, worker, and runtime suites and verify GREEN**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test m4_cache
cargo test --manifest-path src-tauri/Cargo.toml --test m6_worker
cargo test --manifest-path src-tauri/Cargo.toml --test m6_runtime
```

Expected: all pass.

### Task 4: Database Maintenance API and Settings Command

**Files:**
- Modify: `src-tauri/src/persistence.rs`
- Modify: `src-tauri/src/tauri_api.rs`
- Modify: `src-tauri/src/app_runtime.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tests/m6_event_store.rs`
- Modify: `src-tauri/tests/m6_api.rs`
- Modify: `src-tauri/tests/m6_runtime.rs`

**Interfaces:**
- Produces: `DatabaseCleanupReport { removed_jobs, removed_events, removed_commands, removed_diagnostics, removed_progress }` with unsigned bounded counts.
- Produces: `Persistence::cleanup_database(now: i64) -> Result<DatabaseCleanupReport, PersistenceError>`.
- Produces: `ApiBackend::cleanup_database()`, `TauriApi::cleanup_database()`, and Tauri command `cleanup_database`.
- Produces: `JobView::progress_revision: u64` and serialized `ApiJobSnapshot::progress_revision: u64`, populated by Task 1 presentation queries.
- Consumes: Task 1 `job_progress`, current worker lease/command tables, and existing error sanitization.

- [ ] **Step 1: Write maintenance transaction tests**

Seed Completed, Cancelled, Failed, and active jobs plus commands, targets, checkpoints, progress, diagnostics, settings, products, associations, and cache entries. Assert cleanup removes only Completed/Cancelled history, nulls their cache `job_id`, preserves every protected record, reports literal counts, and leaves `PRAGMA integrity_check = ok`.

- [ ] **Step 2: Add refusal tests**

Assert an unexpired lease, pending command, or nonterminal/non-Failed job rejects maintenance before any row changes. Assert checkpoint/VACUUM errors surface as sanitized persistence failures without claiming rolled-back row deletion.

- [ ] **Step 3: Run maintenance tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test m2_persistence database_cleanup -- --nocapture`

Expected: FAIL because maintenance/report interfaces do not exist.

- [ ] **Step 4: Implement transaction, checkpoint, and VACUUM**

Count and delete exact terminal-job relationships in an immediate transaction, null cache ownership, commit, execute `PRAGMA wal_checkpoint(TRUNCATE)`, then `VACUUM`. Do not concatenate user-controlled SQL or delete the database file.

- [ ] **Step 5: Write API/runtime boundary tests**

Assert report camelCase serialization, backend invocation, command registration behavior, active-job refusal, and successful report propagation through `TauriApi`.

- [ ] **Step 6: Run API/runtime tests and verify RED**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test m6_api cleanup_database -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml --test m6_runtime cleanup_database -- --nocapture
```

Expected: FAIL until the command is wired.

- [ ] **Step 7: Wire backend and Tauri command**

Add the trait method, production implementation, sanitizing Tauri API method, command wrapper, and invoke-handler registration. Preserve existing `clear_cache` behavior as a separate operation.

- [ ] **Step 8: Run persistence, API, and runtime suites and verify GREEN**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test m6_event_store
cargo test --manifest-path src-tauri/Cargo.toml --test m6_api
cargo test --manifest-path src-tauri/Cargo.toml --test m6_runtime
```

Expected: all pass.

### Task 5: Frontend Dual-Version Refresh and Maintenance Controls

**Files:**
- Modify: `src/lib/types.ts`
- Modify: `src/lib/tauri.ts`
- Modify: `src/features/queue/QueueView.tsx`
- Modify: `src/features/settings/SettingsView.tsx`
- Modify: `src/test/fixtures.ts`
- Modify: `src/test/tauri.test.ts`
- Modify: `src/test/App.test.tsx`

**Interfaces:**
- Consumes: Task 1/4 API fields `progressRevision` and `DatabaseCleanupReport`, plus `cleanup_database`.
- Produces: `StoreClient.cleanDatabase(): Promise<DatabaseCleanupReport>`.
- Produces: snapshot merge order `(sequence, progressRevision)` and a 500 ms queue poll active only while nonterminal jobs exist.

- [ ] **Step 1: Write merge-order unit tests**

Add literal snapshots proving higher progress revision wins at equal sequence, lower progress revision loses, and a higher sequence stage transition wins even when its progress revision resets to zero.

- [ ] **Step 2: Run unit test and verify RED**

Run: `pnpm test -- src/test/tauri.test.ts`

Expected: FAIL because snapshots lack `progressRevision` and merge compares only sequence.

- [ ] **Step 3: Add frontend DTO/client fields and merge logic**

Update types, fixtures, `invoke("cleanup_database")`, and merge comparison without changing command `expectedSequence`.

- [ ] **Step 4: Write queue polling component tests**

Use fake timers to assert 500 ms polling starts for an active job, refreshes same-sequence progress, stops after a terminal snapshot, and is cleared on unmount. Keep event subscription tests green.

- [ ] **Step 5: Run queue tests and verify RED**

Run: `pnpm test -- src/test/App.test.tsx -t "polls active job progress"`

Expected: FAIL because `QueueView` does not poll.

- [ ] **Step 6: Implement conditional queue polling**

Use one effect-owned interval with cleanup and serialized refreshes; merge `listJobs` results through the shared dual-version helper. Do not poll outside the mounted queue or when every job is terminal.

- [ ] **Step 7: Write settings behavior tests**

Assert the retention toggle persists `keepInstalledPayloads`, database cleanup requires confirmation, displays returned counts, calls `cleanDatabase` once, and leaves the existing cache cleanup command separate.

- [ ] **Step 8: Run settings test and verify RED**

Run: `pnpm test -- src/test/App.test.tsx -t "cleans completed task history"`

Expected: FAIL because the controls/client method do not exist.

- [ ] **Step 9: Implement settings controls**

Add a binary retention toggle and a destructive confirm dialog with concise Chinese copy. Show the structured cleanup result in the existing status region and route errors through `localizeError`.

- [ ] **Step 10: Run the full frontend suite and verify GREEN**

Run: `pnpm test`

Expected: all Vitest tests pass with no leaked-timer warnings.

### Task 6: Documentation, Full Verification, Review, and Scoped Staging

**Files:**
- Modify: `docs/diagnostics.md`
- Modify: `docs/superpowers/specs/2026-10-03-admin-runtime-package-management-redesign.md`
- Modify: `docs/superpowers/plans/2026-10-03-admin-runtime-package-management-refactor-plan.md`
- Modify: `docs/superpowers/plans/2026-10-04-progress-database-cache-maintenance.md`
- Review/stage: all files changed by Tasks 1-5

**Interfaces:**
- Consumes: all implemented behavior and final verification evidence.
- Produces: documentation aligned to schema v2, exact E1 evidence, a reviewed staged diff, and an unexecuted Conventional Commit command.

- [ ] **Step 1: Update documentation from observed behavior**

Document mutable progress projection, event/finalization semantics, maintenance boundaries, cache retention behavior, and the schema v1-to-v2 compatibility correction. Do not claim E2 cache cleanup without a controlled installation.

- [ ] **Step 2: Run formatting and focused diff checks**

Run:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
git diff --check
```

Expected: both exit 0.

- [ ] **Step 3: Run the complete Rust quality gates**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: all non-environment-gated tests pass; record exact passed/ignored totals from this run.

- [ ] **Step 4: Run frontend and application builds**

Run:

```bash
pnpm test
pnpm build
pnpm exec tauri build --debug --no-bundle
```

Expected: all exit 0. If `src-tauri/broker/Cargo.toml` exists in the final checkout, also run `cargo check --manifest-path src-tauri/broker/Cargo.toml`; otherwise record that the approved administrator-runtime redesign removed the Broker.

- [ ] **Step 5: Exercise a copy of the current development database**

Copy `%APPDATA%/com.yetanothermicrosoftstore.client/state.sqlite3` into an ignored test workspace, open the copy through schema v2 code, verify legacy events and settings remain readable, then run cleanup on the copy and verify `integrity_check`. Never mutate the user's live database during automated verification.

- [ ] **Step 6: Perform a whole-diff review**

Review schema constraints, transaction boundaries, stale lease handling, event/progress merge order, cache path safety, cleanup scope, UI timer lifecycle, and documentation evidence labels. Fix Critical/Important findings with a failing test first and rerun affected/full suites.

- [ ] **Step 7: Stage only the reviewed scope**

Use explicit `git add -- <paths>`, then run:

```bash
git diff --cached --check
git diff --cached --stat
git status --short
```

Exclude `target/`, `dist/`, `.superpowers/`, copied databases, runtime caches, credentials, and signed URLs.

- [ ] **Step 8: Hand off without committing**

Report exact verification evidence and remaining E2 limitation. Provide but do not execute:

```bash
git commit -S -m "fix: bound progress storage and clean completed state"
```
