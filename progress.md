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
