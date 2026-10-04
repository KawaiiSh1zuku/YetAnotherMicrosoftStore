# Details Actions and Deployment Blocking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Make product actions reflect local installation state and make package-in-use deployment failures actionable, diagnosable, and resumable without repeating catalog resolution or downloads.

**Architecture:** The backend owns local action selection, launch target lookup, package-in-use classification, and the durable deployment checkpoint. The job state machine gains an active waiting state; the worker retries from verified cache material through a dedicated control. React consumes closed DTOs, performs sequence-monotonic queue synchronization, and presents the waiting state as an automatic confirmation dialog.

**Tech Stack:** Rust 1.98.1, Tauri 2.12.1, Windows crate 0.62.2, SQLite/rusqlite 0.40.2, React 19.3, TypeScript, Vitest/Testing Library, Playwright.

**Spec:** `docs/superpowers/specs/2026-10-04-details-actions-and-deployment-blocking-design.md`

## Global Constraints

- Preserve all existing uncommitted work and make only file-scoped patches after reading the current file.
- Do not create a migration chain; update the single final `src-tauri/migrations/0001_initial.sql` because the application is unreleased.
- Do not automatically commit, stage, revert, or overwrite the user's existing changes.
- Frontend commands accept a trusted product ID or job ID only, never a PID, PFN, AUMID, executable path, cache path, or signed URL.
- Retain HTTPS/redirect allowlists, hash/size checks, manifest identity, publisher/PFN, native signature, cache containment, reparse-point, and case-alias protections.
- Windows deployment, rather than process discovery, is authoritative for package-in-use.
- E0/E1 evidence must not be labeled E2; controlled Windows launch/deployment observations remain explicit.

## Review Focus

- Installed package identity differs from catalog identity: fail details closed and never offer Update/Open for the wrong package.
- A stale initial `listJobs` response races with a newer event: the newer per-job sequence must win.
- A blocked job is restarted with missing or tampered verified cache material: fail closed and require explicit full retry.
- PID reuse occurs between enumeration and termination: PFN recheck must prevent terminating the replacement process.
- Windows returns package-in-use but enumeration finds no process: keep the job retryable and permit direct deployment retry without arbitrary termination.

---

### Task 1: Backend-Owned Details Action and Installed-App Launch

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/package_application.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/app_runtime.rs`
- Modify: `src-tauri/src/tauri_api.rs`
- Modify: `src-tauri/src/error.rs`
- Test: `src-tauri/tests/m6_api.rs`
- Test: `src-tauri/src/app_runtime.rs`
- Test: `src-tauri/src/package_application.rs`

**Interfaces:**
- Produces: `LocalProductActionKind::{Install, Update, Open}` and `LocalProductAction { kind, deployment_scope, installed_version, available_version, launchable }` in the Tauri details DTO.
- Produces: `PackageApplicationManager::is_launchable(package_family_name: &str) -> Result<bool, PackageApplicationError>`.
- Produces: `PackageApplicationManager::launch(package_family_name: &str) -> Result<(), PackageApplicationError>`.
- Produces: `ApiBackend::launch_installed_app(product_id: String) -> ApiFuture<'_, ()>` and Tauri command `launch_installed_app`.
- Consumes: existing all-user inventory, `derive_update_scope`, identity checks, `select_packages`, and persisted product association.

- [x] **Step 1: Write failing details-action contract tests**

Add literal JSON assertions in `m6_api.rs` for Install, Update, Open, version-ahead Open, unlaunchable Open, and identity-mismatch rejection. Name the break each test catches: returning Install for an installed package or trusting mismatched identity.

- [x] **Step 2: Run the focused Rust tests and verify RED**

Run: `cargo test --test m6_api details -- --nocapture`

Expected: FAIL because `localAction` and launch API do not exist.

- [x] **Step 3: Implement local-action derivation**

Extend `AppDetailsSource`/`ApiAppDetails`. In `ProductionApiBackend::get_app_details`, scan inventory once, validate matching main records, derive scope, select in Install or Update mode, and return the closed action DTO. Do not duplicate version logic in React.

- [x] **Step 4: Write failing package-application tests**

Cover invalid PFNs without touching Windows APIs and the platform-independent selection rule for zero, one, and multiple stable app entries.

- [x] **Step 5: Run package-application tests and verify RED**

Run: `cargo test package_application --lib -- --nocapture`

Expected: FAIL because the module and manager do not exist.

- [x] **Step 6: Implement trusted installed-app launch**

Enable only the needed Windows feature namespaces. Resolve the installed package from trusted PFN, read stable app entries, call the first entry's `LaunchAsync`, and map missing/unlaunchable/failed launch to closed error codes. The Tauri command receives only `productId` and re-derives the PFN from persisted trusted state.

- [x] **Step 7: Verify Task 1 GREEN**

Run: `cargo test --test m6_api --lib`

Expected: all selected tests pass with no new warnings.

### Task 2: Sequence-Monotonic Queue Synchronization and Verification Rendering

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/features/queue/QueueView.tsx`
- Modify: `src/lib/i18n.ts`
- Test: `src/test/tauri.test.ts`
- Test: `src/test/App.test.tsx`

**Interfaces:**
- Produces: `mergeJobSnapshots(current, incoming) -> Map<string, JobSnapshot>` with sequence-monotonic semantics.
- Produces: queue initialization that subscribes first, lists jobs, then replays events using one cursor.
- Consumes: existing `listJobs`, `listJobEvents`, `subscribeJobChanges`, and job sequence values.

- [x] **Step 1: Write failing queue-race tests**

Add tests where a sequence-8 event arrives before a sequence-7 list result and where an event is emitted during listener registration. Assert sequence 8 remains rendered.

- [x] **Step 2: Write the failing verification/preparation-stage UI tests**

Render a `verifying` job with `bytesDone == bytesTotal`. Assert `正在验证签名` is visible and the byte-progress bar/`100%` download label is absent. Render a `preparing` job and assert `正在准备安装` is visible without an installation percentage.

- [x] **Step 3: Run focused frontend tests and verify RED**

Run: `pnpm test -- --run src/test/tauri.test.ts src/test/App.test.tsx`

Expected: FAIL from stale snapshot replacement and missing verification presentation.

- [x] **Step 4: Implement monotonic synchronization and stage-specific rendering**

Centralize per-job sequence comparison, register the listener before accepting initial snapshots, replay after initial list, and serialize overlapping replay requests. Render download bytes only in Downloading, verification status only in Verifying, preparation status only in Preparing, and deployment percentage only in Deploying.

- [x] **Step 5: Verify Task 2 GREEN**

Run: `pnpm test -- --run src/test/tauri.test.ts src/test/App.test.tsx`

Expected: focused tests pass without React act warnings.

### Task 3: Process Descriptors and Native Package-In-Use Classification

**Files:**
- Modify: `src-tauri/src/package_process.rs`
- Modify: `src-tauri/src/deployment.rs`
- Modify: `src-tauri/src/deployment_coordinator.rs`
- Modify: `src-tauri/src/error.rs`
- Test: `src-tauri/src/package_process.rs`
- Test: `src-tauri/tests/m0_coordinator.rs`
- Test: `src-tauri/tests/m2_domain.rs`

**Interfaces:**
- Produces: `ProcessDescriptor { pid: u32, name: String }`.
- Produces: `TerminatePackageProcessesResult { matched, terminated, remaining }` using descriptor vectors.
- Produces: a pure `classify_deployment_hresult(code: i32) -> ErrorCode` mapping `0x80073D02` to `PackageInUse`.
- Consumes: ToolHelp `PROCESSENTRY32W.szExeFile`, `GetPackageFamilyName`, minimum process rights, and existing safe error conversion.

- [x] **Step 1: Write failing HRESULT and result-shape tests**

Use literal HRESULT values and literal descriptor JSON. Assert non-package-in-use deployment errors remain `DeploymentFailed`/`DeploymentDenied` according to existing rules.

- [x] **Step 2: Run focused Rust tests and verify RED**

Run: `cargo test --test m0_coordinator --test m2_domain package -- --nocapture`

Expected: FAIL because descriptor vectors and package-in-use mapping are absent.

- [x] **Step 3: Implement bounded descriptor enumeration and termination**

Decode executable name from `szExeFile`, cap collection size, preserve current-PID exclusion, and carry the original descriptor through open/recheck/terminate/wait. Put access denial, timeout, PFN mismatch, and failure in `remaining`.

- [x] **Step 4: Remove predictive deployment blocking**

Delete the `matching_process_count` guard from `SystemDeploymentPort::prepare`. Map the actual native deployment error to `PackageInUse`; enumerate descriptors only after that error for diagnostics.

- [x] **Step 5: Verify Task 3 GREEN**

Run: `cargo test --test m0_coordinator --test m2_domain --lib`

Expected: selected tests pass and no test expects proactive process blocking.

### Task 4: Durable Waiting State and Deployment Checkpoint Persistence

**Files:**
- Modify: `src-tauri/migrations/0001_initial.sql`
- Modify: `src-tauri/src/jobs.rs`
- Modify: `src-tauri/src/job_events.rs`
- Modify: `src-tauri/src/job_store.rs`
- Modify: `src-tauri/src/persistence.rs`
- Modify: `src-tauri/src/tauri_api.rs`
- Test: `src-tauri/tests/m2_domain.rs`
- Test: `src-tauri/tests/m2_persistence.rs`
- Test: `src-tauri/tests/m6_event_store.rs`
- Test: `src-tauri/tests/m6_api.rs`

**Interfaces:**
- Produces: `JobStage::AwaitingProcessExit` serialized as `awaiting_process_exit`.
- Produces: `JobControl::RetryDeployment` serialized as `retry_deployment`.
- Produces: `JobEvent::DeploymentBlocked { processes }` and a checkpoint-ready event/transaction.
- Produces: `DeploymentCheckpoint` containing ordered trusted package metadata and cache keys but no URL or frontend path.
- Produces: persistence methods `save_deployment_checkpoint_leased`, `deployment_checkpoint`, and terminal cleanup under lease validation.

- [x] **Step 1: Write failing state-transition and control tests**

Assert Preparing -> Deploying on the first native progress event, Preparing/Deploying -> AwaitingProcessExit, AwaitingProcessExit -> Preparing only through RetryDeployment, cancellation support, generic Resume rejection, active allowed controls, and restart preservation.

- [x] **Step 2: Run state/API tests and verify RED**

Run: `cargo test --test m2_domain --test m6_api awaiting -- --nocapture`

Expected: FAIL because the stage/control/event do not exist.

- [x] **Step 3: Write failing persistence tests**

Create a job, persist a literal checkpoint and blocked process list under a lease, reopen SQLite, and assert exact round-trip. Add rollback, stale lease, malformed JSON, missing cache-key, and terminal cleanup cases.

- [x] **Step 4: Run persistence/event tests and verify RED**

Run: `cargo test --test m2_persistence --test m6_event_store checkpoint -- --nocapture`

Expected: FAIL because schema and repository methods are absent.

- [x] **Step 5: Implement final schema, domain transitions, events, and repositories**

Extend the single initial migration. Keep event validation strict, cap descriptors/checkpoint entries, require sequence/lease ownership, and keep `awaiting_process_exit` unchanged during restart recovery.

- [x] **Step 6: Verify Task 4 GREEN**

Run: `cargo test --test m2_domain --test m2_persistence --test m6_event_store --test m6_api`

Expected: all four test binaries pass.

### Task 5: Worker Deployment Retry from Verified Checkpoint

**Files:**
- Modify: `src-tauri/src/deployment_plan.rs`
- Modify: `src-tauri/src/job_worker.rs`
- Modify: `src-tauri/src/app_runtime.rs`
- Modify: `src-tauri/src/tauri_api.rs`
- Test: `src-tauri/tests/m5_deployment_plan.rs`
- Test: `src-tauri/tests/m6_worker.rs`
- Test: `src-tauri/tests/m6_api.rs`

**Interfaces:**
- Produces: `DeploymentCheckpoint::from_plan(...)` and `DeploymentCheckpoint::rebuild_verified_plan(...)` with current cache metadata and all preflight validation.
- Produces: worker branch for `AwaitingProcessExit + RetryDeployment` that does not call resolver or downloader.
- Produces: termination backend restricted to the waiting stage and trusted job PFN.
- Consumes: Task 3 descriptors/result and Task 4 checkpoint/stage/control.

- [x] **Step 1: Write failing checkpoint reconstruction tests**

Cover exact reconstruction plus missing blob, wrong size/hash, path escape/reparse rejection, identity mismatch, and signature failure. Expected values must be literal and independent of the builder under test.

- [x] **Step 2: Write failing worker retry tests**

Seed a blocked job/checkpoint, enqueue RetryDeployment, and assert resolver calls = 0, downloader calls = 0, deployment calls = 1, and final stage/progress are correct. Add a restart-preserved blocked job and an invalid checkpoint failure.

- [x] **Step 3: Run worker/deployment-plan tests and verify RED**

Run: `cargo test --test m5_deployment_plan --test m6_worker retry_deployment -- --nocapture`

Expected: FAIL because checkpoint reconstruction and retry branch are absent.

- [x] **Step 4: Persist checkpoint before native deployment and handle package-in-use**

Build/persist the checkpoint after verified cache recording and before deployment. When native deployment returns PackageInUse, enumerate current descriptors and append DeploymentBlocked instead of Failed.

- [x] **Step 5: Implement direct retry**

Consume RetryDeployment only in AwaitingProcessExit, append Preparing, reconstruct and reverify the plan, and call the existing progress-aware deployment path. Append Deploying only on the first native progress event. Do not silently fall back to resolver/downloader when checkpoint validation fails.

- [x] **Step 6: Verify Task 5 GREEN**

Run: `cargo test --test m5_deployment_plan --test m6_worker --test m6_api`

Expected: all selected tests pass, including zero resolver/downloader calls on retry.

### Task 6: Details Actions and Active Process Dialog UI

**Files:**
- Modify: `src/lib/types.ts`
- Modify: `src/lib/tauri.ts`
- Modify: `src/lib/i18n.ts`
- Modify: `src/features/details/DetailsView.tsx`
- Modify: `src/features/queue/QueueView.tsx`
- Modify: `src/test/fixtures.ts`
- Test: `src/test/App.test.tsx`
- Modify if required: `src/App.css`

**Interfaces:**
- Consumes: `AppDetails.localAction`, `launchInstalledApp(productId)`, `JobStage.awaiting_process_exit`, `JobControl.retry_deployment`, and descriptor-array termination result.
- Produces: primary action dispatch for Install/Update/Open and an accessible controlled package-in-use dialog.

- [x] **Step 1: Write failing details UI tests**

Render literal Install, Update, Open, and unlaunchable Open details. Assert button label/icon, correct client method and scope, confirmation only for install/update, and no install fallback for installed packages.

- [x] **Step 2: Write failing package-in-use dialog tests**

Assert a blocked job appears in `进行中`, automatically opens the dialog once per sequence, cancel is non-destructive, successful termination sends RetryDeployment, and partial failure renders each literal `name (PID)` while keeping the dialog open.

- [x] **Step 3: Run focused frontend tests and verify RED**

Run: `pnpm test -- --run src/test/App.test.tsx src/test/tauri.test.ts`

Expected: FAIL because the DTOs/actions/waiting dialog are absent.

- [x] **Step 4: Implement details actions**

Use Lucide Download/RefreshCw/ExternalLink icons. Install and Update use the backend-provided scope; Open calls `launchInstalledApp`. Keep button width stable across loading labels and expose the unlaunchable reason without instructional feature text.

- [x] **Step 5: Implement the controlled active dialog**

Drive dialog state by blocked job ID + sequence, keep it reopenable from the row, list remaining descriptors in a bounded scroll area, and send RetryDeployment only after no survivors remain or the user explicitly retries after closing processes manually.

- [x] **Step 6: Verify Task 6 GREEN and accessibility**

Run: `pnpm test -- --run src/test/App.test.tsx src/test/tauri.test.ts`

Expected: tests pass with no accessibility violations or React warnings.

### Task 7: Diagnostics Documentation and Full Verification

**Files:**
- Modify: `docs/diagnostics.md`
- Modify: `findings.md`
- Modify: `progress.md`
- Modify: `task_plan.md`

**Interfaces:**
- Consumes: final implementation and fresh verification results.
- Produces: evidence-labeled diagnostics and current task records without claiming unrun E2 acceptance.

- [x] **Step 1: Update documentation**

Document local-action sources, actual Windows package-in-use mapping, active waiting behavior, checkpoint retry, residual process descriptors, and E1/E2 boundaries. Record any intentional deviation from the approved spec.

- [x] **Step 2: Run Rust formatting and targeted static checks**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --all-targets -- -D warnings`

Expected: exit 0 for both.

- [x] **Step 3: Run the full Rust E1 suite**

Run: `cargo test --lib --tests`

Expected: all non-environment-gated tests pass; report exact passed/ignored totals.

- [x] **Step 4: Run frontend tests and production build**

Run: `pnpm test -- --run`

Run: `pnpm build`

Expected: exit 0 with exact test totals recorded.

- [x] **Step 5: Run Playwright accessibility/responsive checks**

Run: `pnpm exec playwright test`

Expected: all configured desktop/narrow tests pass.

- [x] **Step 6: Run the Tauri debug build**

Run: `pnpm exec tauri build --debug --no-bundle`

Expected: exit 0. This is compilation/build evidence, not live launch/deployment acceptance.

- [x] **Step 7: Review all scoped diffs**

Run: `git diff --check`

Run: `git status --short`

Review each changed path against the requirement checklist, confirm unrelated user changes remain intact, and report any unrun controlled Windows E2 cases explicitly. Do not stage or commit.
