# Yet Another Microsoft Store 架构与实现计划

## 目标

设计一个以 Rust + Tauri 为基本技术栈、通过 Microsoft 官方交付端点直连获取包的第三方 Microsoft Store 客户端。

## 当前状态

- 范围与架构：已完成
- 证据评审：已完成
- 中文规格文档：已完成，已批准进入 M0
- 中文实现计划与里程碑：已完成，已批准进入 M0
- 规格与实施计划细化：已完成，已同步当前架构、证据等级和 M5 执行门
- M0：当前用户与全用户部署验收完成；双层清单、一次性 UAC Broker 和证书清理已验证
- M1：Store 协议 adapter、规范化 DTO、DCAT/FE3 脱敏 fixtures 和契约测试完成；未宣称线上 Store/FE3 实时验收
- M2：领域 DTO、安全错误契约、可恢复任务状态机、SQLite schema v1 和 repository 已完成
- M3：schema v2、强类型版本、封闭错误详情、适用性与资源选择器已完成
- M4：受控 DCAT/FE3 在线 smoke、静态 Windows/自定义代理、续传下载、流式校验、缓存恢复与淘汰已完成
- M0-M4 计划匹配审查：已完成；M0 为受控 Windows E2，M1-M3 为自动化 E1，M4 为 E1 加受控在线协议 smoke
- 产品代码：Tauri/Rust/React 基线以及 M1-M4 协议、持久化、选择、下载/缓存边界已建立；M5 尚未开始

## 已确认决策

- 采用直连 DCAT/FE3 路径。
- 使用 Tauri 2、Vite + React + TypeScript 和 shadcn/ui。
- 隔离 `storelib_rs` 适配器。
- 支持 x64、ARM64、x86，多市场以及多语言/资源包。
- 支持禁用、系统、自定义 HTTP(S) 和 SOCKS5 代理模式。
- 支持配置缓存目录和保留策略。
- 使用 NSIS 分发。
- 普通 MSIX/AppX 支持全用户安装。
- 安装来源只作观测元数据，使官方 Store 和本客户端可以看到同一包身份与版本。
- MSIXVC 作为目标能力，但 Xbox 包延后到第一阶段之后。
- EXE 和 MSI 延后到第一阶段之后。
- 官方 Store 与第三方客户端的互操作按“身份/授权兼容”处理，不承诺任一客户端控制另一方更新队列。

## 下一步门槛

1. M5：把 M3 选定、M4 已验证的本地包图接入既有 M0 `DeploymentCoordinator`/Broker/Inventory。
2. M5：完成 PFN/Product ID/Content ID 关联、严格版本差异、防降级、部署后清单重扫和稳定错误映射。
3. M5 不重复实现 Broker，不把 M4 在线协议 smoke 当作真实 CDN 下载或跨渠道互操作证据。
4. PAC/WPAD 保留为独立代理能力门；M4 system 模式只承诺 WinHTTP 当前用户静态代理配置。

## 当前代码验证（2026-10-02）

- 工作区扫描基线为 `master` / `86cae46`；本轮文档修改前工作区干净，未发现未跟踪的证书、私钥、Broker 二进制或临时验收证据。
- 实际代码边界包括 `catalog.rs`/`resolver.rs` 的 M1 adapter，以及 `deployment.rs`、`broker_protocol.rs`、`broker_launcher.rs`、`deployment_coordinator.rs`、`inventory.rs` 和 `package_validation.rs` 的 M0 部署路径。
- 本轮重跑 `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo test --manifest-path src-tauri/Cargo.toml --all-targets`（25 项通过，3 项需要签名包/环境变量的真实集成测试保持 ignored）、`cargo check --manifest-path src-tauri/broker/Cargo.toml`、`pnpm build` 和 `pnpm exec tauri build --debug --no-bundle` 均通过。
- 真实 UAC/包/证书回环证据仍以 `progress.md`、`findings.md` 和 `docs/evidence/m0/README.md` 中已记录的验收为准；本轮没有重新执行会改变 Windows 包或证书状态的脚本。
- M2 新增测试使默认特性全量结果达到 38 项通过、3 项 M0 真实包测试 ignored；严格 Clippy、Cargo check、Broker check、前端构建和 Tauri debug 非 bundle 构建通过。
- `cargo clippy --all-targets --all-features` 仍会触发 M0 `broker-dependency` 与桌面 binary 的既有 feature 组合错误；本轮未把该 M0 构建边界修复混入 M2。
- 本轮重新验证格式、38 项通过/3 项 ignored 的全目标测试、默认特性严格 Clippy、Broker check 和前端构建；没有重复执行会改变包/证书状态的 M0 验收脚本。
- M3 新增 23 项测试并扩展 1 项 M1 fixture 契约；当前全目标结果为 62 项通过、3 项 M0 真实包测试 ignored，默认特性严格 Clippy 通过。
- M3 证据只覆盖 schema v1→v2、本地 DTO/选择算法和脱敏 FE3 fixture；没有执行实时 Store、下载或部署。
- M4 新增 Windows 静态系统代理、HTTP(S)/SOCKS5、host/redirect allowlist、Range+ETag/长度变化、取消、并发/限速、URL 刷新、大小/SHA-256、verified/partial 恢复与 retention/LRU 测试；独立审查修复后最终全目标结果为 91 项通过、4 项 ignored。
- 显式开关下的受控在线 smoke 于 2026-10-02 对 `9WZDNCRFJ3TJ`、`US`、`en` 成功观察到 20 个包和 81 条依赖；未保存临时 URL，未下载真实包，未改变安装状态。

## 约束

- 将获取、选择、下载、验证和部署放在相互独立的 Rust 接口之后。
- 将 DCAT/FE3 视为不稳定的外部契约，固定协议 fixture 并测试。
- 将跨渠道 Store 互操作视为身份/授权兼容性，不承诺任一客户端控制另一方更新队列。
- 在专门部署要求验证前，绝不宣称支持 MSIXVC 安装。
- 外部网页研究结果只写入 `findings.md`。

## 里程碑索引

- M0：基线与部署 Spike
- M1：Store 协议适配
- M2：领域模型与持久化
- M3A：领域/schema 加固
- M3B：适用性与资源选择
- M4：下载、缓存与代理
- M5：原生部署与跨渠道清单
- M6：Tauri API 与前端主流程
- M7：更新与互操作验证
- M8：NSIS 与发布加固
- M9：MSIXVC 研究门（后续）

详细文件级任务见 `docs/superpowers/plans/2026-10-01-third-party-store-client-implementation-plan.md`。
