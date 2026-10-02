# 进度日志

## 2026-10-01

- 将请求分类为架构和实施规划任务。
- 确认工作区为空，没有 Git 历史或既有文档。
- 比较三条实现路线，选择直连 DCAT/FE3 + Windows 包部署。
- 确认用户范围：`storelib_rs`、MSIXVC 目标、全用户安装、x64/ARM64/x86、多市场/多语言、代理/缓存设置和 NSIS 分发。
- 查阅 PackageManager、包部署、MSIXVC、Tauri/WebView2 和 Windows 代理文档。
- 阅读 ui-styling 的组件、主题、可访问性和响应式参考。
- 创建规划文件并开始正式架构规格。
- 完成 `docs/superpowers/specs/2026-10-01-third-party-store-client-design.md` 的中文化和自检。
- 明确第一阶段只识别并门控 MSIXVC，不下载、安装或更新；独立 MSIXVC/Xbox 能力位于 M9。
- 将跨渠道互操作、安装来源、身份/授权边界写入规格和发现记录。
- 新建中文实现计划，包含 M0-M9 里程碑、文件级任务、依赖关系、验收标准和停止条件。
- 修正里程碑依赖图，使 M6 明确依赖 M2 后的 DTO 边界，并与 M7 汇合。
- 未执行产品代码、依赖安装或外部状态变更。

## M0 开发（2026-10-01）

- 使用 Tauri 2 + Vite + React + TypeScript 脚手架建立根工程，固定 stable Windows MSVC 工具链声明和 pnpm/Cargo 锁文件。
- 将 Tauri Rust crate 锁定到 2.12.1 版本链；记录本机 Rust 1.98.1、Tauri CLI 2.12.1 和 Windows 10 build 19045 x64。
- 新建 `src-tauri/src/deployment.rs`，通过 `windows` crate 激活 `PackageManager`，返回当前用户/全用户能力状态；全用户明确保持 `RequiresElevation`。
- 新建 `src-tauri/src/broker.rs`，只接受绝对路径和 MSIX/AppX 包扩展，拒绝 `.msixvc`、Xbox、`.exe` 和 `.msi`；未实现任意命令或 shell 执行。
- 新建 `docs/support-matrix.md`，记录包格式、权限证据和 M0 停止条件。
- 新建 `src-tauri/tests/m0_contract.rs`；已观察到契约测试先失败，再通过 4/4；测试目标明确限定 Windows。
- 真实包安装/卸载和签名 broker/UAC IPC 尚未验证，需批准的测试包载荷与独立 Spike 后再进入 M5。
- M0 基线/探针验证完成：`cargo fmt --check`、`cargo check`、`cargo test`（4/4）、`pnpm build`、`pnpm exec tauri build --debug --no-bundle`、`pnpm exec tauri info` 均通过；`pnpm exec tauri dev` 已启动本地 Vite/WebView2 桌面壳并在冒烟后手动终止。
- 根据 M0 退出条件，真实当前用户安装/卸载与签名 broker/UAC IPC 仍开放；因此本次交付标记为“基线与部署 API 探针完成”，不标记为“部署验收完成”。

## M1 Store 协议适配（2026-10-02）

- 固定 `storelib_rs = 0.1.11` 和 `roxmltree = 0.20.0`；第三方协议类型只在 `catalog`/`resolver` adapter 内部使用。
- 新增项目自有 `CatalogProvider`、`PackageResolver`、`CatalogProduct`、`PackageGraph`、`ResolvedPackage` 和依赖边 DTO，供后续 M2/M6 使用。
- DCAT adapter 支持搜索结果和产品 fixture，规范化 Product ID、Package Family Name、标题、发布者、包格式和框架依赖，并拒绝非 HTTPS 包 URL。
- FE3 resolver 复用 `storelib_rs` 的包/关系解析，按 package moniker 补齐 update ID，输出包记录、前置依赖和捆绑更新边。
- 新增脱敏合成 JSON/XML fixtures，覆盖正常解析、缺失字段、失效 URL、坏 XML 和依赖边。
- TDD 契约测试先在模块未实现时失败，随后 `m1_protocol` 5/5 通过；`cargo test --all-targets`（M0 4/4、M1 5/5）、`cargo check --all-targets`、`cargo fmt --all -- --check` 和 `pnpm build` 均通过。
- 未执行实时 Display Catalog/FE3 请求、真实下载或包安装；因此 M1 的退出证据限定为 fixture/adapter 契约，不扩展为线上服务或部署验收。

## 阻塞项与风险

| Item | Status | Handling |
|---|---|---|
| `storelib_rs` 非 Microsoft 官方库且协议端点不稳定 | 已知 | 通过 provider trait 隔离、固定 revision、加入 fixture 和替换路径 |
| 全用户部署需要提权/预配语义 | 开放 | 实现前先做原生 Rust/WinRT Spike |
| msixvc 是 Xbox 专用包族 | 已知 | 第一阶段只识别；专门能力验证前不下载、安装或更新 |
| WinINet 与 WinHTTP 的系统代理/PAC 行为不同 | 已知 | 显式建模代理模式，测试 system、HTTP(S)、SOCKS5 和 PAC 情况 |

## 本次错误记录

| 错误 | 尝试次数 | 处理 |
|---|---:|---|
| 一次性翻译规格多个段落的 apply_patch 上下文不匹配 | 1 | 拆成按章节的小型补丁，随后成功完成剩余章节 |
