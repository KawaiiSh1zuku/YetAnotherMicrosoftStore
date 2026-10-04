# 管理员运行时与包管理重构计划

## 目标

在不重写已经验证的 Store 协议、下载、缓存和包校验能力的前提下，重构权限、部署、清单、更新发现和应用元数据链路：主程序强制管理员运行，彻底删除一次性 Broker，同时保留“当前用户 / 所有用户”部署选择。

## 当前阶段

- [x] 核对仓库、现有实现、测试和历史文档
- [x] 确认产品决策与推荐路线
- [x] 归档旧 `docs/superpowers/`、`task_plan.md`、`findings.md`、`progress.md`
- [x] 编写正式设计规格
- [x] 用户审阅并批准正式设计规格
- [x] 基于获批规格编写实现计划并完成自检
- [x] 用户审阅实施计划
- [x] 在当前会话连续执行重构与 E0/E1 验证
- [x] 审阅并暂存范围内文件，向用户交付 commit 命令

## 已确认决策

- 主程序使用 `requireAdministrator`，拒绝 UAC 时不启动。
- 删除 Broker crate、sidecar、IPC、发布构建和运行时路径。
- 保留当前用户与所有用户两种安装范围。
- 更新优先匹配现有安装方式：其他用户或预配状态优先 `AllUsers`，否则使用 `CurrentUser`。
- 程序尚未发布，不保留旧任务对 `AwaitingElevation` 或 `requiresElevation` 的兼容。
- 程序尚未发布，不保留旧数据库升级链；现有 migration 与本次 schema 调整合并为唯一初始 migration，schema version 重置为 1。
- 旧规划和规格只归档，不作为新架构的当前规范。
- 历史测试证据不自动证明重构后的真实部署、下载或全用户更新已经通过。

## 本阶段交付物

- `docs/superpowers/specs/2026-10-03-admin-runtime-package-management-redesign.md`
- `docs/superpowers/plans/2026-10-03-admin-runtime-package-management-refactor-plan.md`
- 新的 `task_plan.md`、`findings.md`、`progress.md`
- `docs/archive/2026-10-03-pre-admin-runtime-redesign/`

## 实现结果

1. 主 EXE manifest 已静态提取并确认 `requireAdministrator`；Broker 源码、crate、IPC、sidecar 和构建路径已删除。
2. 4 个旧 migration 已合并为唯一 `0001_initial.sql`，schema version 为 1。
3. 搜索/详情、统一选包、机器范围清单、结构化更新扫描和 React 页面已经同步。
4. E1：Rust 167 passed / 8 ignored，Clippy、Vitest 11/11、Playwright 2/2、前端 build 和 Tauri debug no-bundle 通过。
5. E2/E3 未执行；历史 Broker 验收不继承到新架构。

## 已知风险

- 永久提权扩大主进程和 WebView 的权限面，必须保持严格 CSP、导航限制和输入边界。
- `CurrentUser` 指运行中的管理员账户；标准用户使用其他管理员凭据完成 UAC 时会进入该管理员的用户上下文。
- 机器范围更新可能遇到用户占用、预配与实际用户注册状态不一致，需要显式失败和重扫。
- 搜索结果逐项补齐详情与包图可能增加目录请求量，需要并发上限、结果上限和局部失败语义。

## 遇到的错误

| 错误 | 尝试次数 | 处理 |
|---|---:|---|
| 直接把 Windows 路径传给 Git Bash 导致路径解析失败 | 1 | 后续普通命令统一通过显式 Git Bash 包装器执行 |
| 并行执行多个 `git mv` 造成目录重命名失败和短暂 `index.lock` 竞争 | 1 | 确认没有文件移动后改为逐目录、逐批次顺序移动 |
| 进程检查正则转义错误 | 1 | 使用成功的 `git status` 和锁文件不存在作为恢复确认，不重复该正则 |
| 命令包装中的引号被 PowerShell 提前解析 | 2 | 后续使用直接的 Git Bash `-lc` 包装，不再程序化二次转义命令字符串 |

## 2026-10-03 下载、代理、安装与列表排序回归修复

### 目标

- 下载任务显示真实、单调递增的字节进度，不再始终为 0%。
- 直连、Windows 系统代理、自定义 HTTP(S) 与 SOCKS5 模式按各自契约工作，错误能定位到实际失败边界。
- 安装阶段只有在真实调用 Windows 部署 API 后才进入“正在安装”，并显示可观测进度或明确的不可量化等待状态。
- 已安装列表把存在更新的应用稳定排在前面，同时保留组内稳定排序。

### 阶段

- [x] Phase 1: 追踪下载事件、代理构造、部署调用和前端状态投影，确认根因
- [x] Phase 2: 为每个确认的根因编写并运行失败回归测试
- [x] Phase 3: 实施最小修复并完成针对性测试
- [x] Phase 4: 运行完整 E1 验证并核对文档偏差

### 范围与风险

- 影响：`src-tauri` 下载/运行时/部署事件链、Tauri DTO、React 已安装与队列视图。
- 可能破坏：断点续传、代理凭据边界、任务恢复、部署状态机、列表稳定性。
- 验证：Rust 单元/集成测试、Vitest、Playwright、前端构建、严格 Clippy；真实 Store/CDN 与 Windows 部署另行标记 E2/E3。

### 实现结果

- 下载层按响应块上报聚合字节进度，并在 future 完成前排空已排队的最终进度事件。
- 代理和下载失败使用封闭错误码；HTTP 4xx/5xx 均保留状态码，代理连接、认证、超时、连接、响应中断、重定向与本地 I/O 分别说明。
- Windows 部署进度写入事件流并投影到队列；更新安装前检查同 PFN 进程，占用时先失败提示用户。
- `package_in_use` 任务提供确认式“结束相关进程并重试”，后端只接受可信任务 ID，并在终止前再次精确校验 PFN。
- 已安装列表把有更新记录稳定置顶；同组保持原有顺序。
- 保持唯一 `0001_initial.sql` 与 schema version 1，未引入开发期升级链。
