# 管理员运行时与包管理重构进度

## 2026-10-03 设计阶段

- 确认工作区为 `E:\Projects\YetAnotherMicrosoftStore`，分支 `main` 比 `origin/main` 领先 1 个提交。
- 确认用户已有 `README.md` 暂存改动；本阶段不覆盖、不取消暂存，也不提交。
- 检查目录、FE3、适用性、部署协调器、Broker、清单、更新扫描、Tauri DTO、React 视图和发布脚本。
- 确认采用“重写权限/部署/清单纵切面，保留已验证协议与校验能力”的路线。
- 确认主程序强制管理员运行，删除 Broker，保留双安装范围，并让更新匹配既有安装范围。
- 确认程序未发布，可删除 `AwaitingElevation` 和 `requiresElevation`，不建立旧任务兼容层。
- 将旧 `docs/superpowers/`、`task_plan.md`、`findings.md`、`progress.md` 归档到 `docs/archive/2026-10-03-pre-admin-runtime-redesign/`。
- 首次并行 `git mv` 因 Windows 目录重命名和 Git index 锁竞争失败；确认文件未移动、锁已释放后，改为顺序移动并成功完成归档。
- 已创建全新的当前任务计划与发现记录。
- 已编写正式设计规格，并完成占位符、内部一致性、范围和可验收性自检。
- 自检将默认架构顺序修正为现有 Windows 10 能力模型，并明确删除 `elevation_cancelled`，不保留条件性分支。
- 用户已批准路线 1 和正式设计规格，并要求在当前会话完成重构、验证、暂存和 commit 命令交付。
- 已编写文件级实施计划，覆盖状态/API 契约、管理员 manifest、直接部署、Broker 删除、统一选包、目录补全、机器清单、结构化更新扫描、React UI、文档和分层验收。
- 计划自检补充了 Broker 删除前置条件：先迁移仍被生产代码使用的包类型，再删除协议模块。
- 用户进一步确认程序未发布，不保留历史数据库兼容；计划已改为把 4 个旧 migration 和本次 schema 修改合并为唯一 `0001_initial.sql`，版本重置为 1，并直接删除 `requires_elevation` 列。
- 已核对 migration registry 和测试引用：实现时需同步 `persistence.rs` 及 `m3_persistence`、`m5_identity`、`m6_event_store` 等直接 include 旧 SQL 的测试。
- 已取消 `git mv` 自动产生的本轮暂存状态；原先用户暂存的 `README.md` 保持不变。
- 尚未修改产品代码、依赖、迁移或构建脚本。

## 2026-10-03 实施阶段

- 合并最终 schema 为 `src-tauri/migrations/0001_initial.sql`，删除旧 migration 和 elevation 字段。
- 嵌入主 EXE `requireAdministrator` manifest，删除 Broker crate、协议、launcher、sidecar、复制脚本和任务级提升状态。
- 将 CurrentUser/AllUsers 都路由到提升主进程的原生部署路径。
- 增加统一 SelectionPreview、兼容架构回退和封闭拒绝原因。
- 扩展目录 DTO、跨 SKU 格式汇总、搜索有界补全和精确图标白名单。
- 扩展机器范围清单名称字段、稳定合并、partial 语义和更新范围推导。
- 把更新扫描改为结构化结果，并加入 PFN 回查和 identity/publisher/PFN 核验。
- 同步 React 搜索、详情、已安装、更新反馈与窄屏来源 badge；删除二次 UAC 文案。
- 同步 README、support matrix、release、diagnostics，并为历史归档增加只读说明。

## 最终验证状态

- 归档完整性：6 份旧 superpowers 文档和 3 份根规划记录均已核对，行数合计 2012，归档正文未修改。
- 文档占位符检查：通过；没有未决占位标记。
- Rust 格式检查通过。
- Rust `cargo test --lib --tests`：167 passed / 8 ignored；忽略项均要求显式 E2/E3 环境。
- `cargo clippy --all-targets -- -D warnings` 通过。
- Vitest 11/11、Playwright 2/2、TypeScript/Vite production build 通过。
- `pnpm exec tauri build --debug --no-bundle` 通过；`mt.exe` 提取的主 EXE manifest 为 `requireAdministrator`。
- 首次实际启动发现 `TaskDialogIndirect` 入口缺失；核对 Tauri 2.7.1 默认 manifest 后确认自定义 manifest 遗漏 Common Controls v6。添加回归测试和依赖声明、重建后，PE 资源同时包含 Common Controls 6.0.0.0 与 `requireAdministrator`。
- 修复后的 debug EXE 实际启动并保持 8 秒响应，进程 `Responding=True`；非提升验证 shell 无权结束提升进程，需从应用窗口正常关闭。
- 生产路径静态检索无 Broker/elevation 旧契约；migration 目录只有 `0001_initial.sql`；`git diff --check` 通过（仅行尾转换提示）。
- `cargo test --all-targets` 因 Windows 拒绝直接启动要求提升的 bin test harness 返回 740；使用 `--lib --tests` 覆盖全部实际 Rust 测试，binary 编译由 Clippy/Tauri build 覆盖。
- 真实 Store/CDN、UAC 取消、CurrentUser/AllUsers 可逆部署和 ARM64 验收未执行，不声明 E2/E3 通过。

## 2026-10-03 回归修复

- 收到四项现场反馈：下载进度恒为 0%、任何代理模式下载失败、安装状态卡住且未观察到文件句柄安装操作、有更新应用需要置顶。
- 确认工作树初始干净，分支 `main` 与 `origin/main` 一致，HEAD 为 `64d38b5`。
- 读取当前规划、设计规格和历史 M4/M5 约束；当前只完成 E0 调查准备，未改生产代码。
- 两次并行只读命令因 PowerShell/Git Bash 嵌套引号解析失败；未产生仓库副作用，后续改用直接包装格式。
- 读取本机只含安全字段的任务投影：代理失败记录为 `download_failed`；直连后 911,998,170 字节包已进入 verified cache，但任务仍记录 0/NULL 下载进度。
- 确认卡住更新目标为正在运行的 `OpenAI.Codex`；不采用 `ForceApplicationShutdown`，改为包占用失败、用户确认后精确终止同 PFN 进程并重试。
- 查阅本机 Windows 0.62.2 bindings 与 Microsoft 文档，确认 `AddPackageAsync` 原生提供 `DeploymentProgress`，`GetPackageFamilyName` 需要查询权限，`TerminateProcess` 需要终止权限且调用后应等待退出。
- 用户批准实现推荐的包占用行为，并追加“结束相关进程”按钮需求。
- 完成下载逐块进度、worker 完成竞态修复及聚合进度持久化。
- 完成下载错误细分与 HTTP 状态详情；4xx、5xx 和非 HTTP 传输失败均有对应错误码或说明。
- 完成 Windows 部署进度事件、任务投影、队列安装进度条和完成队列排空。
- 完成包占用前置检查、可信任务级进程终止命令、确认按钮及终止后的自动重试；终止前二次校验 PFN。
- 完成有更新应用稳定置顶，并补齐大小写不敏感 PFN 匹配。
- 代理选项区分 HTTP/CONNECT 与 HTTPS TLS 代理；受控 HTTP 代理下载测试通过，真实 Store CDN 经本机代理仍返回上游 502/连接关闭，现会显示具体错误而不是笼统“网络问题”。
- 保持单一 `0001_initial.sql` 和 schema version 1；未新增旧开发数据库升级链。

## 回归修复验证

- Rust `cargo test --lib --tests`：181 passed / 9 ignored；忽略项均需显式 E2/E3 环境。
- Rust `cargo clippy --all-targets -- -D warnings`：通过。
- Vitest：17/17 通过；TypeScript/Vite production build：通过。
- Playwright：2/2 通过，覆盖桌面键盘/axe 与 360 px 窄屏。
- 受控代理、下载进度、部署进度、错误状态码、列表排序和终止命令均有自动化回归。
- 尚未执行真实签名包的可逆 Windows 安装/更新，也未强制终止当前正在运行的 OpenAI.Codex；不声明 E2/E3 安装验收通过。

## 2026-10-04 方案 B 实施进度

- 完成详情页后端本地动作推导、可信已安装应用启动命令，以及 Install/Update/Open 前端分派。
- 完成队列 sequence-monotonic 合并、先订阅后列表再 replay 的初始化顺序，以及验证/下载/部署分阶段进度呈现。
- 完成 Windows 部署 HRESULT 包占用分类、进程名/PID 描述符、PFN 二次校验和残留进程结果。
- 完成 `awaiting_process_exit`、`retry_deployment`、部署 checkpoint 单表持久化、重启保持和终态清理。
- 完成 checkpoint 重建及 worker 直接重试；自动化证明该路径不会调用 resolver/downloader，缓存缺失时关闭式失败。
- 完成自动打开且可重新打开的占用进程弹窗；部分终止失败会逐项显示残留进程名和 PID。
- 最终 E1：Rust `cargo test --lib --tests` 为 205 passed / 9 ignored；`cargo fmt --all -- --check` 与严格 Clippy 通过。
- 最终前端验证：Vitest 24/24、Playwright 3/3、TypeScript/Vite production build 通过；Playwright 覆盖桌面键盘/axe、360px 工作台与 360px 占用弹窗残留进程呈现。
- `pnpm exec tauri build --debug --no-bundle` 通过，生成 debug EXE；这是构建证据，不是 live 部署或启动验收。
- 尚未执行受控 Windows 的真实签名包部署占用、强制终止、重启后重试或 AppListEntry 启动，不声明 E2 通过。
