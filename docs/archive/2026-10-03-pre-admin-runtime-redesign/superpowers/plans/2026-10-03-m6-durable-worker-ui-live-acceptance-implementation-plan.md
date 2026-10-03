# M6 Durable Worker, Frontend, and Live Acceptance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a replayable background job system, freeze the safe Tauri API, deliver the five M6 desktop workflows, and prove one reversible CurrentUser Store download/install round trip.

**Architecture:** SQLite append-only job events are the authority; the existing `jobs` table becomes a transactionally updated projection. A durable command inbox and expiring worker lease make pause/resume/cancel and crash recovery independent of WebView lifetime. React consumes safe snapshots and cursor-based events through one typed adapter, while the worker alone runs resolve/select/download/cache/deploy.

**Tech Stack:** Rust 2021, Tauri 2.12.1, Tokio, reqwest, SQLite/rusqlite, React 19, TypeScript 6, Tailwind CSS, local shadcn/ui components, Radix UI, Lucide, Vitest/Testing Library, Playwright.

**Spec:** `docs/superpowers/specs/2026-10-01-third-party-store-client-design.md`

## Global Constraints

- Keep `storelib_rs` inside `catalog.rs` and `resolver.rs`; expose only project DTOs.
- Never expose package URLs, FE3 response bodies, credentials, raw HRESULT values, cache paths, or user-local paths to React or job events.
- Accept only an explicitly labelled FE3 SHA-256 digest decoded to 64 lowercase hexadecimal characters; never reinterpret SHA-1 or base64 as SHA-256.
- Do not edit historical migrations; schema v4 is a new immutable migration.
- Append event + update projection in one SQLite transaction with expected-sequence concurrency control.
- Only the valid lease owner may execute jobs. An interrupted deployment must reconcile inventory before any retry.
- Pause/cancel are durable and idempotent, but are rejected once deployment begins.
- The Tauri process remains `asInvoker`; CurrentUser work is direct and AllUsers work uses the existing one-shot broker.
- Product code does not invoke PowerShell, WinGet, Microsoft Store UI, or Windows Update.
- Live acceptance uses a currently free ordinary MSIX/AppX product, complete pre/post scans, system-trusted package signatures, and exact rollback of only newly introduced registrations. It never imports or deletes certificates and never reuses the M0 self-signed acceptance script.
- Do not commit. At completion, stage only reviewed milestone files and provide the commit command.

## Review Focus

- Primary SHA-1 plus additional base64 SHA-256: select only one valid SHA-256 and reject conflicts or malformed lengths.
- Crash after `Deploying` event but before terminal event: reconcile inventory and never blindly repeat deployment.
- Duplicate command IDs, expired leases, and two processes racing: exactly one state transition and no duplicate deployment.
- Projection/event divergence or sequence gaps: fail closed and surface a stable storage error.
- Late/duplicate window hints and reconnect: cursor replay cannot regress the latest snapshot.
- 360 px window, keyboard-only navigation, focus restoration, live progress, and safe localized errors remain usable.
- Live target already exists, has shared-state risk, or cleanup is incomplete: stop without deleting pre-existing packages.

---

### Task 1: Normalize the FE3 SHA-256 Contract

**Files:** `src-tauri/src/resolver.rs`, `src-tauri/src/deployment_plan.rs`, `src-tauri/tests/fixtures/fe3-applicability.xml`, `src-tauri/tests/m1_protocol.rs`, `src-tauri/tests/m3_applicability.rs`, `src-tauri/tests/m5_deployment_plan.rs`, `src-tauri/tests/m5_identity.rs`, `src-tauri/Cargo.toml`

**Produces:** `ResolvedPackage::sha256: Option<String>`, always lowercase hex when present.

- [x] Add fixture tests for additional SHA-256 selection, malformed base64, wrong length, SHA-1-only metadata, and conflicting SHA-256 entries.
- [x] Run the focused protocol test and observe RED because normalized SHA-256 is absent.
- [x] Implement strict extraction inside `resolver.rs` and change deployment-plan matching to consume `sha256`.
- [x] Run M1/M3/M5 focused tests GREEN, then all Rust targets.

### Task 2: Add Schema v4 Event Store and Durable Commands

**Files:** create `src-tauri/migrations/0004_m6_job_events.sql`, `src-tauri/src/job_events.rs`, `src-tauri/src/job_store.rs`, `src-tauri/tests/m6_event_store.rs`; modify `src-tauri/src/persistence.rs`, `src-tauri/src/jobs.rs`, `src-tauri/src/lib.rs`, M2/M3 persistence tests.

**Produces:** closed `JobEvent`, `JobEventKind`, `JobControl`, `StoredJobEvent`, `JobCommand`, and transactionally projected `JobSnapshot` APIs.

- [x] Add RED tests for v3-to-v4 migration, append/projection atomicity, expected-sequence conflicts, command-ID dedupe, cursor ordering, projection rebuild, gaps, and redaction.
- [x] Add immutable schema v4 tables/indexes for events, commands, and worker leases.
- [x] Implement event folding, transactional append/project, durable inbox, and projection validation.
- [x] Run M6 event-store tests GREEN and all persistence regressions.

### Task 3: Implement the Leased Background Worker

**Files:** create `src-tauri/src/job_worker.rs`, `src-tauri/tests/m6_worker.rs`; modify `src-tauri/src/download.rs`, `src-tauri/src/cache.rs`, `src-tauri/src/deployment_orchestrator.rs`, `src-tauri/src/error.rs` only where the tested worker contract requires it.

**Consumes:** Task 2 event store/commands; M1-M5 catalog, resolver, selector, downloader, cache, verification, and deployment interfaces.

**Produces:** `JobWorker` with acquire/renew/release lease, deterministic `run_once`, recovery reconciliation, and progress/event callbacks.

- [x] Add RED tests for lease races/expiry, duplicate starts, pause/resume/cancel boundaries, restart during download, crash boundary at deployment, reconciliation, terminal idempotency, and exact event order.
- [x] Implement the worker in dependency-injected layers so tests use real event storage and controlled external ports.
- [x] Wire production ports without persisting URLs or paths in events; downloads may persist only existing safe cache metadata.
- [x] Run worker tests GREEN, then full Rust tests and strict Clippy.

### Task 4: Freeze the Safe Tauri API

**Files:** create `src-tauri/src/tauri_api.rs`, `src-tauri/tests/m6_api.rs`; modify `src-tauri/src/lib.rs`, `src-tauri/src/persistence.rs`, `src-tauri/src/error.rs`.

**Produces:** `search_apps`, `get_app_details`, `scan_installed_packages`, `scan_updates`, `start_install`, `start_update`, `request_job_control`, `get_job`, `list_jobs`, `list_job_events`, `get_settings`, `update_settings`, and `clear_cache`; emits only `job://changed` hints.

- [x] Add RED tests for stable serialization, closed errors, safe detail views, cursor paging, old-hint rejection data, and absence of URLs/paths/HRESULT text.
- [x] Replace `greet` and Spike handlers with the closed command set, initialize storage/worker in Tauri setup, and broadcast only sequence hints.
- [x] Run M6 API tests GREEN, all Rust tests, and strict Clippy.

### Task 5: Build the M6 Desktop Workbench

**Files:** create `components.json`, `src/lib/{tauri,types,i18n}.ts`, `src/components/ui/*`, `src/components/AppShell.tsx`, `src/features/{search,details,queue,installed,settings}/*`, `src/test/*`, `tests/ui/m6.spec.ts`; modify `src/{App.tsx,App.css,main.tsx}`, `package.json`, `pnpm-lock.yaml`, `vite.config.ts`.

**Consumes:** Task 4 typed commands, snapshots, cursor events, and changed hints.

- [x] Add RED Vitest tests for search-to-details focus restoration, cursor replay/old hint handling, job controls, settings validation, safe localization, loading/empty/error states, and narrow navigation.
- [x] Add locked UI/test dependencies and implement a compact workbench with five views, local shadcn-style components, Lucide icons, visible focus, live regions, and reduced motion.
- [x] Run Vitest GREEN, Playwright keyboard/focus/360 px/axe smoke, and `pnpm build`.
- [x] Run a Windows Tauri command smoke; browser adapter doubles do not complete M6 integration evidence.

### Task 6: Reversible Live Store Download and CurrentUser Install

**Files:** create `src-tauri/tests/m6_live_store_acceptance.rs`; modify `docs/support-matrix.md`, the total plan/spec, `task_plan.md`, `findings.md`, and `progress.md`.

**Consumes:** the production resolver, worker, cache, signature preflight, and CurrentUser orchestrator.

- [x] Extend the opt-in metadata smoke to require at least one exact-allowlisted Microsoft delivery URL over HTTP or HTTPS, normalized SHA-256, and an expected byte size without downloading.
- [x] Select a currently free, ordinary, non-installed, reversible product using current Store/catalog evidence and record the exact product/market/language/time/host boundary.
- [x] Capture a complete pre-scan and abort on existing target, ambiguity, missing digest, unsupported format, shared-state risk, or incomplete inventory.
- [x] Download through the production allowlist/proxy/cache path; verify size, SHA-256, manifest identity, and WinTrust before install.
- [x] Execute through the durable worker and production CurrentUser orchestrator, then require an exact inventory postcondition.
- [x] Remove only full names absent from the pre-scan, prove baseline restoration, and treat cleanup failure as an incomplete acceptance.

### Task 7: Final Review, Verification, and Staging

**Files:** review all Task 1-6 paths and generated-output exclusions.

- [x] Run Rust formatting, all-target tests, strict Clippy, broker check, frontend tests, Playwright, frontend build, and Tauri debug no-bundle build.
- [x] Review the whole branch for duplicate deployment, event/projection divergence, DTO leaks, unsafe URLs, cancellation races, accessibility, and document drift.
- [x] Fix every Critical/Important finding with a failing regression test first and rerun full gates.
- [x] Stage only reviewed source, tests, lockfiles, migration, and docs; exclude `dist/`, `target/`, downloaded packages, databases, evidence payloads, and broker binaries.
- [x] Run `git diff --cached --check`, inspect the full staged diff, and provide the Conventional Commit command without executing it.
