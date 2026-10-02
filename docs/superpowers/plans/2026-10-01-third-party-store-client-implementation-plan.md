# 第三方 Microsoft Store 客户端实现计划

> 状态：已完成 M0-M2 对照审查。M0 为受控 Windows E2 完成，M1/M2 为自动化 E1 完成；M3 尚未开始。实时 Store/FE3、下载、资源选择、跨渠道更新和发布能力仍按后续里程碑门控。

## 1. 执行范围

本计划实现 [第三方 Microsoft Store 客户端设计规格](../specs/2026-10-01-third-party-store-client-design.md) 中已确认的第一阶段能力：

- Rust + Tauri 2 桌面客户端。
- Vite + React + TypeScript + shadcn/ui + Tailwind CSS 前端。
- 通过 Display Catalog/FE3 解析 Microsoft 官方包，复用隔离后的 `storelib_rs`。
- 普通 MSIX/AppX、bundle 及经过验证的 eAppx 路径。
- x64、ARM64、x86，多市场、多语言和资源包选择。
- 禁用、系统、自定义 HTTP(S)、自定义 SOCKS5 代理。
- 可配置缓存目录、大小、保留时间和成功部署后的保留策略。
- 全用户安装的原生 Windows broker/UAC 路径。
- 官方 Store 安装可被本客户端发现；本客户端安装在身份和授权兼容时保持官方 Store 更新资格。
- NSIS 分发。

明确不在第一阶段实现：EXE、MSI、Xbox 包；MSIXVC 只做识别、解释和能力门控，不做下载、安装或更新。

## 2. 实施原则

1. 先验证 Windows 部署权限和协议可用性，再建设完整 UI，避免先做出无法安装的界面。
2. `storelib_rs` 只通过项目自有 trait/DTO 暴露，供应商类型不得进入前端或领域层。
3. 所有外部 URL、令牌、FE3 原文和 WinRT 错误只停留在 Rust 诊断边界。
4. 包身份、签名、依赖图和资源图是跨渠道互操作的事实来源；安装来源只作诊断元数据。
5. 任何更新任务都必须先重新解析、校验哈希/签名并执行防降级检查。
6. 每个里程碑都有可重复的 fixture、单元测试或 Windows 集成验收；未通过不得进入下一里程碑。

### 状态与证据记法

- `未开始`：没有满足进入条件的实现工作。
- `进行中`：已有实现，但退出条件或证据不完整。
- `完成（E1）`：本地 fixture/单元/集成/构建证据满足退出条件。
- `完成（E2）`：指定 Windows、载荷和权限环境完成真实回环及清理。
- `完成（E3）`：指定产品、市场、账户和时间点完成实时 Microsoft 服务/官方 Store 互操作。

里程碑状态只取满足其退出条件的最低充分证据，不允许用更早阶段的构建成功替代更晚阶段的外部验收。

## 3. 当前结构与演进边界

M0-M2 采用按职责分文件的扁平 Rust 模块；不为了匹配早期草案强制搬迁目录。后续新模块沿用这一边界，单个模块明显膨胀后再拆子目录：

```text
src-tauri/
  Cargo.toml
  tauri.conf.json
  capabilities/
  src/
    lib.rs
    main.rs
    catalog.rs                 # M1
    resolver.rs                # M1
    domain.rs                  # M2
    error.rs                   # M2
    jobs.rs                    # M2
    persistence.rs             # M2
    deployment.rs              # M0
    deployment_coordinator.rs  # M0
    inventory.rs               # M0
    broker_protocol.rs         # M0
    broker_launcher.rs         # M0
    package_validation.rs      # M0
    applicability.rs           # M3 计划
    download.rs                # M4 计划
    verification.rs            # M4/M5 计划
    settings.rs                # M4 计划
    tauri_api.rs               # M6 计划
  broker/                      # M0 独立 requireAdministrator binary
  migrations/
    0001_m2.sql
    0002_m3_applicability.sql   # M3 计划；不得改写 0001
  tests/fixtures/
src/
  main.tsx
  app/
  components/
  features/search/
  features/details/
  features/queue/
  features/installed/
  features/settings/
  lib/tauri.ts
  lib/validation.ts
docs/
  evidence/m0/
  support-matrix.md
  superpowers/specs/
  superpowers/plans/
```

## 4. 里程碑拆分

| 里程碑 | 当前状态 | 核心目标 | 退出证据 | 保留边界 |
|---|---|---|---|---|
| M0 基线与部署 Spike | 完成（E2） | 建立 Tauri/Rust 基线、当前用户部署、一次性 UAC Broker 和双层清单 | Windows 10 build 19045 x64 上以既有自签 MSIX 完成 CurrentUser/AllUsers 回环、后置清单和证书清理；自动化协议/校验测试通过 | 其他 Windows 构建、ARM64/x86 主机、bundle/eAppx 真实载荷未验收 |
| M1 Store 协议适配 | 完成（E1） | 固定并隔离 `storelib_rs`，规范化 DCAT/FE3 | `storelib_rs 0.1.11`、`roxmltree 0.20.0` 精确固定；5 项 fixture/adapter 契约测试通过 | 未执行实时 Store/FE3、授权、地区或 CDN URL 验收 |
| M2 领域模型与持久化 | 完成（E1） | 建立包/产品/任务/设置/缓存模型、schema v1 和重启恢复 | 13 项 M2 测试覆盖错误序列化、迁移重放/回滚、repository 往返、请求上下文和恢复语义 | M3 适用性字段与封闭错误详情尚未补齐 |
| M3 适用性与资源选择 | 未开始 | 补齐 schema v2/领域字段，实现 OS、架构、市场、语言、资源和依赖选择 | x64/ARM64/x86、neutral、多个市场/语言、资源包、依赖状态、最低 OS 和防降级表驱动测试通过；选择结果可解释 | 不访问实时下载，不执行部署 |
| M4 下载、缓存与代理 | 未开始 | 实现受控实时协议 smoke、可续传下载、缓存和四种代理模式 | host allowlist、Range、URL 过期重解析、缓存淘汰、disabled/system/HTTP(S)/SOCKS5、日志脱敏通过；记录线上测试时间/市场 | system PAC/WPAD 范围须单独确认；不宣称跨渠道更新 |
| M5 安装/更新编排与身份关联 | 未开始 | 复用 M0 部署原语接入适用包图、版本差异和 Store 产品关联 | 包图安装/更新、清单重扫、严格更新、防降级、PFN/Product ID 关联和稳定错误映射通过 | 不重复实现 Broker；官方 Store 接管仍待 M7 |
| M6 Tauri API 与前端主流程 | 未开始 | 冻结安全命令/事件 DTO，完成搜索、详情、队列、已安装和设置 UI | 移除脚手架接口；UI 完成主流程、进度/取消/恢复、键盘/焦点/窄窗口检查 | 静态 mock 不能代替 M5 后端集成 |
| M7 更新与互操作验证 | 未开始 | 取得双渠道更新和冲突策略的 E3 证据 | 指定产品/市场/账户上完成官方 Store→第三方检测、第三方→官方 Store 手动更新、任一渠道领先不降级和重扫收敛 | 条件性兼容，不作普遍保证 |
| M8 NSIS 与发布加固 | 未开始 | 形成可签名、可升级、可诊断的分发包 | 干净机 NSIS 安装/升级/卸载、Release Broker/安装器签名、WebView2、日志、网络白名单和可访问性回归通过 | 未签名 debug Broker 不能进入发布 |
| M9 MSIXVC 研究门（后续） | 未开始 | 独立评估 MSIXVC/Xbox API、服务和许可要求 | 调研与专门 Spike 经新范围审批 | 第一阶段不下载、不安装、不更新 |

## 5. 依赖关系与并行边界

```text
M0 ──> M1 ──> M2 ──> M3A 模型加固 ──> M3B 适用性 ──> M4 ──> M5 ──> M6B 集成 ──> M7 ──> M8
                                      └──────────> M6A 静态 UI ───────────────┘
M8 ──> M9（新范围审批后）
```

允许的有限并行：

- M3A 冻结字段后，M6A 可以先做静态 UI 和 mock 数据，但不得宣称后端功能完成。
- M4 可以与 M5 的 Product ID/PFN 关联只读研究并行，但 M5 部署编排必须等待 M4 产生已验证的本地包图。
- M8 的 NSIS 配置可在 M6 后预研，但发布验收必须等待 M7。

禁止的并行：

- 不得因 M0 E2 通过而宣称 Store 下载、bundle/eAppx 或跨渠道更新已支持。
- 不得在 `catalog`/`resolver` adapter 之外新增 `storelib_rs` 领域依赖。
- 不得改写 `0001_m2.sql`；M3 字段扩展必须新增 migration 并验证旧库升级。
- 未完成 M7 E3 前不得把“官方 Store 可更新第三方安装”写成已验收，更不得写成无条件承诺。

## 6. 文件级任务清单

### M0：基线与部署 Spike（完成，E2）

- [x] 建立 Tauri 2 + Vite + React + TypeScript 基线并固定工具链/锁文件。
- [x] 在 `deployment.rs`、`deployment_coordinator.rs`、`inventory.rs`、`broker_protocol.rs`、`broker_launcher.rs` 和 `package_validation.rs` 建立当前用户/全用户边界。
- [x] 建立独立 `src-tauri/broker/`，主程序保持 `asInvoker`，Broker 使用 `requireAdministrator` 和一次性受限命名管道。
- [x] 在 Windows 10 build 19045 x64 以既有自签 MSIX 完成 CurrentUser/AllUsers 回环、清单后置条件和证书精确清理。
- [x] 记录 `docs/support-matrix.md`、`docs/evidence/m0/README.md` 和专项 M0 规格/计划。

审查结论：与专项 M0 plan 匹配。普通测试重跑只证明 E1；真实 UAC/包/证书结论引用既有 E2 记录，本轮不重复执行有状态脚本。

### M1：Store 协议适配（完成，E1）

- [x] 定义项目自有 `CatalogProvider`、`PackageResolver`、`CatalogProduct`、`PackageGraph`、`ResolvedPackage` 和依赖边 DTO。
- [x] 将 `storelib_rs` 固定在 `catalog.rs`/`resolver.rs` adapter 中，不让其类型进入 domain 或前端。
- [x] 保存脱敏 DCAT/FE3 fixture，覆盖正常解析、非 HTTPS URL、缺失字段、坏 XML、前置依赖和 bundle 更新边。
- [x] 按 package moniker 关联 `<UpdateIdentity>`，避免依赖响应顺序补 `update_id`。

审查结论：与 M1 离线适配目标匹配。`PackageIdentity`/安装清单归 M0/M2，不再列为 M1 独占产物；production adapter 存在不等于实时端点已验收。

### M2：领域模型与持久化（完成，E1）

- [x] 建立 `domain.rs`、`error.rs`、`jobs.rs` 与 `persistence.rs`。
- [x] 新增不可改写的 `0001_m2.sql`，覆盖产品、包版本、依赖、任务、缓存、设置、安装来源和诊断。
- [x] 验证 migration 首次执行/重放/事务回滚、repository 往返、请求上下文冻结和重启恢复持久化。
- [x] 下载前阶段重启后强制重新解析；部署中断先进入清单 reconciliation。
- [x] 代理设置不存用户名/密码，诊断 operation 使用封闭枚举。

审查结论：满足 M2 原退出条件。M3 开始前仍须通过新 migration 补适用性字段，并把开放 `SafeErrorDetail` 收紧为封闭字段；这些作为 M3A 入口任务，不回写或伪造 M2 历史。

### M3A：模型加固与 schema v2

- 在 `PackageRecord`/resolver 映射中补齐 publisher、resource ID、package kind、最低 Windows build、资源限定/neutral 信息和可用的内容标识。
- 新增 `0002_m3_applicability.sql`，验证从 schema v1 升级、重复执行、失败回滚和旧数据默认语义。
- 将 `SafeErrorDetail` 改为封闭 enum/typed detail；建立允许字段与错误码的映射测试。
- 为版本建立可比较的四段值或强类型，避免用任意字符串执行更新比较。

退出条件：旧 M2 数据库可无损升级；DTO/错误快照稳定；没有 URL、令牌、响应正文或凭据可进入前端/SQLite 安全字段。

### M3B：适用性与资源选择

- 新建 `applicability.rs`，输入主机能力、用户偏好、已安装清单和 `PackageGraph`，输出选定包图与逐项解释。
- 实现 OS build、x64/ARM64/x86/neutral、市场、BCP-47 语言回退、资源包、框架依赖、bundle 和格式门控。
- 明确 ARM64 主机对 x64/x86 的兼容策略；不得仅凭 CPU 架构硬编码选择。
- 实现严格更新与防降级；“修复”请求必须是单独策略而不是更新默认路径。

退出条件：表驱动/属性测试覆盖架构、市场、语言、资源包、依赖已安装/缺失、最低 OS、版本领先和不支持格式；选择结果可序列化供 UI 解释。

### M4：实时解析、下载、缓存与代理

- 在显式测试开关下对 production adapter 做受控实时 smoke，记录市场、语言、产品和时间，不保存临时 URL/令牌。
- 新建 `ProxyProvider`：disabled、system、自定义 HTTP(S)、SOCKS5；凭据只引用 Windows Credential Manager 或运行时输入。
- 实现 Range 续传、ETag/长度变化处理、取消、限速/并发、URL 过期重新解析和按内容哈希原子落盘。
- 实现缓存上限、保留时间、verified/partial 分离和重启恢复；SQLite 不保存敏感 URL。
- 实现 host/redirect allowlist、大小/SHA-256 流式验证和日志脱敏。

退出条件：本地可控 HTTP fixture 与受控实时 smoke 均通过；四种代理模式和缓存恢复有证据；失败不会改变已安装包。

### M5：安装/更新编排与身份关联

- 复用 M0 `DeploymentCoordinator`/Broker/Inventory，把 M3 选定且 M4 验证的本地包图转换成 `VerifiedPackageSet`。
- 扩展包图部署和依赖 URI 顺序，不复制另一套提权/部署实现。
- 建立 PFN/Product ID/Content ID 的关联缓存和置信状态；不能关联时返回明确状态，不猜测来源。
- 实现安装清单与新鲜目录图的版本差异、严格更新、防降级和更新后重扫收敛。
- 将 M0 字符串错误和 Broker 内部错误映射为稳定 `AppErrorDto`；原始 HRESULT 只进入脱敏诊断。

退出条件：普通 MSIX/AppX 包图的 CurrentUser/AllUsers 安装/更新集成通过；官方 Store 已安装包能进入第三方扫描候选；Store 接管更新仍不在本里程碑宣称完成。

### M6：Tauri API 与前端主流程

- 移除 `greet` 和产品不需要的 Spike 命令；冻结命令、事件、取消/恢复和错误 DTO。
- 使用 shadcn/ui 组合搜索、详情、队列、已安装和设置页面。
- 展示架构、市场、语言、资源包、依赖、安装来源置信状态和互操作限制。
- 接入任务进度、暂停/恢复/取消、等待提权和错误恢复。

退出条件：Playwright 或等价 UI 冒烟、事件乱序/重连、键盘导航、焦点、窄窗口、屏幕阅读器和本地化错误检查通过。

### M7：更新与跨渠道互操作验证

- 选定具有合法授权、可重复恢复且风险可控的测试产品/账户/市场矩阵。
- 验证官方 Store 安装→第三方清单/更新检测，以及第三方安装→官方 Store 手动更新。
- 验证任一渠道版本领先、目录滞后、市场不可用和授权缺失时不降级并给出明确状态。
- 每次外部验收记录日期、版本、市场、账户类型和清理/恢复结果。

退出条件：获得限定范围的 E3 互操作报告；无法证明的产品保持“未知/条件不满足”，不提升为普遍兼容。

### M8：NSIS 与发布加固

- 配置 Tauri NSIS bundle、版本号、安装目录、升级/卸载行为和 WebView2 前置策略。
- 建立 Release 主程序/Broker/安装器签名、时间戳和证书轮换方案。
- 加入诊断导出、网络主机审计、日志脱敏、崩溃恢复和依赖许可证清单。

退出条件：干净机安装/升级/卸载、Release 签名验证、无 PowerShell/winget 产品子进程和完整回归矩阵通过。

### M9：MSIXVC 研究门（后续）

- 单独记录 Xbox/MSIXVC API、服务、许可、磁盘和流式安装要求。
- 未获得新的范围批准前，不修改第一阶段支持矩阵，也不实现下载、安装或更新。

## 7. M0-M2 进度与计划匹配审查

| 里程碑 | 原计划核心要求 | 当前实际证据 | 偏差与处置 | 结论 |
|---|---|---|---|---|
| M0 | 工程基线、当前用户部署、全用户 UAC/预配、双层清单、支持矩阵 | 提交 `157c23c`；M0 专项 Task 1-7；E1 测试；Windows 10 19045 x64 的 CurrentUser/AllUsers E2 回环与证书清理记录 | 总计划曾把 Broker/机器清单重复放到 M5；已把 M5 改为复用 M0。专项规格陈旧“待审阅”状态已修正 | 匹配，完成（E2） |
| M1 | `storelib_rs` 隔离、DCAT/FE3 规范化、fixture 契约 | `catalog.rs`、`resolver.rs`、5 项 `m1_protocol` 测试，依赖精确固定 | 原计划把 Windows `PackageIdentity`/安装清单混入 M1；已归回 M0/M2。没有实时端点证据，明确留到 M4 | 匹配，完成（E1，离线） |
| M2 | 领域 DTO、稳定错误码、任务状态机、SQLite schema/repository、迁移与恢复 | `domain.rs`、`error.rs`、`jobs.rs`、`persistence.rs`、`0001_m2.sql`；13 项 M2 测试 | publisher/resource ID/package kind/min OS 和封闭 error detail 尚缺；不属于原 M2 最低退出条件，已提升为 M3A 强制入口任务 | 匹配，完成（E1，本地） |

本轮可复现验证命令：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo check --manifest-path src-tauri/broker/Cargo.toml
pnpm build
```

当前结果：38 项 Rust 测试通过，3 项需要签名包/环境变量的 M0 真实部署测试 ignored；格式、默认特性严格 Clippy、Broker check 和前端构建通过。ignored 测试不替代历史 E2 验收，也不构成实时 Store/FE3 或跨渠道证据。

## 8. 风险、回滚与停止条件

| 风险 | 触发条件 | 处理 |
|---|---|---|
| DCAT/FE3 变化 | fixture 或线上解析持续失败 | 锁定失败样本，暂停发布，替换 adapter，不把协议修补散落到业务层 |
| storelib_rs 不兼容 | API/协议变化或 crate 无法审计 | 保留 trait 边界，切换项目自有实现；不改领域 DTO |
| M3 migration 破坏旧库 | schema v1 升级丢字段、失败后残留半套结构 | 使用新 migration、事务回滚和旧库 fixture；禁止改写 `0001_m2.sql` |
| 安全错误详情泄漏 | URL、令牌、服务正文或用户路径进入 DTO/SQLite | M3A 封闭 detail 类型；快照测试和日志扫描失败即停止 |
| 全用户部署在新环境不可行 | 非 19045、不同架构或包图无法机器范围预配 | 保持“受限/不支持”错误；不静默回退当前用户；扩展支持矩阵后再声明 |
| 官方 Store 不接管更新 | 身份、授权、市场或渠道不匹配 | 显示稳定原因；保留本客户端直连更新；不宣称官方队列已接受 |
| MSIXVC 研究越界 | 需要 Xbox 专用服务、许可或闭源接口 | 停在 M9 调研门；第一阶段不下载、不安装、不更新 |
| 代理/PAC 差异 | system 模式无法复现用户环境 | 记录受支持范围，必要时将 PAC/WPAD 从首发承诺中移除 |

任何涉及重打包、重签名、绕过授权、执行 EXE/MSI 或调用 PowerShell/winget 的实现都必须停止并重新评审。

## 9. 完成定义

一个里程碑只有同时满足以下条件才可标记完成：

1. 产物和文件级任务已写入仓库。
2. 对应单元、fixture、Windows 集成或发布验证已执行，并记录结果。
3. 已知错误和未覆盖环境写入 `findings.md` 与 `progress.md`。
4. 规格、实现和支持矩阵没有相互矛盾的承诺。
5. 未把条件性互操作能力描述成普遍保证。

## 10. 当前执行点

中文规格和本实现计划已获批准，M0-M2 的进度/代码/测试/证据与计划已完成对照审查。下一步进入 M3A：新增 schema v2、补齐适用性字段、强类型版本和封闭安全错误详情；M3A 通过后再实现 M3B 选择器。实时 Display Catalog/FE3、真实下载、跨渠道更新、NSIS 发布和 MSIXVC 仍未验收，不得从 fixture、SQLite 或本地构建结果推断支持。
