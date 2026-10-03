# Yet Another Microsoft Store

Yet Another Microsoft Store is a Windows desktop client that searches Microsoft Store catalog metadata, resolves compatible AppX/MSIX packages, downloads them from Microsoft delivery hosts, verifies package identity and signatures, and deploys them through Windows package APIs.

The application is built with Rust, Tauri 2, React, and TypeScript. It does not use `winget` or PowerShell as a product runtime dependency.

## Release status

Version `0.1.0` is a pre-release build. M0-M6 are implemented at the evidence levels recorded in [the support matrix](docs/support-matrix.md). M7 cross-channel Store interoperability is deferred because the current validation machine's Microsoft Store is unavailable. M8 release hardening is implemented at local E1: the x64 unsigned NSIS path has been compiled and checked, while the ARM64 artifact path is configured for GitHub's native ARM64 runner but has not yet been executed in this checkout.

Release artifacts are intentionally unsigned because the project does not use a code-signing certificate. Publish both architecture-specific SHA-256 files with every release and complete the clean-machine checklist in [the release guide](docs/release.md). Windows may show an unknown-publisher or SmartScreen warning.

## Supported environment

- Windows 10 x64 build 19045 is the currently recorded validation baseline.
- Release artifacts target Windows x64 and Windows ARM64.
- Supported payload families are `.msix`, `.appx`, `.msixbundle`, and `.appxbundle` within the documented package, identity, architecture, and authorization boundaries.
- `.eappx` and `.eappxbundle` remain conditional; MSIXVC/Xbox, EXE, and MSI payloads are not supported.
- Current-user deployment is the default. All-users operations use a one-shot UAC Broker; the main application remains `asInvoker`.
- The NSIS installer is configured for per-user installation, downgrade blocking, and Microsoft's WebView2 download bootstrapper. Those behaviors still require clean-machine acceptance; initial installation may require network access.

The recorded Sysinternals Suite round trip proves one product, market, language, host, time, and CurrentUser scenario. It is not a general compatibility or official Store update guarantee.

## Install

1. Download the installer matching the machine architecture from the release.
2. Verify the SHA-256 value against the adjacent `SHA256SUMS.txt`.
3. Run the NSIS installer. It is configured for the current Windows user. Application-data preservation during an in-place upgrade remains part of the clean-machine release checklist.

The installer, main executable, and Broker are unsigned. The elevated Broker still validates the named-pipe peer PID/session/nonce, caller image path, protected staging path, package identity, hash, and Microsoft package signature; it does not require Authenticode on the caller executable.

## Privacy and network boundary

The production network audit permits these Microsoft hosts:

- `displaycatalog.mp.microsoft.com`
- `fe3.delivery.mp.microsoft.com`
- `dl.delivery.mp.microsoft.com`
- `tlu.dl.delivery.mp.microsoft.com`

Signed download URLs and proxy credentials are not persisted. Diagnostic exports contain only closed event names, timestamps, version/architecture data, crash-recovery state, and the host allowlist. They omit URLs, tokens, proxy credentials, raw service responses, HRESULT values, and local paths. Use **Settings > Export diagnostics** to create a JSON report in the current user's Downloads directory.

## Build from source

Prerequisites:

- Windows with the MSVC build tools for the target architecture
- the stable Rust toolchain selected by `rust-toolchain.toml` (CI currently pins `1.98.1`)
- Node.js 24 and pnpm `8.15.1`
- WebView2 development/runtime prerequisites required by Tauri

```powershell
pnpm install --frozen-lockfile
pnpm test
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
pnpm build:release
```

Build one architecture without rerunning the full quality gate:

```powershell
./scripts/build-release.ps1 -Architecture x64 -SkipChecks
./scripts/build-release.ps1 -Architecture arm64 -SkipChecks
```

The script verifies the main executable and Broker PE machine type and writes the installer, `SHA256SUMS.txt`, `BUILD-METADATA.json`, and `THIRD_PARTY_LICENSES.json` under `release-artifacts/<architecture>/`.

See [the release guide](docs/release.md) for dual-architecture CI, hash publication, and upgrade/uninstall acceptance. See [diagnostics](docs/diagnostics.md) for the export contract.

## Development checks

```powershell
pnpm test
pnpm exec playwright test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/broker/Cargo.toml
pnpm exec tauri build --debug --no-bundle
```

## License status

The repository does not currently declare a project distribution license. Do not mirror or redistribute source or binaries without maintainer permission. Third-party dependency license metadata is generated from the locked Cargo and pnpm graphs and bundled as `THIRD_PARTY_LICENSES.json`.
