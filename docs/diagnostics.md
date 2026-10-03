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

## Partial scans

- Inventory results set `complete = false` when machine enumeration, user registration, or provisioned-package queries are incomplete.
- Update scans return scanned and associated counts, candidates, skipped PFNs with closed reason codes, and `complete`.
- Association lookup and FE3 resolution use the independent `maxConcurrentUpdateScans` setting (1-64, default 16); persistence remains serialized after each bounded network phase.
- A newer compatible main package that fails strict update selection is reported as `selection_rejected`; it is not silently treated as up to date.
- Partial results remain displayable and do not expose underlying raw errors.

## Network audit

Catalog/FE3 and package delivery hosts remain explicitly allowlisted. Application images use a separate CSP-only allowlist for `https://store-images.s-microsoft.com`; this does not widen `connect-src` or package download hosts.
