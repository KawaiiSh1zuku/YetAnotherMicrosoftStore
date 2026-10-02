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

## M0 提权部署与双层清单（2026-10-02）

- 固化版本化 Broker 协议：长度前缀 JSON 帧、1 MiB 上限、请求 ID/nonce、父 PID/session 校验、包身份、SHA-256 和 AllUsers 卸载目标。
- 实现当前用户与机器范围清单：CurrentUser 使用 `FindPackagesByUserSecurityId`；AllUsers 合并 `FindPackages`、`FindProvisionedPackages`、`FindUsers`，并显式输出 `complete`、用户计数、当前用户/其他用户和预配状态。
- 实现包校验替换：源包拒绝相对路径、UNC/设备路径和重解析点；Broker 在管理员专属 `ProgramData` 暂存根中复制、重新计算 SHA-256、读取 `AppxManifest.xml` 身份后再调用 WinRT 部署 API。
- 实现非提权主程序 + 一次性 `runas` Broker：命名管道 ACL 允许当前所有者、Administrators 和 SYSTEM；Broker manifest 为 `requireAdministrator`，主程序保持 Tauri 默认 `asInvoker`；Broker 校验高完整性、管道服务端 PID/session、nonce 和一次请求后退出。
- 接入 Tauri `scan_installed_packages`、`install_package`、`uninstall_package` 命令；CurrentUser 不启动 Broker，AllUsers 始终走 Broker，并在成功返回前执行完整清单后置校验。
- 新增 `scripts/m0-deployment-acceptance.ps1` 与 `docs/evidence/m0/README.md`。完整脚本在 Windows 10 build 19045 x64 上通过：CurrentUser 安装/卸载 1/1、AllUsers UAC stage/provision/deprovision/remove 1/1、`-WhatIf` 预检通过；脚本 finally 先清包，再按显式指纹清理 `CurrentUser\Root`、`CurrentUser\TrustedPeople`、`LocalMachine\Root`，复核三处匹配数均为 0。
- 真实验收包为既有自签 `.msix`，签名指纹仅用于本次测试，不生成新证书；未把包、PFX/CER、私钥、Broker 生成二进制或临时证据加入 Git。

## 代码扫描与文档同步（2026-10-02）

- 以 `master` / `157c23c52f1e8d08dc389c63e3d1b3d77a8c64c4` 为扫描基线；文档修改前工作区干净，构建生成的 Broker 和前端产物均未改变 Git 状态。
- 代码现状确认：M0 部署路径由 `deployment.rs`、`package_validation.rs`、`inventory.rs`、`broker_protocol.rs`、`broker_launcher.rs` 和 `deployment_coordinator.rs` 组成；M1 由 `catalog.rs`、`resolver.rs`、项目 DTO 和 fixture 契约测试组成；M2 持久化模块尚未创建。
- 本轮重跑 `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo test --manifest-path src-tauri/Cargo.toml --all-targets`、`cargo check --manifest-path src-tauri/broker/Cargo.toml`、`pnpm build` 和 `pnpm exec tauri build --debug --no-bundle` 均成功。Rust 集成测试报告为 25 项通过、3 项 ignored；ignored 项需要签名测试包和 M0 环境变量，不能等同于本轮重跑的真实包验收。
- 发现并同步三处陈旧叙述：设计规格、README 和 2026-10-01 总实施计划仍停留在“进入/批准 M0”；现已改为 M0 验收完成、M1 离线协议适配完成、M2 待开始，并保留实时 Store/FE3、下载、跨渠道更新和 MSIXVC 的未验收边界。
- M0 实施计划原有 Task 1–7 复选框未反映底部完成记录；现已全部勾选，并注明实际以单次最终提交 `157c23c` 交付，未按 Task 拆分提交。

## M2 领域模型与持久化（2026-10-02）

- 评审 M1 DTO/错误边界：`storelib_rs` 类型仍只存在于 `catalog`/`resolver` adapter；M2 不改写 adapter，新增独立领域与前端安全错误转换层。
- 第一轮 TDD 已完成：`m2_domain` 先因 `domain`/`error`/`jobs` 模块不存在而失败，随后 6/6 通过。
- 已固定包身份、版本、架构、语言、市场、格式和安装来源字段；完整文档错误码可稳定序列化，M1 协议详情不会原样进入前端 DTO。
- 任务状态机把普通活动阶段重启恢复为 `Interrupted` 并要求重新解析，把部署中断恢复为 `NeedsReconciliation` 并要求先扫描 Windows 包清单，避免盲目重复部署。
- 固定 `rusqlite = 0.40.2`，关闭默认功能并启用 bundled SQLite；新增 schema v1，覆盖产品、包版本、依赖、任务、缓存、设置、安装来源与诊断索引。
- 第二轮 TDD 已完成：`m2_persistence` 先因领域类型、SQLite 依赖和 persistence 模块不存在而失败，随后 migration 首次执行/重放/失败回滚、repository 往返和重启恢复 4/4 通过。
- 自审补充测试先因代理/诊断模型缺口失败，修正后 M2 目标测试 12/12 通过；代理凭据只持久化策略，不把用户名/密码放入 SQLite，诊断 operation 使用封闭枚举。
- 独立只读审查发现 3 项 Important：重复启动会丢恢复动作、Job 未冻结请求上下文、migration rollback 测试未在成功 DDL 后失败。三项均先补失败测试，再完成修复；M2 目标测试现为 13/13。
- 最终默认特性 `cargo test --all-targets` 为 38 项通过、3 项既有 M0 环境测试 ignored；严格 Clippy、Cargo check、Broker check、`pnpm build` 和 Tauri debug 非 bundle 构建通过。
- 未执行实时 DCAT/FE3、包下载或新的 Windows 安装/卸载；M2 退出证据限定为本地域模型、SQLite 与构建测试。

## 规格/计划细化与 M0-M2 对照审查（2026-10-02）

- 以 `master` / `86cae46` 和干净工作区为审查基线，读取总规格、总实施计划、M0 专项规格/计划、支持矩阵、M0 证据说明、M0-M2 实现与测试。
- 本轮重跑 `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` 通过。
- 本轮重跑 `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`：38 项通过，3 项需要签名包/环境变量的 M0 真实部署测试 ignored；没有失败。
- 本轮重跑默认特性严格 Clippy（`--all-targets -- -D warnings`）、Broker `cargo check` 和 `pnpm build`，均通过。
- 本轮重跑 `pnpm exec tauri build --debug --no-bundle` 通过，生成 debug 主程序并按既有脚本复制 Broker；生成二进制保持被 Git 忽略。
- 不重复执行会改变 Windows 包/证书状态的 M0 验收脚本；M0 真实 CurrentUser/AllUsers/UAC/清理结论只引用已记录的 Windows 10 build 19045 x64 验收证据。
- 审查确认：M0 完成但需修正文档状态和 M5 重复职责；M1 完成的是离线 adapter/fixture 契约而非线上 Store；M2 完成本地领域/SQLite/恢复边界，但 M3 前须补字段与收紧安全错误详情。
- 已细化总规格：新增 E0-E3 证据等级、当前/目标架构、安装来源置信规则、M2 持久化与恢复契约、当前临时 Tauri API 边界和 M0-M9 执行门。
- 已细化总计划：按实际文件结构重写，拆分 M3A/M3B，消除 M5 对 M0 Broker/清单的重复职责，并加入 M0-M2 匹配审查表、可复现命令、风险和退出条件。
- 已将 M0 专项规格状态从“待用户审阅”修正为“已批准并完成”，同时保留验证环境和未覆盖载荷边界。
- 最终 `git diff --check`、陈旧状态词扫描和文档链接目标检查通过；本轮只修改 6 个 Markdown 规划/规格文件，没有修改产品代码或加入生成物。

## M3 适用性与资源选择（2026-10-02）

- M3A 按 TDD 完成：四段 `PackageVersion`、publisher/resource ID/package kind/minimum OS/neutral/content ID 字段和封闭 `SafeErrorDetail` 测试先失败，随后实现通过。
- 新增不可改写历史的 `0002_m3_applicability.sql`；测试覆盖 schema v1→v2 无损升级、旧数据 unknown/null 默认语义、重复打开和在部分 DDL 后失败的事务回滚。
- 扩展 `resolver.rs` 的项目 DTO 映射，从 `storelib_rs 0.1.11` 已有 typed fields 和 package moniker 提取身份、publisher、版本、架构、资源、包种类、最低 OS、语言、content ID 与格式；第三方类型未越过 adapter。
- M3B 新增无 I/O 的 `applicability.rs`，输入 `PackageGraph`、主机能力、用户偏好和已安装清单，输出选定 `ResolvedPackage` 列表与封闭逐包解释。
- 选择器测试覆盖 x64/x86/ARM64/neutral、显式兼容架构、市场、BCP-47 精确/主语言回退、neutral/scale 资源、框架已安装/依赖缺失、最低 OS、MSIXVC 格式门、严格更新、防降级和显式 repair。
- ARM64 对 x64/x86 的兼容性不硬编码，由主机 `compatible_architectures` 明确提供；相同版本按用户架构偏好选择，bundle 在相同版本/偏好下优先。
- 只读复审后补齐 7 个回归边界：v1 任务错误 JSON 白名单兼容、依赖环拒绝、bundle 成员传递依赖、publisher/architecture 安装身份匹配、BCP-47 script 回退、多资源身份分组和 Update 必须存在已安装对象。
- M3 新增 23 项测试并扩展 1 项 M1 fixture 契约；当前全目标为 62 项通过、3 项既有 M0 环境测试 ignored，默认特性严格 Clippy 通过。
- 未执行实时 Display Catalog/FE3、下载、缓存、代理或 Windows 部署；M3 状态限定为本地自动化 E1。

## 阻塞项与风险

| Item | Status | Handling |
|---|---|---|
| `storelib_rs` 非 Microsoft 官方库且协议端点不稳定 | 已知 | 通过 provider trait 隔离、固定 revision、加入 fixture 和替换路径 |
| 全用户部署需要提权/预配语义 | 开放 | 实现前先做原生 Rust/WinRT Spike |
| msixvc 是 Xbox 专用包族 | 已知 | 第一阶段只识别；专门能力验证前不下载、安装或更新 |
| WinINet 与 WinHTTP 的系统代理/PAC 行为不同 | 已知 | M4 只实现 WinHTTP 当前用户静态值；纯 PAC/WPAD 显式拒绝并保留为后续能力门 |

## 本次错误记录

| 错误 | 尝试次数 | 处理 |
|---|---:|---|
| 一次性翻译规格多个段落的 apply_patch 上下文不匹配 | 1 | 拆成按章节的小型补丁，随后成功完成剩余章节 |
| M2 首次使用不存在的 `transaction_with_behavior_unchecked` | 1 | 核对 `rusqlite 0.40.2` 源码，改用支持共享借用且失败回滚的 `unchecked_transaction` |
| migration 失败测试用 `expect_err` 意外要求 `Persistence: Debug` | 1 | 改为直接断言 `Result::is_err`，避免为数据库连接扩大调试接口 |
| `cargo clippy --all-targets --all-features` 编译不到 `run()` | 1 | 确认是 M0 `broker-dependency` 既有 feature 组合问题；不混入 M2 修复，改跑默认桌面特性严格 Clippy并保留失败证据 |
| M2 评审记录补丁使用了不存在的 `findings.md` 章节标题 | 1 | 读取文件尾部后改用实际的“当前代码状态扫描”插入点 |
| M2 Important 修复的跨文件补丁因 `persistence.rs` 格式化上下文不匹配而拒绝 | 1 | 确认补丁未部分应用，拆为 Job、migration、repository 的小型文件级补丁 |
| 最终文件统计循环中的 `$f` 被 PowerShell 在传给 Bash 前展开 | 1 | 改为不含 shell 变量的显式 `wc -l` 文件列表 |
| M0、M1、M2 三个只读审查子代理均返回 `429 Too Many Requests` | 1 | 不采用任何子代理结论，不重复相同并发请求；由主线直接读取实现、测试和证据文档完成审查 |

## M4 下载、缓存与代理（2026-10-02）

- 从 `master` / `14188ad` 的干净基线进入 M4；先形成文件级实施计划，再按 TDD 建立代理/URL 策略、下载、校验和缓存契约。
- 新增 `settings.rs`：disabled、WinHTTP 当前用户静态 system、自定义 HTTP(S) 与 SOCKS5。运行时凭据 Debug 脱敏且不进入 SQLite；纯 PAC/自动检测/WPAD 显式返回未支持。
- 新增 `download.rs` 与 `verification.rs`：HTTPS host allowlist、逐跳重定向复核、Range + If-Range、ETag/Content-Range/总长度变化重启、一次 URL 刷新、取消、并发与聚合限速、大小/SHA-256 流式校验和按内容哈希提升。
- 新增 `cache.rs` 和 persistence cache 查询/删除：启动恢复 partial sidecar，核对 verified 哈希，拒绝缓存根逃逸，按 retention 后 LRU 淘汰并保护活动任务；数据库和 sidecar 不保存临时下载 URL。
- 本地真实 socket fixture 覆盖 15 项下载场景，代理/网络策略 6 项、缓存 7 项；live smoke 作为显式开关测试保持 ignored，普通测试不会访问外网。
- 受控在线 smoke 通过代理于 2026-10-02 成功执行：产品 `9WZDNCRFJ3TJ`、市场 `US`、语言 `en`，观察 20 个包与 81 条依赖。输出只含非敏感输入、时间和计数；未保存 URL/令牌，未下载真实包，未改变安装状态。
- 在线 FE3 暴露 ARM32 moniker，原枚举只有 x86/x64/ARM64/neutral；新增 `Architecture::Arm` 与脱敏 fixture 回归，避免把 ARM32 错映射为 ARM64或因单个包终止整个包图。
- 下载自审补齐三个边界：无 ETag 的 partial 从零开始、`Content-Range` 总长度变化重启、限速等待可取消。fixture 的非阻塞 listener 一度把属性传给 accepted socket，导致 Windows `WouldBlock` 后析构双 panic；已恢复 accepted socket 为阻塞模式并验证默认并行运行。
- 独立只读审查提出 7 类 Important：缓存重解析点约束、校验期取消、空闲限速额度、共享内容计费/淘汰、同 key 并发、chunked 超限和提升后未入库孤儿文件。已逐项补回归测试并修复；同时将 `DownloadRequest` 的 Debug URL 脱敏、禁止把 HTTP-only system proxy 复用于 HTTPS。
- 修复后重跑格式、91 项通过/4 项 ignored 的全目标测试和默认特性严格 Clippy，均成功；M4 聚焦测试为下载 15、网络策略 6、缓存 7。
- M4 文档明确区分：本地 fixture 是真实字节传输的 E1，在线 smoke 只证明指定时点协议适配；真实 Microsoft CDN 包下载、签名验证、代理服务器互操作、磁盘故障和部署编排仍未验收。
