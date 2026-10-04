# Diagnostics and recovery

Diagnostics remain closed and local. The frontend cannot submit arbitrary log text, paths, URLs, service responses, SIDs, or HRESULTs.

## Export contents

`Settings > Export diagnostics` writes `yamstore-diagnostics-<timestamp>-<random-id>.json` to the current administrator account's Downloads directory. The Tauri response exposes only the file name and fixed destination label.

The export may contain:

- schema version, application version, and target architecture;
- unclean-session state;
- the audited production host allowlist;
- at most 200 closed diagnostic events with Unix timestamps.

Allowed runtime event names remain `runtime_started`, `previous_session_unclean`, `worker_failure`, `panic`, and `diagnostics_exported`. Inventory and update UI summaries expose only closed counters, completeness, and reason enums; raw Windows or Store errors are never copied into the public result.

The report never contains signed download URLs, proxy credentials, tokens, package paths, database paths, raw Store responses, raw panic strings, SIDs, or HRESULTs. The event log rotates at 64 KiB.

## Recovery boundary

The application owns a per-session Windows mutex and a clean-shutdown marker. The durable event store and generation-fenced worker remain authoritative for job recovery. Interrupted deployment requires inventory reconciliation; diagnostics do not retry deployment or infer success.

The main executable is already elevated before Tauri starts. There is no Broker lifecycle, IPC event, `AwaitingElevation` stage, or task-level UAC cancellation event to diagnose. UAC cancellation happens before the application process exists.

### Package-in-use recovery

Windows deployment is authoritative for package-in-use detection. HRESULT `0x80073D02` is mapped to the closed `package_in_use` error only after the native deployment call returns it; process enumeration is diagnostic and never predicts whether deployment is allowed.

Before native deployment, the worker persists a lease- and sequence-fenced checkpoint containing ordered cache keys, expected size/hash, package format, and package identity. It contains no signed URL or frontend-supplied path. A blocked job remains active in `awaiting_process_exit`, including after restart. `retry_deployment` rebuilds the exact plan from current verified cache entries and repeats cache containment, size/hash, manifest identity, publisher, and native signature checks. Missing or changed material fails closed; resolver and downloader are not called by this retry path.

The process termination command accepts only the trusted job ID. The backend derives the PFN from that job, re-enumerates matching processes, rechecks the PFN after opening each handle, and returns bounded `matched`, `terminated`, and `remaining` descriptors. The UI displays each remaining executable name and PID. A process name/PID is diagnostic only and is never accepted as a termination target from the frontend.

These contracts and recovery transitions have E0/E1 coverage. They do not prove a live Windows deployment, process termination, installed-app launch, or restart round trip; those remain controlled Windows E2 checks.

## Partial scans

- Inventory results set `complete = false` when machine enumeration, user registration, or provisioned-package queries are incomplete.
- Update scans return scanned and associated counts, candidates, skipped PFNs with closed reason codes, and `complete`.
- Association lookup and FE3 resolution use the independent `maxConcurrentUpdateScans` setting (1-64, default 16); persistence remains serialized after each bounded network phase.
- A newer compatible main package that fails strict update selection is reported as `selection_rejected`; it is not silently treated as up to date.
- Partial results remain displayable and do not expose underlying raw errors.

## Network audit

Catalog/FE3 and package delivery hosts remain explicitly allowlisted. Application images use a separate CSP-only allowlist for `https://store-images.s-microsoft.com`; this does not widen `connect-src` or package download hosts.
