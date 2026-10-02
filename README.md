# Yet Another Microsoft Store

Windows desktop client baseline for direct Microsoft Store package delivery. M0 native deployment acceptance, M1 offline Store protocol adapters, M2 domain persistence, and M3 applicability selection are complete.

## Current development state

- M0: CurrentUser and AllUsers deployment paths have passed the recorded Windows 10 19045 x64 acceptance loop, including one-shot UAC Broker, machine inventory postconditions, and exact certificate cleanup.
- M1: DCAT/FE3 adapters, project-owned DTOs, redacted fixtures, and contract tests are complete. This does not claim live Store/FE3 endpoint, authorization, download, or cross-channel update acceptance.
- M2: Project-owned domain/error/job DTOs, SQLite schema v1, repositories, transactional migrations, and restart recovery are complete. This is local persistence evidence only and does not claim live Store, download, or update acceptance.
- M3: SQLite schema v2, typed four-part versions, closed safe error details, FE3 applicability mapping, and explainable package/resource/dependency selection are complete at local E1 evidence. This does not claim live Store, download, package deployment, or cross-channel acceptance.
- M4: Controlled live protocol smoke, download/cache, and proxy behavior are the next gate.

## Development checks

```powershell
pnpm install
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo check --manifest-path src-tauri/broker/Cargo.toml
pnpm exec tauri build --debug --no-bundle
pnpm tauri dev
```

The deployment path keeps the Tauri main process at the Windows default `asInvoker` level. Current-user operations call `PackageManager` directly; all-users install, uninstall, and machine inventory use a one-shot `runas` Broker with a protected staging copy and bounded named-pipe protocol. It never launches PowerShell/winget. See [docs/support-matrix.md](docs/support-matrix.md) for the evidence boundary.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
