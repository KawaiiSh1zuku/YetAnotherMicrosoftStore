# 管理员运行时与包管理重构发现

## 重构前基线事实

- 搜索 DTO 没有图标字段；UI 只能用标题首字母生成占位图标。
- `normalize_product` 只读取第一个本地化属性和第一个 SKU availability，稀疏搜索结果会丢失发布者、PFN 和包格式。
- 详情命令已经解析 FE3 包图，但返回值仍主要来自 DCAT 产品对象，没有用包图补齐包身份、发布者和格式。
- 详情页的兼容架构预览与后台任务的完整 `select_packages` 不是同一条决策路径，界面可显示存在兼容架构，任务仍可能返回 `no_compatible_package`。
- 用户架构设置在核心选择器中已经作为排序权重；实际分裂点是详情页绕过核心选择器，只统计兼容主包架构。重构后详情与 worker 均走统一入口。
- 更新扫描固定读取 `CurrentUser`，且只处理数据库中已有可信 PFN/Product ID 关联的主包；未关联包被静默跳过。
- 已安装页固定调用 `scan_installed_packages("current_user")`，因此不可能显示其他用户或预配包。
- 已安装 DTO 只有 identity name，没有 Windows `Package.DisplayName`；UI 因而把包身份名当作应用名。
- 来源徽章允许在窄表格列中逐字折行，造成截图中的纵向溢出。

## 已确认的产品语义

- 主程序永久管理员运行，拒绝 UAC 即不启动。
- 当前用户部署仍表示运行中的管理员账户，不自动提升为全用户部署。
- 机器清单记录只要包含其他用户或未来用户预配状态，更新范围就选择 `AllUsers`。
- 仅当前管理员账户安装的记录选择 `CurrentUser`。
- 同一 PFN 同时具有当前用户与机器范围状态时选择 `AllUsers`，避免版本分裂。
- 程序未发布，可删除 `AwaitingElevation`、`requiresElevation` 及相关兼容分支。

## 边界与保留能力

- 保留 `storelib_rs` adapter 隔离，不让第三方模型越过项目 DTO 边界。
- 保留 HTTPS/重定向白名单、Range/ETag 续传、哈希、manifest identity、WinTrust、缓存 containment 和重解析点防护。
- 删除 Broker 不等于删除部署前验证；验证后的本地包图仍是原生部署唯一输入。
- 旧 M0/M4/M6 测试和真实回环记录已归档，只描述旧架构的历史证据。

## 文档选型

- 本任务是通用架构设计，`docs-generator` 的安全报告 flavor 不适用。
- 正式规格采用渐进披露：先列破坏性变化和成功标准，再给架构、契约、失败语义与验收矩阵。

## 实施计划自检结论

- Broker 协议中仍被验证、deployment plan 和 orchestrator 使用的 `PackageIdentity`/`PackageFileRequest` 必须先迁移到无 IPC 语义的领域模块，不能随 Broker 目录直接删除。
- 仓库现有 4 个 migration 分别创建 M2 schema、追加适用性字段、包关联和任务事件；运行时 `CURRENT_SCHEMA_VERSION` 为 4，多组测试直接 include 旧文件。
- 程序尚未发布，因此不需要承担旧数据库升级兼容。最终实现应把现有最终 schema 与本次字段调整合并为唯一 `0001_initial.sql`，版本重置为 1，并从 schema 本身删除 `requires_elevation`。
- 合并不能机械拼接旧 SQL：应直接创建最终列、索引、外键和 CHECK 约束，删除 v1→v4 升级/旧行兼容测试，改为验证新库、重复打开幂等和 migration 事务回滚。
- `CatalogProvider` 已支持 `PackageFamilyName` lookup，可复用为未关联已安装包的 PFN 查询入口，不需要猜测 Product ID 或创造旁路协议。
- `scan_all_users` 已具备 FindPackages/FindUsers/FindProvisionedPackages 基础能力，重构应直接复用并补齐 DisplayName、局部 warning 与稳定去重，而不是另写系统扫描器。
- README 已有用户暂存内容；最终文档同步必须基于当前暂存版本合并并分别审阅 staged/unstaged diff。

## 重构后实现事实

- 主程序资源 manifest 为 `requireAdministrator`；生产源码、迁移、前端契约和 Tauri 配置中已无 Broker、`AwaitingElevation`、`requiresElevation` 或 `elevation_cancelled`。
- 唯一初始 migration 直接创建最终 schema version 1；旧开发数据库不提供升级兼容。
- `SelectionPreview` 由 `select_packages` 的结果投影，不包含 URL；worker 和详情共享同一选择算法，架构设置只影响排序。
- 搜索最多补全 20 项、最多 4 并发；图标仅接受 `https://store-images.s-microsoft.com`。
- 机器清单 DTO 包含 appName、packageName、PFN 和 publisher，并按 PFN/version/architecture/resource ID 稳定合并。
- 更新扫描返回计数、候选、跳过原因和 complete；候选冻结由安装事实推导的 deployment scope。
- `cargo test --all-targets` 会显式运行带提升 manifest 的零测试 binary harness，并在非提升 runner 返回 Windows 740；最终测试门改为 `cargo test --lib --tests`，binary 由严格 Clippy 和 Tauri build 覆盖。
- 自定义 elevation manifest 会完全替换 Tauri 默认 manifest；若不保留 `Microsoft.Windows.Common-Controls` v6 依赖，运行库静态导入的 `TaskDialogIndirect` 会在进程装载时失败。当前 manifest 已合并该依赖并由 release 测试固定。

## 2026-10-03 回归修复初始事实

- 当前分支为干净的 `main`，HEAD `64d38b5`，已包含管理员运行时重构及其后续更新扫描、语言偏好和 devUrl 修复。
- 旧 M4 约束仍要求代理模式显式区分：直连、WinHTTP 系统代理、自定义 HTTP(S)、SOCKS5；不得用 `reqwest` 猜测 WinINet/PAC 行为。
- 安装完成必须用真实 Windows 部署调用及库存后置条件证明，不能由入队或前端状态字符串推断。
- 尚未确认本轮四个症状是否共享根因；在完成事件与调用链追踪前不修改生产代码。

## 已确认根因与现场证据

- `DownloadManager::perform_download` 已按响应 chunk 写入文件，但没有进度回调；`ManagerDownloadPort` 只在整个 artifact 完成后发送一次进度。
- `run_download` 的 `tokio::select!` 可先观察下载 future 完成并直接返回，未保证排空同一时刻已发送的最终进度；本机成功下载的 911,998,170 字节包因此仍持久化为 `bytes_done=0, bytes_total=NULL`。
- 本机任务历史显示三个代理模式下载尝试都在进入 `downloading` 的同秒以封闭 `download_failed` 失败；切回 `disabled` 后同一目标下载并验证成功，说明失败位于代理传输边界而非目录解析或包选择。
- 本机静态系统代理为 `127.0.0.1:7890`，HTTP 与 SOCKS5 受控探测均能到达普通 HTTPS 站点；`HTTPS` 代理模式会对该明文 mixed-port 发起 TLS，按预期失败，UI 需要明确代理服务器传输协议。
- 当前卡住任务目标是 `OpenAI.Codex` 26.930.3930.0；本机 26.924.2738.0 的主程序和沙箱服务仍在运行。当前 `DeploymentOptions::None` 没有占用前置检查，任务进入 `deploying` 后可等待包释放。
- Windows `AddPackageAsync` 返回带 `DeploymentProgress` 的异步操作，现有实现直接 `.join()`，丢弃了系统提供的百分比。
- 终止相关进程采用 Win32 进程枚举 + `GetPackageFamilyName` 精确 PFN 匹配 + `TerminateProcess`；后端由可信 `job_id` 反查 PFN，排除自身 PID，不接受前端任意 PID/PFN。

## 回归修复实现事实

- `DownloadManager` 现在逐 chunk 回调已写入字节，worker 在下载 future 完成分支先排空进度队列，避免最终完成事件抢先覆盖持久化。
- 下载错误契约不再用单一 `download_failed` 覆盖所有网络失败：代理连接、407 认证、超时、连接/TLS、响应体中断、HTTP 状态、重定向拒绝和缓存 I/O 分别映射；401/403/404/410 仍只刷新一次 URL，但最终失败保留实际状态码。
- 受控本地 HTTP 代理测试能够下载白名单 Microsoft host URL；本机真实 Store CDN 经 127.0.0.1:7890 的 HTTP 路由返回 502，HTTPS 模式对明文 mixed-port 握手失败，SOCKS5 路由响应提前关闭。这是当前代理/CDN 路由的现场结果，不等同于客户端构造失败，也不采用静默直连回退。
- 部署调用为 `AddPackageAsync`、`StagePackageAsync` 与 `ProvisionPackageForAllUsersAsync` 安装进度处理器；进度作为单调事件持久化，完成前排空回调队列。
- 更新部署在进入 `deploying` 前检查同 PFN 活跃进程；发现占用时返回 `package_in_use`。强制结束命令只允许该失败任务触发，并在获取终止句柄后再次查询 PFN，避免 PID 复用导致误杀。
- 安装进度列和事件类型已合并进唯一初始 migration；没有偏离“未发布、不维护旧开发数据库升级链”的既定架构。
