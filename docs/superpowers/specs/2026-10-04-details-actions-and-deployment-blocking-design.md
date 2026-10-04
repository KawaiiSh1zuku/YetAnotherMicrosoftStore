# Details Actions and Deployment Blocking Design

## Goal

Make product details and deployment recovery reflect the real local machine state:

- A product opened from search offers **Install**, **Update**, or **Open** from authoritative local inventory and version comparison.
- Signature verification is shown as its own task stage instead of appearing as a download stuck at 100%.
- A package-in-use condition remains actionable in the active queue and opens a confirmation dialog.
- Failed process termination reports every remaining process by executable name and PID.
- Retrying after a package-in-use failure resumes deployment from verified local artifacts instead of resolving and downloading again.
- A newly booted machine is not blocked merely because a same-package process is discoverable; Windows deployment is the authority on whether the package is actually in use.

## Non-Goals

- Do not use Microsoft Store web pages or URI schemes as a substitute for launching the installed app.
- Do not expose arbitrary process termination by PID or package family name to the frontend.
- Do not weaken hash, manifest identity, publisher, package family, signature, cache-containment, or reparse-point checks.
- Do not promise that every installed package has a launchable application entry.
- Do not claim E2 Windows deployment or launch acceptance from automated E1 tests.

## Current Failures

1. `get_app_details` always previews install mode with an empty installed set. `DetailsView` therefore always calls `startInstall` and always labels its primary action as Install.
2. Queue initialization calls `listJobs` concurrently with event subscription. A stale list response can overwrite a newer event snapshot, and an event emitted before subscription can be missed. This can leave a completed download displayed at 100% while the backend is verifying signatures.
3. The update preflight treats any process with the target PFN as proof that deployment is blocked. Windows has not rejected deployment at that point, so a legitimate background process can produce a permanent false positive.
4. `package_in_use` is persisted as terminal `Failed`, which places the action under the ended-task filter. Generic Resume always transitions to Resolving, repeating catalog resolution and the download stage.
5. Process enumeration is reduced to a count. Termination cannot explain which processes survived.

## Chosen Architecture

### 1. Backend-Owned Product Action

Extend the details response with a closed local-action contract:

```text
localAction:
  kind: install | update | open
  deploymentScope: current_user | all_users | null
  installedVersion: four-part version | null
  availableVersion: four-part version | null
  launchable: boolean
```

The production details path must:

1. Resolve catalog metadata and the FE3 package graph as it does today.
2. Scan elevated all-user inventory once.
3. Match only trusted main-package records by package identity, publisher, and PFN.
4. If no matching installation exists, run install-mode selection and return Install.
5. If a matching installation exists, derive its deployment scope and run update-mode selection with the installed record.
6. Return Update only when the selected catalog version is newer and applicable.
7. Otherwise inspect the installed package's application entries and return Open. If there is no launchable entry, return Open with `launchable=false`; the primary button is disabled with an accessible reason rather than falling back to Install.

The frontend consumes this result and does not independently infer versions or installation scope. Install and Update retain confirmation. Open invokes a dedicated backend command that re-resolves the trusted installed package from its PFN, selects its first stable application entry, and calls `AppListEntry.LaunchAsync`. The frontend never supplies an AUMID or executable path.

### 2. Race-Free Queue Synchronization

Queue startup must establish the event listener before accepting an initial list snapshot. Every merge is sequence-monotonic per job: an older snapshot may never overwrite a newer one. After the listener is active, perform an event replay to close the registration/list gap and advance a single cursor.

Only Downloading renders byte progress. Verifying renders the `verifying` stage label and a non-byte verification status; it does not keep a 100% download bar on screen. Deploying renders deployment percentage when available.

### 3. Windows-Authoritative Package-In-Use Detection

Remove `matching_process_count` from deployment preparation. The worker calls the native deployment API after all existing preflight validation. Only an actual Windows deployment result corresponding to package-in-use (including `0x80073D02`) becomes `package_in_use`.

When deployment reports package-in-use, enumerate processes with the same PFN and capture bounded, sanitized descriptors:

```text
ProcessDescriptor {
  pid: u32,
  name: string
}
```

Enumeration failures and inaccessible unrelated processes are not matches. A matched process is one for which `GetPackageFamilyName` succeeds and equals the trusted job PFN case-insensitively. The current process remains excluded.

### 4. Durable Deployment Block

Add an active `awaiting_process_exit` job stage. A package-in-use deployment result appends a deployment-blocked event containing bounded process descriptors and keeps the task in the active queue. The queue automatically opens one confirmation dialog for the newest blocked sequence. Dismissing the dialog leaves the job waiting and exposes a reopen action on its row.

Before the native deployment attempt, persist a deployment checkpoint owned by the job. It contains the minimum immutable information required to rebuild `VerifiedPackageSet` from verified cache entries, including package role/order, cache key, expected size/hash, format, identity, publisher, version, architecture, and resource ID. It contains no signed URL, credentials, or unverified path supplied by the frontend.

On retry, the worker reconstructs the deployment plan from this checkpoint and current verified cache metadata, re-runs cache containment, size/hash, manifest identity, publisher/PFN, and native signature validation, then calls deployment directly. The resolver and downloader are not invoked. Missing or invalid cache material fails closed with a re-resolve recommendation; it never deploys stale or unverified bytes.

Restart recovery preserves `awaiting_process_exit`. It does not automatically retry or transition to Resolving. The user may retry after closing processes or use the termination confirmation.

### 5. Scoped Process Termination

The termination command accepts only a job ID. The backend permits it only when the current snapshot is `awaiting_process_exit` with `package_in_use`, obtains the PFN from that trusted job, enumerates matches again, opens each process with the minimum required rights, rechecks PFN after opening the handle, terminates, and waits for exit.

The result is:

```text
TerminatePackageProcessesResult {
  matched: ProcessDescriptor[]
  terminated: ProcessDescriptor[]
  remaining: ProcessDescriptor[]
}
```

PID reuse, access denial, timeout, PFN mismatch after handle acquisition, or termination failure puts that process in `remaining`. When `remaining` is non-empty, the dialog stays open and lists `name (PID)` for each survivor. When it is empty, the frontend sends a dedicated RetryDeployment control; it does not send generic Resume.

## State Transitions

```text
verifying -> deploying
deploying -> completed
deploying -> awaiting_process_exit   (Windows reports package in use)
deploying -> failed                  (other deployment failure)

awaiting_process_exit -> deploying  (RetryDeployment from valid checkpoint)
awaiting_process_exit -> cancelled

restart(awaiting_process_exit) -> awaiting_process_exit
```

Generic Resume remains for Paused, Interrupted, and retryable Failed jobs. It must not be offered for `awaiting_process_exit`.

## Persistence

The application is not released and retains the repository's single final `0001_initial.sql` schema. Extend that schema rather than creating an upgrade chain.

Persist:

- the new job stage;
- the deployment-blocked event and its bounded process descriptors;
- the immutable deployment checkpoint;
- checkpoint replacement/removal rules tied to job event sequencing.

Checkpoint writes occur under the worker lease and in the same transactional boundary as the event that makes the checkpoint usable. Terminal completion/cancellation may remove the checkpoint according to existing cache-retention policy, but event history remains valid.

## Error and UI Behavior

- Install: package is not installed and an install selection is applicable.
- Update: package is installed and a strictly newer applicable version exists.
- Open: package is installed with no applicable newer version.
- Open unavailable: installed package has no launchable application entry; show a disabled Open button and concise status.
- Verifying: show `正在验证签名` without a download percentage.
- Package in use: keep the task under `进行中` and open the destructive confirmation dialog once per blocked event sequence.
- Termination partial failure: show every sanitized remaining `name (PID)` inside the dialog.
- Retry deployment: show deployment progress and do not show resolving or downloading unless the checkpoint fails validation and the user explicitly chooses a full retry.

## Security Boundaries

- Details state and launch targets come from Windows inventory, never catalog text alone.
- Launch and termination commands accept trusted identifiers only; no arbitrary executable path, AUMID, PID, or PFN crosses from the frontend.
- Process names are bounded to the ToolHelp executable-name field and serialized as display-only text.
- All checkpoint paths are derived through verified cache metadata and revalidated before deployment.
- Windows deployment remains the authority for package-in-use; enumeration supplies diagnostics and a user-authorized remediation action only.

## Compatibility and Failure Handling

- Multiple matching installed registrations use the existing stable inventory merge and `derive_update_scope` rules.
- Installed versions newer than the catalog return Open and never downgrade.
- Inventory or identity mismatch fails details closed instead of offering an incorrect action.
- Multiple app entries use a stable first entry. A later product feature may expose entry selection, but this change does not add it.
- A blocked job with no currently matching process may retry deployment without calling termination.
- A changed PID must be revalidated by PFN after its handle is opened.

## Verification

### E0 Static

- No frontend API accepts arbitrary PID, PFN, AUMID, or executable path.
- No proactive same-PFN count blocks deployment.
- `awaiting_process_exit` is active and restart-stable.

### E1 Automated

- Details API tests cover Install, Update, Open, version-ahead, identity mismatch, and unlaunchable packages.
- Frontend tests cover the three primary actions and disabled Open behavior.
- Queue tests reproduce the list/subscription race and prove sequence-monotonic convergence.
- Queue tests prove Verifying does not render the download progress bar.
- State/event/persistence tests cover blocked transitions, checkpoint atomicity, restart preservation, and invalid transitions.
- Worker tests prove RetryDeployment calls neither resolver nor downloader and revalidates the checkpoint.
- Process tests cover bounded names, current-PID exclusion, PFN recheck, partial termination, and remaining descriptor serialization.
- Full Rust tests, strict Clippy, formatting, Vitest, Playwright, frontend build, and Tauri debug build run before completion claims.

### E2 Controlled Windows

- Launch a known installed Store app from details.
- Reproduce `0x80073D02`, verify the active modal lists the observed name/PID, and verify cancel is non-destructive.
- Exercise partial termination if an access-denied process can be arranged safely.
- Close or terminate blockers and verify deployment resumes without catalog resolution or network download.
- Reboot with no true deployment conflict and confirm no proactive process count blocks deployment.

E2 remains open unless these observations are performed on the target Windows machine with a reversible test package.

## Documentation

Update `docs/diagnostics.md` with:

- details-action decision sources;
- package-in-use Windows error mapping;
- the waiting stage and deployment-checkpoint recovery behavior;
- residual process-name/PID reporting;
- E1 versus controlled E2 evidence boundaries.
