# Yet Another Microsoft Store

Yet Another Microsoft Store 是一个 Windows 桌面客户端，用于搜索 Microsoft Store 目录元数据、解析兼容的 AppX/MSIX 软件包、从 Microsoft 分发主机下载软件包、验证软件包身份与签名，并通过 Windows 软件包 API 完成部署。

本项目使用 Rust、Tauri 2、React 和 TypeScript 构建，不依赖 `winget` 或 PowerShell 作为产品运行时组件。

## 发布状态

当前版本 `0.1.0` 为预发布版本。M0-M6 已按[支持矩阵](docs/support-matrix.md)中记录的证据等级完成。由于当前验证机器上的 Microsoft Store 不可用，M7 跨渠道 Store 互操作验证已暂缓。M8 发布加固已达到本地 E1：x64 无签名 NSIS 构建链路已完成编译与检查；ARM64 工件链路已配置为使用 GitHub 原生 ARM64 runner，但尚未在当前检出或 GitHub Actions 中实际运行。

发布工件有意保持无代码签名，因为本项目不使用代码签名证书。每次发布都应同时公布两个架构各自的 SHA-256 文件，并完成[发布指南](docs/release.md)中的干净机器检查清单。Windows 可能显示“未知发布者”或 SmartScreen 警告。

## 支持环境

- 当前已记录的验证基线为 Windows 10 x64 build 19045。
- 发布工件面向 Windows x64 和 Windows ARM64。
- 在已记录的软件包、身份、架构和授权边界内，支持 `.msix`、`.appx`、`.msixbundle` 和 `.appxbundle`。
- `.eappx` 和 `.eappxbundle` 仍为条件支持；不支持 MSIXVC/Xbox、EXE 和 MSI 软件包。
- 主程序通过 `requireAdministrator` 强制以管理员身份启动。仍可选择“当前管理员账户”或“所有用户”；两种范围都由同一主进程直接调用 Windows 包 API。
- NSIS 安装器配置为当前用户安装、阻止降级，并使用 Microsoft WebView2 下载引导程序。这些行为仍需通过干净机器验收；首次安装可能需要网络连接。

已记录的 Sysinternals Suite 往返验证仅证明特定产品、市场、语言、主机、时间和 CurrentUser 场景，不代表普遍兼容，也不保证可通过官方 Store 更新。

## 安装

1. 从发布页面下载与机器架构匹配的安装器。
2. 使用相邻的 `SHA256SUMS.txt` 校验安装器的 SHA-256。
3. 运行 NSIS 安装器。安装器配置为安装到当前 Windows 用户；就地升级时是否完整保留应用数据仍属于干净机器发布检查项。

安装器和主程序均无代码签名。启动主程序时 Windows 会显示 UAC；拒绝 UAC 后应用不会启动。删除独立提权进程没有削弱软件包验证：下载内容仍须通过哈希、manifest identity 和 Microsoft 签名检查后才能进入部署。

## 隐私与网络边界

生产网络审计仅允许以下 Microsoft 主机：

- `displaycatalog.mp.microsoft.com`
- `fe3.delivery.mp.microsoft.com`
- `dl.delivery.mp.microsoft.com`
- `tlu.dl.delivery.mp.microsoft.com`
- `store-images.s-microsoft.com`（仅用于应用图标）

带签名的下载 URL 和代理凭据不会被持久化。诊断导出仅包含封闭事件名、时间戳、版本与架构信息、崩溃恢复状态和主机白名单，不包含 URL、令牌、代理凭据、原始服务响应、HRESULT 或本地路径。可在 **Settings > Export diagnostics**（设置 > 导出诊断）中将 JSON 报告导出到当前用户的“下载”目录。

## 从源码构建

前置条件：

- Windows，以及目标架构对应的 MSVC 构建工具
- `rust-toolchain.toml` 选择的 stable Rust 工具链（CI 当前固定为 `1.98.1`）
- Node.js 24 和 pnpm `8.15.1`
- Tauri 所需的 WebView2 开发与运行时组件

```powershell
pnpm install --frozen-lockfile
pnpm test
pnpm build
cargo test --manifest-path src-tauri/Cargo.toml --lib --tests
pnpm build:release
```

如需跳过完整质量检查并仅构建一个架构：

```powershell
./scripts/build-release.ps1 -Architecture x64 -SkipChecks
./scripts/build-release.ps1 -Architecture arm64 -SkipChecks
```

构建脚本会验证主程序的 PE machine 类型和 `requireAdministrator` manifest，并确认安装包不含旧 sidecar；随后将安装器、`SHA256SUMS.txt`、`BUILD-METADATA.json` 和 `THIRD_PARTY_LICENSES.json` 写入 `release-artifacts/<architecture>/`。

双架构 CI、哈希发布以及升级/卸载验收流程见[发布指南](docs/release.md)；诊断导出契约见[诊断与恢复](docs/diagnostics.md)。

## 开发检查

```powershell
pnpm test
pnpm exec playwright test
pnpm build
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --lib --tests
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm exec tauri build --debug --no-bundle
```

`pnpm exec tauri dev` 启动 Vite 并让 WebView2 连接 `http://localhost:1420`，只用于开发热更新。普通 `cargo build`、`cargo test` 和 `tauri build` 默认启用内嵌前端协议，因此生成的 EXE 不依赖本地 Web 服务器；修改前端后应先运行 `pnpm build`，避免嵌入旧的 `dist`。

## 许可证状态

仓库当前尚未声明项目自身的分发许可证。未经维护者许可，请勿镜像或再分发源代码及二进制文件。第三方依赖许可证元数据由锁定的 Cargo 和 pnpm 依赖图生成，并以 `THIRD_PARTY_LICENSES.json` 随安装包分发。
