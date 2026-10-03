# 管理员运行时与包管理重构发现

## 当前实现事实

- 搜索 DTO 没有图标字段；UI 只能用标题首字母生成占位图标。
- `normalize_product` 只读取第一个本地化属性和第一个 SKU availability，稀疏搜索结果会丢失发布者、PFN 和包格式。
- 详情命令已经解析 FE3 包图，但返回值仍主要来自 DCAT 产品对象，没有用包图补齐包身份、发布者和格式。
- 详情页的兼容架构预览与后台任务的完整 `select_packages` 不是同一条决策路径，界面可显示存在兼容架构，任务仍可能返回 `no_compatible_package`。
- 用户架构设置当前作为硬过滤条件传给选择器；其产品语义应改为偏好顺序，主机兼容性才是硬门。
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
