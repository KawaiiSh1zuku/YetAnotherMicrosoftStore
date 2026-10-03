# Release guide

This project publishes intentionally unsigned NSIS artifacts. No code-signing certificate, PFX secret, SignTool step, or tag-only signing gate is required. Windows can show an unknown-publisher or SmartScreen warning; users must verify release hashes before running an installer.

Unsigned release builds use one elevated application process for both CurrentUser and AllUsers deployment. The executable embeds `requireAdministrator`; there is no Broker, named-pipe protocol, or sidecar artifact. Package identity, hash, signature, staging containment, and deployment postcondition checks remain mandatory.

## Artifact matrix

| Architecture | Rust target | GitHub runner | Output |
|---|---|---|---|
| x64 | `x86_64-pc-windows-msvc` | `windows-2025` | unsigned NSIS setup EXE, SHA-256, metadata, dependency licenses |
| ARM64 | `aarch64-pc-windows-msvc` | `windows-11-arm` | unsigned NSIS setup EXE, SHA-256, metadata, dependency licenses |

The ARM64 application is a native ARM64 PE image. Tauri's NSIS installer executable may run through Windows x86 emulation; this does not make the installed application x64.

## Local build

Install the matching Rust target and MSVC tools, then run:

```powershell
./scripts/build-release.ps1 -Architecture x64
./scripts/build-release.ps1 -Architecture arm64
```

`-Architecture all` builds both. The script removes any inherited `TAURI_CONFIG` while bundling so a local signing override cannot change the documented unsigned artifact contract. It fails if the requested Rust target is absent, if the application PE machine type or elevation manifest is wrong, if a legacy Broker file is present, or if NSIS does not emit exactly one fresh installer.

The application crate enables Tauri's `custom-protocol` feature by default, so direct Cargo builds embed `frontendDist` and never require a localhost server. `tauri dev` explicitly disables default features and starts Vite at `http://localhost:1420`. Build the frontend before direct Cargo checks on a clean checkout; the release script enforces this order.

Each clean `release-artifacts/<architecture>/` directory contains:

- the unsigned NSIS setup executable;
- `SHA256SUMS.txt`;
- `BUILD-METADATA.json` with `signed: false`, source commit, and dirty-worktree state;
- `THIRD_PARTY_LICENSES.json`.

## GitHub Actions

`.github/workflows/release-build.yml` runs quality checks once and uses native Windows x64/ARM64 runners for the bundle matrix. Pull requests, main-branch pushes, manual runs, and `v*` tags all use the same unsigned build path. No certificate secrets are read or required.

The checked-in workflow is configuration evidence until it runs on GitHub. Local x64 compilation does not prove that the ARM64 runner or uploaded ARM64 artifact succeeded.

GitHub's hosted-runner reference lists `windows-11-arm` as the standard Windows ARM64 runner, and Tauri documents `aarch64-pc-windows-msvc` for native ARM64 Windows builds:

- <https://docs.github.com/en/actions/reference/runners/github-hosted-runners>
- <https://v2.tauri.app/distribute/windows-installer/>

## Publication checklist

1. Run the workflow from the exact release commit or tag.
2. Download both architecture artifacts and verify that each directory contains exactly one setup EXE plus the three metadata files.
3. Recompute SHA-256 independently and compare it with both `SHA256SUMS.txt` and `BUILD-METADATA.json`.
4. Confirm `architecture`, Rust target, source commit, `dirty: false`, and `signed: false` in metadata.
5. Publish the hashes alongside the installers and state clearly that the executables are unsigned.

## Clean-machine acceptance

Perform this matrix on throwaway Windows VMs or snapshots. Never infer it from the build job.

1. Verify the installer hash before acknowledging any unknown-publisher warning.
2. Install with no prior app state; confirm the current-user install directory and Start menu entry.
3. Validate WebView2 bootstrapper behavior with WebView2 present and absent.
4. Launch, search, open settings, export diagnostics, and run the accessibility smoke.
5. Confirm launch requests UAC, cancellation leaves no application process, and an accepted launch runs elevated.
6. Exercise a safe CurrentUser workflow and confirm it targets the running administrator account without PowerShell or `winget`.
7. Attempt a downgrade and confirm the installer blocks it.
8. Uninstall and verify binaries, shortcuts, and uninstall registration are removed. Record whether user data is retained by policy.
9. Exercise AllUsers only with a reversible test package: confirm machine inventory convergence, deprovision/removal, and restored baseline.
10. Repeat on x64 and ARM64.

M8 may be called release-accepted only after the workflow and this matrix pass. M7 cross-channel Store interoperability remains a separate gate.
