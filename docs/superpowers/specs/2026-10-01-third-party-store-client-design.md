# 第三方 Microsoft Store 客户端设计规格

> 状态：架构已批准；M0 原生部署验收与 M1 离线协议适配已完成。M2 及后续能力仍需按里程碑验收和条件门控，实时 Store/FE3 与跨渠道互操作不因本地 fixture 或构建结果而视为完成。

## 关键边界

- 第一阶段只安装和更新普通 Microsoft Store MSIX/AppX 包。
- 第一阶段不支持 EXE、MSI 和 Xbox 包。
- MSIXVC 是目标能力，但不作为普通 MSIX 处理。第一阶段只识别并标记 MSIXVC 记录为延后；下载、安装和更新必须经过独立的 Xbox/MSIXVC 部署验证。
- “不依赖系统更新服务”指不调用 winget、PowerShell、Microsoft Store 界面、Windows Update 扫描或 Store 更新队列。受支持的本地 MSIX 注册仍使用 Windows 原生包部署栈。
- 安装来源只用于观测。客户端不得创建私有包身份，也不得阻止官方 Store 识别同一个 Microsoft 签名包。
- 付费、Flight 和地区受限产品不得通过绕过许可来安装。身份认证与授权处理属于另行批准的能力。

## 产品目标与成功标准

产品是一个 Windows 桌面客户端，允许用户搜索 Microsoft Store 目录、检查包适用性、从 Microsoft 交付端点下载包，并在本地安装或更新受支持的包。

第一个可用里程碑是在干净的受支持 Windows 环境中，以全用户范围成功安装一个免费且可公开下载的 MSIX/AppX 应用；过程中不创建 winget 或 PowerShell 子进程。安装后的同一包必须能通过已安装包清单发现，并通过同一条直连解析路径更新。

互操作里程碑是来源无关的包关联：官方 Store 安装的应用应出现在本客户端的清单和更新扫描中；本客户端安装的应用在包身份、签名、市场/渠道和用户授权兼容时，应仍具备由官方 Store 更新的资格。

## 技术选型

| 层 | 选择 | 理由 |
|---|---|---|
| 桌面壳 | Tauri 2 | 使用 Rust 命令和 WebView UI 的轻量原生壳 |
| 前端 | Vite + React + TypeScript | 本地开发速度快，组件模型有类型约束 |
| 组件 | shadcn/ui + Radix primitives | 提供可访问的对话框、表单、表格、标签页、菜单和进度状态 |
| 样式 | 使用 CSS 变量的 Tailwind CSS | 支持主题、响应式布局且无运行时样式依赖 |
| 后端 | Rust、Tokio、windows crate | 异步网络/任务控制以及原生 WinRT/Win32 访问 |
| Store 协议 | 围绕固定版本 storelib_rs 的项目适配器 | 复用 DCAT/FE3 解析，避免供应商类型泄漏到业务层 |
| 存储 | 嵌入式 SQLite 或等价单文件存储 | 保存任务状态、目录缓存元数据、包缓存元数据和设置，不依赖后台服务 |
| 分发 | Tauri bundler + NSIS | 符合非 Store 分发路径要求 |

Tauri 的 Windows 运行时使用 WebView2；这是 UI 运行时前置条件，不是包获取或更新服务依赖。参见 [Tauri 架构](https://v2.tauri.app/concept/architecture/) 和 [Windows 前置条件](https://v2.tauri.app/start/prerequisites/)。

## 跨渠道互操作

### 规范化身份

包关联以 Windows 包身份为主键，而不是依赖本客户端的下载历史。规范化关联记录包含：

- Package Identity 名称和发布者。
- Package Family Name。
- Package Full Name 和版本。
- 架构和资源 ID。
- 可用时记录 Application User Model ID。
- 可解析时记录 Microsoft Store Product ID、Content ID 以及包/分类标识。
- 最近一次解析使用的市场、语言和渠道。
- 观测到的安装来源：官方 Store、本客户端或未知。

安装来源只作为诊断和 UI 元数据，绝不能改变包身份、发布者、签名或包族。

Microsoft 将 Package Identity 和 Package Family Name 定义为区分包及其版本的稳定标识。参见 [Package Identity 概览](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/package-identity-overview)。

### 检测官方 Store 安装

已安装包清单直接扫描 Windows 包状态，包括官方 Store 安装的包。对每个包族执行：

1. 读取已安装包的身份、版本、架构和资源包。
2. 将 Package Family Name 或其他受支持的备用标识解析为 Store Product ID。
3. 获取新鲜的 Display Catalog/FE3 元数据。
4. 将已安装包图与适用的 Microsoft 包图进行比较。
5. 将包标记为已是最新、可更新、高于目录、在所选市场不可用或无法关联。

这样第三方客户端无需依赖自己的任务数据库是否记录过最初安装。

### 保留官方 Store 更新能力

客户端必须：

- 只安装原始 Microsoft 签名的 Store 包及其官方依赖。
- 保留包清单身份和发布者。
- 不重新打包、重签名、打补丁或重命名 Store 中的包。
- 避免安装不同包族或发布者的并行包。
- 让已安装版本和资源图保持在 Windows 包清单可见范围内。
- 对付费、受限或 Flight 产品要求用户具备正常 Store 授权。

在这些条件满足时，官方 Store 可能更新本客户端安装的包。Microsoft 记录了独立 MSIX 分发后仍可由用户从 Microsoft Store 手动更新同一应用的案例，并建议跨渠道保持一致的身份和更新机制。这是兼容性目标，不是对所有产品或授权情况的普遍保证。参见 [Windows App 更新行为](https://learn.microsoft.com/en-us/windows-app/configure-updates-windows) 和 [Windows 应用最佳实践](https://learn.microsoft.com/en-us/windows/apps/get-started/best-practices)。

当 Store 接管更新不可行时，客户端必须给出明确原因，包括缺少授权、市场/渠道不匹配、包身份不匹配、策略限制或 MSIXVC/Xbox 能力边界。

### 版本冲突策略

- 不得因为某个渠道报告了更旧版本而降级已安装包。
- 如果已安装版本高于直连目录结果，标记为“高于目录”，不得提供降级。
- 如果官方 Store 更新了包，下次清单扫描应记录新版本并清理第三方更新任务。
- 如果直连路径发现更新的兼容版本，客户端可以提供更新，但不得改变官方 Store 自己的队列。
- 两个渠道不得互相替换队列状态，也不得把本地部署成功宣称为 Store 已接受更新。

## 系统架构

~~~mermaid
flowchart LR
  UI[Vite React UI] --> CMD[Tauri commands/events]
  CMD --> ORCH[Job orchestrator]
  ORCH --> CAT[Catalog provider]
  CAT --> DCAT[Display Catalog]
  ORCH --> FE3[FE3 package resolver]
  FE3 --> MS[Microsoft delivery endpoints]
  ORCH --> SEL[Applicability selector]
  SEL --> DL[Resumable downloader]
  DL --> VERIFY[Hash/signature verifier]
  VERIFY --> DEPLOY[Native package deployer]
  DEPLOY --> PM[Windows PackageManager]
  ORCH --> INV[Installed package inventory]
  INV --> DIFF[Version/update diff]
  DIFF --> UI
~~~

### Rust 模块职责边界

- catalog：搜索、Product ID/PFN 查询、产品元数据和本地化目录数据。
- resolver：Display Catalog 到 FE3 的解析、包记录、前置条件和临时 URL。
- applicability：架构、语言、市场、OS 构建版本、包格式和依赖选择。
- download：HTTP Range 请求、重试、取消、校验和流式计算、缓存落盘和代理策略。
- verification：期望大小/哈希、包签名预检和防降级检查。
- inventory：当前用户和机器范围的包枚举、包身份映射和已安装版本状态。
- deployment：普通包安装/更新以及特权全用户 broker/预配路径。
- jobs：持久化任务状态、排队、按包加锁、进度事件和重启恢复。
- settings：区域、市场、架构偏好、代理策略、缓存策略、并发数和日志脱敏。
- tauri_api：唯一面向前端的命令/事件层，将内部错误转换为稳定 DTO。

前端不得依赖 storelib_rs 类型、FE3 XML 结构、原始下载 URL 或 WinRT 错误对象。

## Store 获取与包解析

### 搜索与详情

目录提供器接收搜索字符串、市场、语言和设备族，并返回包含以下字段的规范化结果：

- Product ID 和备用标识。
- 显示名称、发布者、图标和本地化描述。
- 可用时提供包族名称。
- 产品类型以及许可/可用性提示。
- 支持的架构、语言和包格式。

搜索结果只包含元数据。安装前必须重新查询产品并解析新的包记录，避免复用过期下载 URL。

### 直连包解析

解析器先使用 Display Catalog 结果获取包身份和履约数据，再通过 FE3 路径获取包实例、依赖边、包大小、可提供的哈希以及 Microsoft CDN 临时位置。

storelib_rs 必须被隔离在项目自有接口之后：

- 固定精确 crate 版本或 Git revision。
- 使用脱敏后的 JSON/XML fixture 保留适配器测试集。
- 将端点构造和协议选项集中在一个模块。
- 如果 crate 或端点发生变化，保留可替换实现路径。

StoreLib 本身是参考实现，不是稳定的平台 SDK。仓库已归档，见 [StoreDev/StoreLib](https://github.com/StoreDev/StoreLib)；Rust 移植版文档见 [storelib_rs](https://docs.rs/crate/storelib_rs/latest)。

### 适用性选择

选择过程必须是确定且可解释的：

1. 拒绝要求高于当前主机 Windows 构建版本的包。
2. 优先使用配置的架构，再选择兼容回退架构。
3. 优先使用用户语言和中性资源。
4. 包含必需的语言/资源包。
5. 包含尚未安装的必需框架包。
6. 当 bundle 包含适用架构时优先选择 bundle。
7. 拒绝当前能力矩阵之外的包格式。
8. 拒绝低于已安装版本的包，除非用户明确请求修复且策略允许。

对于需要提权的安装，UI 应在开始前展示选定的架构、市场、语言和依赖集合。

## 包格式支持矩阵

| 格式 | 第一阶段 | 行为 |
|---|---:|---|
| .msix / .appx | 是 | 下载、验证并安装/更新 |
| .msixbundle / .appxbundle | 是 | 选择适用架构/资源并安装/更新 |
| .eappx / .eappxbundle | 有条件 | 仅在部署 API 接受该包且授权有效时解析并下载 |
| .msixvc | 目标能力，延后 | 第一阶段只识别并解释不支持的能力；专门的 MSIXVC 部署验证通过后才允许下载/安装/更新 |
| .exe / .msi | 否 | 第一阶段绝不执行供应商安装器 |
| Xbox 包族 | 否 | 明确延后 |

Microsoft 将 MSIXVC 描述为“Microsoft Installer for Xbox Virtual Console”，并为流数据源记录了独立的 COM/API 接口。参见 [MSIXVC API 参考](https://github.com/MicrosoftDocs/win32/blob/docs/desktop-src/appxpkg/msixvc-api-reference.md)。因此该格式必须隔离处理，不能直接交给普通 PackageManager 调用。

## 下载、缓存与验证

### 下载行为

- 下载到配置缓存目录下按任务隔离的临时文件。
- 服务端支持时使用 HTTP Range 请求实现续传。
- 临时 URL 过期时重新解析包。
- 限制并行包下载，并串行部署共享同一包族的包。
- 持久化任务状态，使重启后可安全恢复。
- 绝不将原始包 URL 暴露给 WebView。

### 缓存策略

用户可以设置：

- 是否启用缓存。
- 缓存目录。
- 最大缓存大小。
- 保留时间。
- 保留已安装包载荷，或部署成功后删除。
- 将未完成任务与已验证包分开清理。

临时 URL 不是持久缓存键。持久键由规范化产品/包身份、版本、架构、语言和内容哈希组成。

### 验证

部署前必须：

- 验证 HTTPS 以及重定向主机白名单。
- 验证期望大小。
- 流式计算并校验 SHA-256。
- 检查包清单身份和版本。
- 拒绝发布者/包族不匹配。
- 由 Windows 部署执行最终的包签名和依赖验证。

验证失败必须保持已安装包不变，并保留脱敏后的诊断记录。

## 安装与更新

### 普通包部署

使用 windows crate 调用 Windows.Management.Deployment.PackageManager。文档中的 AddPackageAsync 重载接受本地包 URI 和依赖包 URI，是通过 PowerShell 调用 Add-AppxPackage 的原生替代方案。参见 [AddPackageAsync](https://learn.microsoft.com/en-us/uwp/api/windows.management.deployment.packagemanager.addpackageasync?view=winrt-28000)。

### 全用户部署

全用户安装是特权操作：

- 普通 Tauri 进程保持非提权。
- 只有在用户明确确认后才调用小型签名原生 broker。
- broker 使用 WinRT 部署/预配 API，绝不使用 PowerShell、winget 或 shell 命令。
- 只在部署窗口请求 UAC。
- broker 通过受限 IPC 通道接收白名单内的本地包清单和依赖列表。
- broker 返回结构化部署状态以及 HRESULT/错误文本。
- broker 绝不接受任意可执行路径或命令行。

第一个实现里程碑必须验证选定 Windows 版本和包格式是否允许 broker 执行目标全用户预配流程。如果包无法进行机器范围预配，UI 必须报告该限制，不得静默回退到当前用户安装。

### 更新

更新检测将已安装包身份和版本与新鲜的目录/FE3 解析结果比较。更新路径与安装相同：

1. 扫描已安装包。
2. 关联包族/产品标识。
3. 解析新的包图。
4. 选择严格更新且兼容的包图。
5. 下载并验证所有发生变化的包。
6. 在受支持时作为一次 Windows 包操作部署。
7. 重新扫描清单并记录最终版本。

本设计不集成后台 Windows Update 或 Store 更新队列。

## 代理配置

设置模型有四种明确模式：

- disabled：直连。
- system：读取用户/系统代理策略，包括所选 HTTP 栈支持的自动检测/PAC 行为。
- http / https：显式代理 URL 和可选凭据策略。
- socks5：显式 SOCKS5 代理 URL 和可选凭据策略。

实现不得假设通用 Rust HTTP 客户端会自动复现所有 Windows 代理行为。Microsoft 记录了 WinINet 与 WinHTTP 的重要差异，包括桌面应用如何继承 Internet 选项以及自动代理如何配置。参见 [WinINet 与 WinHTTP](https://learn.microsoft.com/windows/win32/wininet/wininet-vs-winhttp) 和 [在 WinHTTP 中使用 WinINet 代理设置](https://learn.microsoft.com/en-us/windows/win32/winhttp/setting-wininet-proxy-configurations-in-winhttp)。

因此网络层暴露 ProxyProvider 边界。system 模式可使用感知 Windows 的解析器；自定义 HTTP(S)/SOCKS5 模式使用显式连接器配置。只有用户主动选择保存时才存储凭据，并且不得写入普通日志。

## Tauri 命令与事件契约

第一版稳定命令集为：

- search_apps
- get_app_details
- resolve_app_packages
- scan_installed_packages
- scan_updates
- start_install
- start_update
- cancel_job
- pause_job
- resume_job
- get_job
- get_settings
- update_settings
- clear_cache

事件按任务作用域划分：

- job-created
- job-stage-changed
- job-progress
- job-warning
- job-awaiting-elevation
- job-completed
- job-failed
- job-cancelled

DTO 包含 job_id、product_id、package_family_name、stage、bytes_done、bytes_total、version、architecture、language、requires_elevation 和面向用户安全的错误码等稳定字段。原始 URL、令牌和完整服务器响应只保留在 Rust 诊断信息中。

## 前端设计

### 信息架构

- 搜索：搜索框、最近搜索、筛选标签和结果网格/列表。
- 应用详情：本地化元数据、发布者、支持架构、包格式、所选市场/语言以及安装/更新操作。
- 队列：活动中、已暂停、等待提权、已完成和失败任务。
- 已安装：已安装包清单、安装来源标记和更新扫描。
- 设置：市场/语言、架构偏好、代理、缓存、并发数、主题和诊断。

### 组件计划

使用 shadcn/ui 原语：

- 使用 Input、Command、Button、Badge 实现搜索和筛选。
- 使用 Card、Table、Tabs、Skeleton 实现目录和清单。
- 使用 Dialog 或 AlertDialog 实现安装确认、提权说明和破坏性缓存操作。
- 使用 Progress、Alert、Toast 展示任务状态。
- 使用 Select、RadioGroup、Switch，以及 React Hook Form + Zod 的 Form 实现设置表单。

### 视觉系统

- 使用 CSS 变量主题，支持浅色、深色和跟随系统。
- 以中性的 zinc/slate 为基础色，用一种强调色表示安装/更新操作。
- 为成功、警告、破坏性和等待状态使用语义颜色。
- 统一间距和圆角 token；组件逻辑中不得硬编码原始颜色名称。
- 队列行使用紧凑密度，但指针目标保持至少 44px。
- 使用响应式、移动优先的 Tailwind 类，确保窄 Tauri 窗口仍可用。

### 可访问性

- 使用语义化 nav、main、section、form、table 和 button 元素。
- 显示 focus-visible 指示器。
- 对话框使用 Radix 焦点陷阱。
- 为命令搜索、筛选、标签页和队列操作提供键盘导航。
- 进度和完成状态使用 aria-live="polite"；仅对阻塞错误使用 assertive。
- 代理/缓存校验使用 aria-invalid 和字段说明。
- 满足 WCAG AA 对比度，支持减少动画并保持合理的 Tab 顺序。

UI 样式遵循选定的 ui-styling 指导：组件组合、CSS 变量主题、Radix 可访问性原语和移动优先 Tailwind 断点。

## 错误与恢复模型

错误统一为以下代码：

- catalog_not_found
- catalog_unavailable
- license_required
- market_unavailable
- no_compatible_package
- dependency_unresolved
- download_failed
- download_url_expired
- hash_mismatch
- signature_invalid
- elevation_cancelled
- deployment_denied
- deployment_failed
- package_in_use
- store_entitlement_missing
- store_channel_unavailable
- source_identity_mismatch
- version_ahead_of_catalog
- msixvc_capability_unavailable
- unsupported_package_type

每个错误都有稳定代码、本地化用户消息、安全诊断详情和重试策略。未经重新解析，重试不得重复无效包选择或哈希验证失败的操作。

## 测试策略

### 单元与 fixture 测试

- Display Catalog JSON 解析。
- FE3 SOAP/XML 解析。
- 包依赖图解析。
- 架构/区域/市场选择。
- 版本比较和防降级逻辑。
- 代理模式校验。
- 缓存淘汰和重启恢复。
- URL 白名单和重定向处理。

### Windows 集成测试

- 在受支持 Windows 版本上执行当前用户安装。
- 通过带 UAC 的原生 broker 执行全用户预配。
- x64、ARM64 和 x86 选择。
- 多语言资源包选择。
- 应用运行期间更新。
- 依赖已安装与缺失两种情况。
- 磁盘空间不足、代理失败、下载取消和 URL 过期。
- 普通 MSIX/AppX bundle 验证。
- 在独立部署 spike 通过前，仅测试 MSIXVC 识别和明确的能力门控失败。
- 第三方清单检测到官方 Store 安装。
- 在测试产品、身份和授权允许时，第三方安装的 Microsoft 包可由官方 Store 手动更新。
- 任一渠道版本领先时的冲突测试，且不得降级。

### 发布验证

- 在干净 Windows 机器上测试 NSIS 安装器。
- WebView2 前置条件行为。
- 不创建 PowerShell/winget 子进程。
- 网络主机白名单审计。
- 日志中不出现令牌、凭据或原始 URL。
- 键盘独立操作和屏幕阅读器冒烟测试。

## 发布阶段

1. 协议/部署 Spike：单个免费公开 MSIX/AppX，完成新鲜 DCAT/FE3 解析、验证和全用户部署。
2. 目录 UI：搜索、本地化详情、市场/语言/架构选择器。
3. 下载与缓存：可续传任务、代理模式、保留策略和恢复。
4. 部署/更新：清单、版本差异、特权 broker 和更新队列。
5. MSIXVC 能力：针对独立 API/服务要求进行研究和验证；本阶段完成前不作 Xbox 支持声明。
6. 发布加固：NSIS、支持矩阵、诊断和回归测试集。

## 下一次评审待决策事项

- 全用户预配的最低 Windows 构建版本。
- storelib_rs 固定 crates.io 版本，还是经过审计的 Git revision。
- 首次发布的 system 代理模式是否支持 PAC/WPAD，还是只支持静态 Windows 代理设置。
- 全用户安装成功后是否默认保留缓存包。
- 付费产品和 Microsoft 账户认证是后续项目，还是永久不在范围内。
- “官方 Store 更新第三方安装”是接受条件性兼容保证，还是必须建立逐产品认证矩阵。

## 审批门槛

本规格和配套实现计划已准备好一并评审。用户批准或提出修改前，不得开始实现或安装依赖；批准后先执行实现计划中的 M0 基线与部署 Spike。
