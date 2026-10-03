# Release guide

This project publishes intentionally unsigned NSIS artifacts. No code-signing certificate, PFX secret, SignTool step, or tag-only signing gate is required. Windows can show an unknown-publisher or SmartScreen warning; users must verify release hashes before running an installer.

Unsigned release builds support both CurrentUser and the existing AllUsers Broker path. The Broker does not require caller Authenticode; it retains the named-pipe PID/session/nonce checks, caller image-path check, protected staging boundary, and package identity/hash/Microsoft-signature validation.

## Artifact matrix

| Architecture | Rust target | GitHub runner | Output |
|---|---|---|---|
| x64 | `x86_64-pc-windows-msvc` | `windows-2025` | unsigned NSIS setup EXE, SHA-256, metadata, dependency licenses |
| ARM64 | `aarch64-pc-windows-msvc` | `windows-11-arm` | unsigned NSIS setup EXE, SHA-256, metadata, dependency licenses |

The ARM64 application and Broker are native ARM64 PE images. Tauri's NSIS installer executable may run through Windows x86 emulation; this does not make the installed application x64.

## Local build

Install the matching Rust target and MSVC tools, then run:

```powershell
./scripts/build-release.ps1 -Architecture x64
./scripts/build-release.ps1 -Architecture arm64
```

`-Architecture all` builds both. The script removes any inherited `TAURI_CONFIG` while bundling so a local signing override cannot change the documented unsigned artifact contract. It fails if the requested Rust target is absent, if the Broker or application PE machine type is wrong, or if NSIS does not emit exactly one fresh installer.

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
5. Exercise a safe CurrentUser workflow and confirm no PowerShell or `winget` child process is created.
6. Upgrade from the previous release; verify settings, job event history, and cache policy are preserved.
7. Attempt a downgrade and confirm the installer blocks it.
8. Uninstall and verify binaries, shortcuts, and uninstall registration are removed. Record whether user data is retained by policy.
9. Exercise AllUsers only with a reversible test package: confirm the expected UAC prompt, machine inventory convergence, deprovision/removal, and restored baseline.
10. Repeat on x64 and ARM64.

M8 may be called release-accepted only after the workflow and this matrix pass. M7 cross-channel Store interoperability remains a separate gate.
