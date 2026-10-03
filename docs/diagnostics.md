# Diagnostics and recovery

M8 diagnostics are deliberately closed and local. The frontend cannot supply arbitrary log text, paths, URLs, or service responses.

## Export contents

`Settings > Export diagnostics` writes `yamstore-diagnostics-<timestamp>-<random-id>.json` to the current user's Downloads directory. The random component prevents rapid consecutive exports from overwriting each other. The Tauri response exposes only the file name and the fixed destination label, not the absolute path.

When **Save redacted diagnostics** is off (the default), the application removes the event log and session marker and does not persist runtime, worker, or panic events. A user-requested export still contains the static version, architecture, and host-audit fields, but its event list is empty. Enabling the setting starts a new diagnostic session; disabling it clears the persisted diagnostic files.

The report contains:

- schema version, application version, and target architecture;
- whether the previous session ended without the clean shutdown marker;
- the audited production host allowlist;
- at most 200 fixed diagnostic events with Unix timestamps.

Allowed event names are `runtime_started`, `previous_session_unclean`, `worker_failure`, `panic`, and `diagnostics_exported`. Unknown or malformed stored event lines are not copied into exports. The event log rotates at 64 KiB and exports retain at most 200 valid events.

The report never contains signed download URLs, proxy credentials, tokens, package paths, database paths, raw Store responses, or raw panic/error strings. Automated tests reject URL schemes and Windows drive paths in the exported JSON.

## Crash recovery boundary

The application holds a per-session Windows mutex so only one process can own the fixed session marker. The marker detects an unclean prior exit and records a fixed recovery event on the next launch. The existing M6 durable event store and generation-fenced worker remain authoritative for job recovery; the marker does not retry deployments or infer package state. Interrupted deployment still requires inventory reconciliation.

The panic hook records only the closed `panic` event and then invokes the previous hook so normal crash reporting behavior is preserved. A clean application exit removes the session marker.

## Network audit

The report lists the two production catalog/FE3 hosts and the two package delivery hosts. A Rust regression test keeps this list aligned with the production download allowlist. Redirects remain subject to the M4 per-hop host, scheme, credential, fragment, and port checks.
