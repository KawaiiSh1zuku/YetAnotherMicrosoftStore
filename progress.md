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

## 验证状态

- 归档完整性：6 份旧 superpowers 文档和 3 份根规划记录均已核对，行数合计 2012，归档正文未修改。
- 文档占位符检查：通过；没有未决占位标记。
- 实施计划状态：待用户审阅；按 `superpowers:writing-plans` 的强制门禁，批准前不修改生产代码。
- 产品测试与构建：本阶段未修改产品代码，不在设计审阅前运行完整质量门。
- 真实 Store/CDN/Windows 部署：本阶段未执行。
