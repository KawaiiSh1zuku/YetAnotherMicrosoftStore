# Support matrix

Evidence levels used here:

- **E0**: static source, schema, or artifact inspection.
- **E1**: local automated tests and builds.
- **E2**: controlled Windows deployment with reversible packages.
- **E3**: live Microsoft Store/CDN or cross-channel behavior.

## Current architecture

| Area | Current behavior | Evidence |
|---|---|---|
| Process privilege | Main executable embeds `requireAdministrator`; UAC cancellation prevents startup | E0 manifest extraction and release regression |
| Deployment scopes | CurrentUser means the running administrator account; AllUsers uses direct stage/provision/remove | E1 coordinator/orchestrator tests |
| Privilege helper | No Broker crate, IPC protocol, sidecar, or task-level elevation stage | E0 source and bundle audit |
| Database | Pre-release schema is created by the single `0001_initial.sql`, schema version 1 | E1 persistence tests |
| Search metadata | Bounded detail hydration exposes app name, package name, PFN, publisher, formats, and allowlisted icon | E1 protocol/API/UI tests |
| Package selection | Host capability is a hard gate; architecture settings rank compatible candidates; ordered language preferences fall back to `en-US` and then any available language resource | E1 applicability/worker tests |
| Installed inventory | Machine scan merges current, other-user, and provisioned state; partial failures stay explicit | E1 inventory tests; E2 not rerun |
| Update discovery | Two bounded network phases use an independent 1-64 concurrency setting (default 16); returns counts, candidates, closed skipped reasons, completeness, and scope derived from installation state | E1 API/runtime tests; targeted live PFN/FE3 probes |

## Package formats

| Format | Current code path | Acceptance boundary |
|---|---|---|
| `.msix` / `.appx` | Resolve, select, download, validate, and deploy | E1 after refactor; prior Broker E2 is historical only |
| `.msixbundle` / `.appxbundle` | Same path with bundle-aware selection | E1 after refactor; E2 pending |
| `.eappx` / `.eappxbundle` | Parser/selector representation only | Deployment acceptance pending |
| `.msixvc` / Xbox | Rejected by capability gate | E1 rejection tests |
| `.exe` / `.msi` | Unsupported | Not downloaded or executed |

## Platform matrix

| Platform | Build status | Runtime status |
|---|---|---|
| Windows 10 x64 build 19045 | Local x64 debug build is the current E1 baseline | Controlled CurrentUser/AllUsers refactor acceptance pending |
| Windows x64 release/NSIS | Script and checks implemented | Clean-machine run pending |
| Windows ARM64 | Workflow and target configured | Native artifact and runtime acceptance pending |

## Security boundaries

- Production package downloads remain limited to the audited Microsoft delivery hosts and are checked per redirect.
- Catalog icons accept only HTTPS URLs on `store-images.s-microsoft.com`; credentials, fragments, custom ports, HTTP, and other hosts are rejected.
- Signed URLs, tokens, proxy credentials, raw service responses, HRESULTs, SIDs, and local paths are not exposed through public DTOs or diagnostics.
- Removing the Broker removes obsolete IPC checks; it does not remove hash, manifest identity, WinTrust, cache containment, staging, or inventory postcondition checks.
- The elevated WebView increases impact of frontend compromise, so CSP and the Tauri command boundary remain release gates.

## Outstanding acceptance

- E2: UAC accept/cancel lifecycle and absence of a residual process.
- E2: reversible CurrentUser and AllUsers install/update/remove convergence on the refactored runtime.
- E2: machine inventory matches Windows facts for other-user and provisioned packages.
- E3: live Store search hydration, PFN association, package resolution, and CDN delivery.
- E1/E2: native ARM64 build and runtime checks.

Historical Broker evidence is preserved under `docs/archive/2026-10-03-pre-admin-runtime-redesign/` and must not be cited as acceptance for this architecture.
