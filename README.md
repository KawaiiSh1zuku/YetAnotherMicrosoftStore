# Yet Another Microsoft Store

Windows desktop client for direct Microsoft Store package delivery. M0 native deployment through M6 durable jobs, safe Tauri API, desktop workflows, and one reversible real Store package round trip are complete at their documented evidence levels.

## Current development state

- M0: CurrentUser and AllUsers deployment paths have passed the recorded Windows 10 19045 x64 acceptance loop, including one-shot UAC Broker, machine inventory postconditions, and exact certificate cleanup.
- M1: DCAT/FE3 adapters, project-owned DTOs, redacted fixtures, and contract tests are complete. This does not claim live Store/FE3 endpoint, authorization, download, or cross-channel update acceptance.
- M2: Project-owned domain/error/job DTOs, SQLite schema v1, repositories, transactional migrations, and restart recovery are complete. This is local persistence evidence only and does not claim live Store, download, or update acceptance.
- M3: SQLite schema v2, typed four-part versions, closed safe error details, FE3 applicability mapping, and explainable package/resource/dependency selection are complete at local E1 evidence. This does not claim live Store, download, package deployment, or cross-channel acceptance.
- M4: A controlled live DCAT/FE3 smoke and local HTTP download/cache/proxy tests are complete. Static current-user Windows proxy settings, custom HTTP(S)/SOCKS5, resumable verified downloads, URL refresh, cancellation, and cache recovery are implemented. PAC/WPAD remains outside this evidence; M6 separately exercised one real Microsoft CDN payload.
- M5: SQLite schema v3 identity correlation, verified package-graph planning, WinTrust signature preflight, strict install/update decisions, deployment convergence, and stable error mapping are complete at local E1 evidence. Production deployment delegates to the M0 coordinator/Broker.
- M6: SQLite schema v4 events/projections, durable commands, generation-fenced worker leases, 13 safe Tauri commands, and the search/details/queue/installed/settings workbench are complete. One Microsoft-signed Sysinternals Suite `.msixbundle` passed real CurrentUser download, SHA-256/identity/WinTrust validation, install, inventory convergence, uninstall, and exact baseline restoration on Windows 10 19045 x64; this does not claim cross-channel Store E3, AllUsers, or general product compatibility.
- M7: Official Store cross-channel correlation/update interoperability is the next development gate.

## Development checks

```powershell
pnpm install
pnpm test
pnpm exec playwright test
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
