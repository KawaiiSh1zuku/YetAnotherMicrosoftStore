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
- M5：schema v3、verified 包图、WinTrust 预检、身份关联、严格版本决策与部署后收敛已完成 E1
- M0-M5 计划匹配审查：已完成；M0 为受控 Windows E2，M1-M3/M5 为自动化 E1，M4 为 E1 加受控在线协议 smoke
- M6：已完成；schema v4 追加式事件日志、durable command inbox、generation-fenced 租约 worker、安全 Tauri DTO、五视图前端和单产品 CurrentUser 真实回环均已验收
- 产品代码：Tauri/Rust/React 主流程已串联 M1-M5 的协议、选择、下载/缓存、签名预检和原生部署边界；跨渠道更新、发布加固和 MSIXVC 仍分别受 M7-M9 门控

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

1. M7：用既有官方 Store 安装或更新同身份包，验证来源无关关联、防降级和跨渠道互操作；M6 单产品 CurrentUser 回环不替代该 E3 门。
2. M7：扩展真实产品/市场/语言/架构矩阵，并继续以完整 Windows 清单而不是客户端数据库判定已安装状态。
3. PAC/WPAD 保留为独立代理能力门；当前 system 模式只承诺 WinHTTP 当前用户静态代理配置。
4. M8：完成 NSIS、Release Broker 签名、干净机安装/升级/卸载与发布诊断。

## M5 实施结果（2026-10-03）

1. schema v3 与身份关联：保存 PFN/Product ID/Content ID、关联证据和置信状态，纯函数输出更新可用/最新/高于目录/无法关联。
2. verified 包图装配：只消费 M3 `SelectionResult` 与 M4 `CacheState::Verified` 条目，生成 M0 `VerifiedPackageSet`，按依赖图稳定排序并在部署前复核哈希、manifest identity 与 Windows 签名。
3. 部署与收敛：以 trait 隔离自动化测试，生产 adapter 只调用 M0 `DeploymentCoordinator`；部署后按 identity/publisher/version/architecture/PFN 完整重扫，成功后记录 `ThisClient`，失败映射为封闭 `AppErrorDto`。

验证边界：默认测试提供 E1 领域/持久化/编排证据；真实签名包、UAC 与 Microsoft CDN 仍使用显式 ignored 环境门，不能由 E1 推断。

## 当前代码验证（2026-10-03）

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
- M5 新增 schema v3、身份关联、verified 包图计划、WinTrust 预检、严格版本决策和编排收敛；独立审查修复五项 Important 后，全目标结果为 110 项通过、5 项真实环境测试 ignored。
- M5 最终格式检查、默认特性严格 Clippy、Broker check、前端构建和 Tauri debug 非 bundle 构建通过；本轮未执行真实 Microsoft CDN 下载、签名包图部署、UAC 或跨渠道 Store 更新。
- M6 新增 schema v4 事件/投影、durable command inbox、generation-fenced worker lease、封闭的 13 个 Tauri 命令、`job://changed` cursor 提示和搜索/详情/队列/已安装/设置五视图；前端 10/10、Playwright 2/2 通过。
- M6 修正真实目录契约：FE3 `prerequisites` 作为 Windows Update category GUID 保留审计，不再伪造包依赖边；DCAT 命名 framework 依赖携带最低版本，选择器按 architecture/version/已安装清单解析。
- M6 允许 `dl.delivery.mp.microsoft.com` 与 `tlu.dl.delivery.mp.microsoft.com` 的默认端口 HTTP/HTTPS 交付 URL；任何下载仍必须同时有期望大小和 SHA-256，重定向逐跳复核，其他主机、凭据、fragment 或非默认端口均拒绝。
- 2026-10-03 在 Windows 10 build 19045 x64 上，以产品 `9P7KNL5RWT25`、市场 `US`、语言 `en-US` 完成真实 Microsoft 签名 `.msixbundle` CurrentUser 回环：下载 300,193,716 字节、校验 SHA-256/manifest/WinTrust、安装后精确清单验证、卸载并恢复原始零目标清单。未导入测试证书、未触发 UAC、未执行 AllUsers 或官方 Store 跨渠道更新。
- M6 最终门通过：Rust 全目标 171 项通过、7 项显式环境测试 ignored，严格 Clippy、Broker check、前端构建、Tauri debug no-bundle、Vitest 10/10、Playwright 2/2 与 staged diff 检查均通过；单产品 E2 不扩展为 M7 E3 或普遍兼容声明。

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
