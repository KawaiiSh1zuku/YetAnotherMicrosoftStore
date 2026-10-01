# 第三方 Microsoft Store 客户端实现计划

> 状态：待规格批准。本文是文件级实现计划与里程碑拆分，不授权立即编写产品代码。

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

## 3. 目标目录结构

实现开始后，预期建立以下结构；具体文件名可在 M0 里程碑根据 Tauri 模板微调：

```text
src-tauri/
  Cargo.toml
  tauri.conf.json
  capabilities/
  src/
    lib.rs
    main.rs
    error.rs
    model/
    catalog/
    resolver/
    applicability/
    download/
    verification/
    inventory/
    deployment/
    broker/
    jobs/
    settings/
    storage/
    tauri_api/
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
  superpowers/specs/
  superpowers/plans/
```

## 4. 里程碑拆分

| 里程碑 | 目标 | 主要产物 | 进入条件 | 退出验收 |
|---|---|---|---|---|
| M0 基线与部署 Spike | 建立 Tauri 工程并验证 Windows 原生部署边界 | 工程骨架、Rust/前端最小启动、broker 原型、支持矩阵记录 | 规格批准；明确最低 Windows 构建版本 | 能在测试机启动 Tauri；不创建 PowerShell/winget 子进程；证明当前用户部署；验证或明确全用户 UAC/预配可行性 |
| M1 Store 协议适配 | 固定并隔离 `storelib_rs`，解析 DCAT/FE3 | `catalog`/`resolver` trait、适配器、脱敏 JSON/XML fixtures | M0 完成；选定 crate 版本或 Git revision | fixture 解析、搜索、Product ID/PFN 关联、FE3 包记录和依赖边测试通过 |
| M2 领域模型与持久化 | 建立包、身份、任务、设置和缓存元数据模型 | Rust DTO、SQLite schema/migrations、错误码、状态机 | M1 的 DTO 边界稳定 | 重启后任务可恢复；包身份、版本、架构、语言、市场和来源字段可持久化；迁移可回滚/重放 |
| M3 适用性与资源选择 | 支持架构、市场、语言、资源包和依赖选择 | `applicability`、选择解释、包矩阵 | M1/M2 完成 | x64/ARM64/x86、多个市场/语言、资源包、依赖缺失/已安装和 OS 版本筛选测试通过 |
| M4 下载、缓存与代理 | 实现可续传下载、缓存策略和四种代理模式 | `download`、`ProxyProvider`、缓存索引、哈希验证 | M2/M3 完成；网络 host allowlist 确认 | Range 续传、URL 过期重解析、缓存淘汰、直连/系统/HTTP(S)/SOCKS5 测试通过；日志不泄露 URL/凭据 |
| M5 原生部署与跨渠道清单 | 实现普通安装、全用户 broker、身份扫描和更新比较 | `deployment`、`broker`、`inventory`、版本差异引擎 | M0 部署 Spike、M3/M4 完成 | 普通 MSIX/AppX 安装/更新成功；官方 Store 安装可被清单发现；包身份/签名/依赖不被改变；无法互操作时有稳定错误码 |
| M6 Tauri API 与前端主流程 | 提供稳定命令/事件和可用 UI | Tauri commands/events、搜索、详情、队列、已安装、设置页面 | M2-M5 的 DTO 与错误码冻结 | UI 可搜索、解析、预览选择、启动/暂停/取消任务、扫描更新；键盘导航和焦点行为通过检查 |
| M7 更新与互操作验证 | 闭环验证双渠道更新与冲突策略 | 更新编排、安装来源标记、互操作测试报告 | M5/M6 完成；准备有授权的测试产品 | 官方 Store 安装→第三方检测；第三方安装→官方 Store 手动更新（条件满足时）；任一渠道领先不降级；更新后重新扫描收敛 |
| M8 NSIS 与发布加固 | 形成可分发的 Windows 客户端 | NSIS 配置、签名/版本策略、诊断包、回归矩阵 | M0-M7 完成 | 干净机器安装/卸载/升级；WebView2 前置行为明确；网络白名单、日志脱敏、无 PowerShell/winget 子进程、可访问性冒烟通过 |
| M9 MSIXVC 研究门（后续） | 单独评估 MSIXVC/Xbox 能力，不混入第一阶段 | 调研报告、API/COM Spike、独立支持矩阵 | M8 完成且有明确授权 | 只有在专门部署验证和产品范围批准后，才决定是否进入实现；否则保持能力门控 |

## 5. 依赖关系与并行边界

```text
M0 ──> M1 ──> M2 ──> M3 ──> M4 ──> M5 ──> M7 ──> M8
              └────────────────────────> M6 ────────┘
M9 仅在 M8 后单独评估
```

允许的有限并行：

- M3 通过 DTO 评审后，M6 可以先做静态 UI 和 mock 数据，但不得宣称后端功能完成。
- M4 可以与 M5 的 inventory 读路径并行，但部署写路径必须等待 M0/M5 的权限验证。
- M8 的 NSIS 配置可在 M6 后预研，但发布验收必须等待 M7。

禁止的并行：

- 未完成 M0 前不得把“全用户安装”写成已支持。
- 未完成 M1 fixture 前不得在业务模块散落 storelib_rs 调用。
- 未完成 M5 前不得把“官方 Store 可更新第三方安装”写成无条件承诺。

## 6. 文件级任务清单

### M0：基线与部署 Spike

- 初始化 Tauri 2 + Vite + React + TypeScript 工程和 workspace 目录。
- 固定 Rust toolchain、Node 包管理器和 Windows 最低构建版本。
- 创建 `src-tauri/src/deployment/` 与 `src-tauri/src/broker/` 最小接口。
- 用 `windows` crate 验证 PackageManager 调用、当前用户部署和 UAC broker 通路。
- 编写 `docs/support-matrix.md`，记录 Windows 构建、架构、包格式和权限结果。

验证：启动 smoke test、依赖锁文件检查、当前用户安装/卸载测试、broker 安全边界审查。

### M1-M2：协议适配与领域基础

- 定义 `CatalogProvider`、`PackageResolver`、`PackageGraph`、`PackageIdentity` 和 `InstalledPackage`。
- 将 `storelib_rs` 固定在单一 adapter 中，禁止 UI 直接依赖其类型。
- 保存脱敏 DCAT/FE3 fixture，并覆盖失效 URL、缺失字段和依赖错误。
- 建立 SQLite migrations：产品、包版本、依赖、任务、缓存、设置、安装来源和诊断索引。
- 定义稳定错误码和前端安全 DTO。

验证：解析 fixture、schema migration round-trip、错误序列化快照、重启恢复测试。

### M3-M5：解析、下载与部署闭环

- 实现确定性选择器：OS、架构、市场、语言、资源包、依赖和包格式。
- 实现 `ProxyProvider` 和下载器：Range、重试、取消、URL 过期重解析、缓存索引。
- 在部署前执行大小、SHA-256、身份、版本和发布者检查。
- 实现普通 PackageManager 路径和受限 IPC 的特权 broker。
- 实现当前用户/机器范围清单，关联 PFN、Product ID、版本、资源包和观测来源。
- 实现版本比较、防降级、来源无关更新任务和可解释错误。

验证：四种代理模式、缓存恢复、包签名/哈希失败、UAC 取消、官方 Store 安装检测、条件性 Store 更新测试。

### M6-M7：前端与更新体验

- 固定 Tauri command/event schema，前端仅使用 DTO。
- 使用 shadcn/ui 组合搜索、详情、队列、已安装和设置页面。
- 展示架构、市场、语言、资源包、依赖、安装来源和互操作限制。
- 接入任务进度、暂停/恢复/取消、等待提权和错误恢复。
- 完成跨渠道版本冲突、更新后清单收敛和不降级策略。

验证：Playwright 或等价 UI 冒烟、键盘操作、屏幕阅读器检查、事件乱序恢复、错误码本地化。

### M8-M9：发布与后续能力门

- 配置 Tauri NSIS bundle、版本号、安装目录、升级/卸载行为。
- 加入诊断导出、网络主机审计、日志脱敏和崩溃恢复。
- 在独立文档中记录 MSIXVC/Xbox 调研，不修改第一阶段支持矩阵，直到获得新的批准。

验证：干净机安装/升级/卸载、无系统组件子进程审计、签名/哈希清单、回归矩阵归档。

## 7. 风险、回滚与停止条件

| 风险 | 触发条件 | 处理 |
|---|---|---|
| DCAT/FE3 变化 | fixture 或线上解析持续失败 | 锁定失败样本，暂停发布，替换 adapter，不把协议修补散落到业务层 |
| storelib_rs 不兼容 | API/协议变化或 crate 无法审计 | 保留 trait 边界，切换项目自有实现；不改领域 DTO |
| 全用户部署不可行 | 目标 Windows/包格式无法机器范围预配 | 保持“受限/不支持”错误；不静默回退当前用户；重新提交范围决策 |
| 官方 Store 不接管更新 | 身份、授权、市场或渠道不匹配 | 显示稳定原因；保留本客户端直连更新；不宣称官方队列已接受 |
| MSIXVC 研究越界 | 需要 Xbox 专用服务、许可或闭源接口 | 停在 M9 调研门；第一阶段不下载、不安装、不更新 |
| 代理/PAC 差异 | system 模式无法复现用户环境 | 记录受支持范围，必要时将 PAC/WPAD 从首发承诺中移除 |

任何涉及重打包、重签名、绕过授权、执行 EXE/MSI 或调用 PowerShell/winget 的实现都必须停止并重新评审。

## 8. 完成定义

一个里程碑只有同时满足以下条件才可标记完成：

1. 产物和文件级任务已写入仓库。
2. 对应单元、fixture、Windows 集成或发布验证已执行，并记录结果。
3. 已知错误和未覆盖环境写入 `findings.md` 与 `progress.md`。
4. 规格、实现和支持矩阵没有相互矛盾的承诺。
5. 未把条件性互操作能力描述成普遍保证。

## 9. 当前审批点

在用户批准中文规格和本实现计划前，保持“仅规划”状态。批准后先执行 M0；M0 的部署 Spike 结果将决定全用户安装 broker 的具体 API 和最低 Windows 版本。
