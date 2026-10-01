# Yet Another Microsoft Store

Windows desktop client baseline for direct Microsoft Store package delivery. The repository is currently at M0: the Tauri/Rust/React shell and the native deployment capability probe are in place.

## M0 development

```powershell
pnpm install
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml
pnpm tauri dev
```

The deployment probe only activates `Windows.Management.Deployment.PackageManager`. It does not install a package, launch PowerShell/winget, or claim that the all-users broker is ready. See [docs/support-matrix.md](docs/support-matrix.md) for the evidence boundary.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
