# 管理员运行时与包管理重构实施计划

> 状态：已实施；E0/E1 完成，E2/E3 门保留
>
> 依据：`docs/superpowers/specs/2026-10-03-admin-runtime-package-management-redesign.md`
>
> 执行约束：使用 `superpowers:executing-plans` 与测试驱动开发；本轮不创建中间提交，完成后只暂存经审阅的范围内文件，并给出英文 Conventional Commit 命令。

## 交付目标

把当前“普通权限主程序 + 一次性提权 Broker”改成单一管理员主程序，并修复搜索元数据、兼容包选择、机器范围清单、更新扫描反馈和已安装页布局。保留已经验证的 Store adapter、下载、缓存、哈希、manifest identity、WinTrust 和部署后置清单边界。

## 决策与依赖

- 采用单一管理员进程：`src-tauri/build.rs` 为主 EXE 嵌入 `requireAdministrator`，不再保留普通权限运行模式。
- `CurrentUser` 指运行中的管理员账户；`AllUsers` 指机器范围 stage/provision/remove。NSIS 仍为 `currentUser` 安装器。
- 删除 Broker crate、IPC、sidecar、相关状态和发布脚本。Broker 中仍被主程序使用的包请求类型先迁移到无 IPC 语义的领域模块，再删除协议模块。
- 程序尚未发布，不承担旧数据库兼容：把现有 4 个 migration 和本次 schema 调整合并为唯一的 `0001_initial.sql`，`CURRENT_SCHEMA_VERSION` 重置为 `1`。最终 schema 不创建 `requires_elevation` 等已删除字段；旧开发数据库由开发者删除后重建。
- 搜索、详情、安装 worker 和更新扫描必须共享同一适用性选择入口。用户架构设置只影响排序，主机能力和格式支持仍为硬门。
- 所有新网络补全均有上限：搜索最多 20 项且补全最多 4 个并发；更新扫描使用独立的 1–64 并发设置（默认 16）。单项失败产生 `partial` 或封闭 `skipped` 原因，不取消整批。
- 不把 fixture、单元测试、构建或 PE 静态检查称为真实 Store/CDN/部署验收。

## 影响范围

可能破坏的模块：任务持久化和恢复、Tauri DTO、部署协调器、Windows 清单、目录 adapter、FE3 选择、更新扫描、React 三个主要页面、发布脚本和历史验收测试。

验证分四层：

1. E0：静态检索、DTO/manifest/sidecar 结构检查。
2. E1：Rust、Vitest、Playwright、TypeScript/Vite、Clippy 和 debug Tauri build。
3. E2：受控 Windows x64 的 UAC、CurrentUser/AllUsers 部署、机器清单和清理恢复。
4. E3：真实 Store/CDN 或跨渠道行为；只有实际执行并留存脱敏证据时才声明。

## Task 1：固定新状态、共享包类型和 API 契约

**文件：**

- 新建：`src-tauri/src/package.rs`
- 修改：`src-tauri/src/jobs.rs`
- 修改：`src-tauri/src/job_events.rs`
- 修改：`src-tauri/src/persistence.rs`
- 修改：`src-tauri/src/tauri_api.rs`
- 修改：`src-tauri/src/error.rs`
- 修改：`src-tauri/src/lib.rs`
- 修改：`src-tauri/src/deployment.rs`
- 修改：`src-tauri/src/deployment_plan.rs`
- 修改：`src-tauri/src/deployment_orchestrator.rs`
- 修改：`src-tauri/src/package_validation.rs`
- 新建：`src-tauri/migrations/0001_initial.sql`
- 删除：`src-tauri/migrations/0001_m2.sql`
- 删除：`src-tauri/migrations/0002_m3_applicability.sql`
- 删除：`src-tauri/migrations/0003_m5_identity.sql`
- 删除：`src-tauri/migrations/0004_m6_job_events.sql`
- 修改：`src-tauri/tests/m0_validation.rs`
- 修改：`src-tauri/tests/m2_domain.rs`
- 修改：`src-tauri/tests/m2_persistence.rs`
- 修改：`src-tauri/tests/m3_persistence.rs`
- 修改：`src-tauri/tests/m5_deployment_plan.rs`
- 修改：`src-tauri/tests/m5_orchestration.rs`
- 修改：`src-tauri/tests/m6_api.rs`
- 修改：`src-tauri/tests/m6_event_store.rs`

### 1.1 先写失败测试

- 删除测试 fixture 中的 `requires_elevation` 后，新增断言：序列化的 job、snapshot、event 和 Tauri DTO 不含 `requiresElevation`。
- 新增状态机测试：`Verifying -> Deploying` 合法，`AwaitingElevation` 不再可解析或恢复。
- 新增持久化测试：空数据库一次性创建最终 schema，schema version 为 1；重复打开不重复执行 DDL；migration 失败时 schema 和 `user_version` 原子回滚。
- 新增 schema 断言：包含现有产品、包、关联、任务、事件、命令、租约、缓存、设置、观察和诊断表及索引，但 `jobs` 不含 `requires_elevation`。
- 删除旧版本升级测试和直接 include 旧 migration 的 fixture；所有持久化测试从唯一初始 migration 或 `Persistence::open` 建库。
- 新增错误映射测试：公共错误码不再包含 `elevation_cancelled`。
- 先运行相关测试并确认失败原因来自旧字段/旧阶段，而不是测试拼写或 fixture 损坏。

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test m2_domain --test m2_persistence --test m3_persistence --test m6_api --test m6_event_store
```

### 1.2 迁移领域类型并删除旧状态

- 在 `package.rs` 定义原 `broker_protocol` 中仍由主进程使用的 `PackageIdentity` 和 `PackageFileRequest`；字段和 serde 仅服务验证/部署，不保留协议版本、nonce、父进程、session 或帧概念。
- 将部署计划、orchestrator、验证器和测试导入改到 `crate::package`。
- 从 `JobStage`、`JobSnapshot`、事件 payload、Tauri DTO 和错误枚举删除提权字段/阶段。
- 把 4 份旧 SQL 的最终表、列、索引、外键、CHECK 约束与本次修改合并到 `0001_initial.sql`；直接省略 `requires_elevation`，产品展示字段使用最终模型命名。
- 将 `CURRENT_SCHEMA_VERSION` 和 migration registry 重置为单一版本 1；删除 v1→v4 兼容、旧行解析和旧 schema 分支。
- 更新所有 SQL 查询、写入、row mapping 和测试 fixture，使其只面向最终 schema。
- 从 `lib.rs` 导出新领域模块，为后续删除 `broker_protocol.rs` 建立编译边界。

### 1.3 验证

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --test m0_validation --test m2_domain --test m2_persistence --test m3_persistence --test m5_deployment_plan --test m5_orchestration --test m6_api --test m6_event_store
```

记录 Task 1 实际测试数、最终 schema 表/索引清单和旧开发数据库需重建的破坏性变化到 `progress.md`，不提交。

## Task 2：建立管理员主程序与直接部署，删除 Broker

**文件：**

- 修改：`src-tauri/build.rs`
- 新建：`src-tauri/windows/app.manifest`
- 修改：`src-tauri/Cargo.toml`
- 修改：`src-tauri/Cargo.lock`
- 修改：`src-tauri/src/deployment_coordinator.rs`
- 修改：`src-tauri/src/job_worker.rs`
- 修改：`src-tauri/src/lib.rs`
- 修改：`src-tauri/tauri.conf.json`
- 修改：`package.json`
- 修改：`scripts/build-release.ps1`
- 修改：`scripts/m0-deployment-acceptance.ps1`
- 删除：`scripts/copy-broker.ps1`
- 删除：`src-tauri/src/broker_launcher.rs`
- 删除：`src-tauri/src/broker_protocol.rs`
- 删除：`src-tauri/broker/`
- 删除或替换：`src-tauri/tests/m0_broker_protocol.rs`
- 删除或替换：`src-tauri/tests/m0_contract.rs`
- 修改：`src-tauri/tests/m0_coordinator.rs`
- 修改：`src-tauri/tests/m0_deployment_acceptance.rs`
- 修改：`src-tauri/tests/m6_worker.rs`
- 修改：`src-tauri/tests/m8_release.rs`

### 2.1 先写失败测试

- 将 coordinator 路由测试改为 `CurrentUserDirect` 与 `AllUsersDirect`，并断言两个范围都调用 `WindowsDeploymentBackend`/`WindowsInventory`。
- 将 worker 测试改为验证 AllUsers 从 `Verifying` 直接进入 `Deploying`。
- 将 release 测试改为断言：主 manifest 包含 `requireAdministrator`，Tauri 配置无 `externalBin`，package scripts 无 `build:broker`，发布脚本无 Broker 路径。
- Windows-only PE 资源测试从 debug/release 主 EXE 读取 execution level；非 Windows 单元测试只验证 manifest 注入配置，不冒充 PE 验收。

### 2.2 实现直接路径

- 使用锁定的 `tauri-build` `WindowsAttributes::app_manifest` 注入 manifest。
- coordinator 的 `scan/install/uninstall` 直接按 scope 调用现有 Windows inventory/deployment 原语，并保留完整后置清单与 identity 收敛检查。
- worker 删除 `requires_elevation` 和 `AwaitingElevation` 分支。
- 从 Cargo workspace、feature 和 Windows features 中删除仅 Broker 使用的依赖能力；只有通过 `rg` 证明无其他引用后才移除 `Pipes`、`RemoteDesktop`、`Threading` 等 feature。
- 删除 Broker 源码、协议测试、复制脚本、sidecar 配置和生成产物路径。
- 发布脚本只构建主程序/NSIS，并核验主 EXE 架构、manifest 和 bundle 中不存在 Broker 文件。
- 受控部署脚本直接从已提权主进程/测试 harness 执行，不再接收 Broker 路径。

### 2.3 验证

```bash
rg -n "broker|AwaitingElevation|requires_elevation|requiresElevation|elevation_cancelled" src-tauri/src src-tauri/tests src-tauri/migrations scripts package.json src-tauri/tauri.conf.json
cargo test --manifest-path src-tauri/Cargo.toml --test m0_coordinator --test m0_deployment_acceptance --test m6_worker --test m8_release
pnpm exec tauri build --debug --no-bundle
```

只允许文档归档出现旧词；生产源码、当前测试、唯一 migration 和构建配置不得出现。

## Task 3：统一包选择预览与执行语义

**文件：**

- 修改：`src-tauri/src/applicability.rs`
- 修改：`src-tauri/src/app_runtime.rs`
- 修改：`src-tauri/src/job_worker.rs`
- 修改：`src-tauri/src/tauri_api.rs`
- 修改：`src-tauri/tests/m1_protocol.rs`
- 修改：`src-tauri/tests/m3_applicability.rs`
- 修改：`src-tauri/tests/m6_runtime.rs`
- 修改：`src-tauri/tests/m6_worker.rs`

### 3.1 先写失败测试

- x64 主机偏好只有 x64、图中只有 x86 时仍选择 x86；neutral 同理。
- ARM64 主机默认顺序为 ARM64、x86、neutral；不默认接受未经能力模型声明的 x64 仿真。
- 同一 PackageGraph/host/preferences/installed 输入的详情 preview 与 worker `SelectionResult` 主包版本、架构、格式、语言和依赖数一致。
- OS、格式、语言资源和依赖缺失分别返回封闭拒绝原因。

### 3.2 实现统一入口

- 保持 `select_packages` 为唯一选择算法；增加由 `SelectionResult` 投影出的安全 `SelectionPreview`，不含 URL、token 或本地路径。
- 把 `preferred_architectures` 从候选硬过滤改为兼容候选的排序权重；主机兼容集、OS、格式、语言和依赖仍是硬门。
- 详情、安装、更新和修复都构造同一 `SelectionPreferences` 并调用统一入口；删除详情页只数主包架构的旁路判断。
- Tauri DTO 暴露 preview 的可安装状态、主包选择和封闭拒绝原因。

### 3.3 验证

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test m1_protocol --test m3_applicability --test m6_runtime --test m6_worker
```

## Task 4：补全目录元数据与图标安全策略

**文件：**

- 修改：`src-tauri/src/catalog.rs`
- 修改：`src-tauri/src/app_runtime.rs`
- 修改：`src-tauri/src/tauri_api.rs`
- 修改：`src-tauri/tauri.conf.json`
- 新建或修改：`src-tauri/tests/fixtures/dcat-*.json`
- 修改：`src-tauri/tests/m1_protocol.rs`
- 修改：`src-tauri/tests/m4_network_policy.rs`
- 修改：`src-tauri/tests/m6_runtime.rs`

### 4.1 先写失败测试

- fixture 覆盖：第一个 localized property 缺字段、请求语言完整匹配、语言/脚本回退、跨多个 SKU 汇总 package format、稀疏搜索对象、PFN 查询。
- 图标测试覆盖：`//store-images.s-microsoft.com/...` 规范化为 HTTPS；拒绝 HTTP、凭据、fragment、非默认端口和其他 host。
- runtime 测试覆盖：最多 20 个搜索结果、最多 4 个并发补全、单项失败保留 partial 结果、FE3 回填 packageName/PFN/publisher/format。

### 4.2 扩展稳定 DTO 与 adapter

- `CatalogProduct` 使用 `app_name`，增加 `package_name`、`icon_url` 与 `metadata_state`；保留 PFN、publisher、formats 和依赖。
- normalization 遍历所有 SKU/package，不再固定第一个 availability；本地化字段按请求语言/市场分别选择第一个有效值。
- `CatalogProvider::lookup(PackageFamilyName, ...)` 作为 PFN 查询的正式边界，不猜 Product ID。
- runtime 用有界并发进行 DCAT/FE3 补全；持久化只保存安全文本与关联，不持久化图标 URL 或包 URL。
- Tauri CSP 的 `img-src` 只增加 `https://store-images.s-microsoft.com`。

### 4.3 验证

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test m1_protocol --test m4_network_policy --test m6_runtime
```

## Task 5：机器范围清单、应用名和范围推导

**文件：**

- 修改：`src-tauri/src/inventory.rs`
- 修改：`src-tauri/src/deployment_coordinator.rs`
- 修改：`src-tauri/src/app_runtime.rs`
- 修改：`src-tauri/src/tauri_api.rs`
- 修改：`src-tauri/tests/m0_inventory.rs`
- 修改：`src-tauri/tests/m0_coordinator.rs`
- 修改：`src-tauri/tests/m6_api.rs`
- 修改：`src-tauri/tests/m6_runtime.rs`

### 5.1 先写失败测试

- `PackageInventoryRecord` 包含 `app_name` 和明确的 `package_name`；Windows DisplayName 为空或 `ms-resource:` 时按“目录 appName -> identityName”回退。
- `scan_all_users` 聚合 current user、other users 和 provisioned 状态，FindUsers/FindProvisionedPackages 单项失败时保留结果并置 `complete = false`。
- 稳定去重排序键为 PFN、版本、架构和 resource ID。
- 纯函数 `derive_update_scope(record)` 覆盖五种已确认范围组合，并证明 other/provisioned 优先 AllUsers。

### 5.2 实现清单契约

- Windows adapter 读取 `Package.DisplayName`，把读取失败降级为 warning，不让整次扫描失败。
- 已安装页后端默认调用机器范围扫描；部署后置条件仍按实际任务 scope 检查完整清单。
- 在 app runtime 通过可信目录关联补齐 unresolved display name；无关联时回退 identity。
- 把范围推导放在不依赖 Windows API 的纯函数中，供更新扫描和单元测试复用。

### 5.3 验证

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test m0_inventory --test m0_coordinator --test m6_api --test m6_runtime
```

## Task 6：重写更新扫描为结构化、可解释结果

**文件：**

- 修改：`src-tauri/src/app_runtime.rs`
- 修改：`src-tauri/src/catalog.rs`
- 修改：`src-tauri/src/persistence.rs`
- 修改：`src-tauri/src/tauri_api.rs`
- 修改：`src-tauri/src/error.rs`
- 修改：`src-tauri/tests/m2_persistence.rs`
- 修改：`src-tauri/tests/m6_api.rs`
- 修改：`src-tauri/tests/m6_runtime.rs`

### 6.1 先写失败测试

- `scan_updates` 返回 `UpdateScanResult { scanned_main_packages, associated_packages, candidates, skipped, complete }`，零候选也有非零扫描计数和完成态。
- 已有可信 Product ID 关联优先复用；缺失关联时按 PFN 调用 DCAT。
- PFN 查询结果必须同时匹配 identity name、publisher 和 PFN，任何不一致都进入封闭 `skipped` 原因且不持久化。
- 单包目录/FE3/选择失败不取消其他包；结果 `complete = false`。
- candidate 冻结 `deployment_scope`；创建任务前重扫范围变化时返回 `reconcile_inventory`。

### 6.2 实现扫描流水线

- 对机器范围主包按 PFN 去重，使用设置市场/语言和有界并发补齐关联。
- PFN 关联和 FE3 解析分为两个有界网络阶段，共用 `maxConcurrentUpdateScans`（1–64，默认 16）；数据库写入不进入并发 future。
- 只保存经过 identity/publisher/PFN 三重核验的关联。
- 对每个已关联包运行统一 PackageSelectionService；候选包含 appName、packageName、publisher、PFN、当前/可用版本和建议 scope。
- 若存在更新版本但严格选择失败，记录 `selection_rejected`，不得静默返回“无更新”。
- `skipped` 仅包含 PFN、封闭原因码和消息键，不包含原始响应、HRESULT、SID 或路径。
- Tauri API 和错误映射返回结构化摘要，不再把空数组作为唯一反馈。

### 6.3 验证

```bash
cargo test --manifest-path src-tauri/Cargo.toml --test m2_persistence --test m6_api --test m6_runtime
```

## Task 7：同步 React 契约与三类页面

**文件：**

- 修改：`src/lib/types.ts`
- 修改：`src/lib/tauri.ts`
- 修改：`src/lib/i18n.ts`
- 修改：`src/test/fixtures.ts`
- 修改：`src/features/search/SearchView.tsx`
- 修改：`src/features/details/DetailsView.tsx`
- 修改：`src/features/installed/InstalledView.tsx`
- 修改：`src/features/queue/QueueView.tsx`
- 修改：`src/App.css`
- 修改：`src/test/App.test.tsx`
- 修改：`src/test/tauri.test.ts`
- 修改或新建：`tests/*.spec.ts`

### 7.1 先写失败测试

- 搜索卡显示 appName、packageName、publisher、格式和固定尺寸图标；图标失败使用占位，不改变布局。
- 详情显示 appName/packageName/PFN/publisher/format/selection preview；无可用 preview 时按钮禁用并显示具体原因。
- 已安装页默认调用机器范围扫描，搜索覆盖四类名称字段，主列三行显示应用名/包名或 PFN/发布者。
- 更新按钮运行中有状态；零候选显示“扫描完成，未发现更新”；partial 显示非阻塞警告与计数。
- 来源 badge 不折行，窄窗口无横向不可达内容；Playwright 在桌面和窄视口做可访问性与溢出断言。
- queue UI 不再识别 `awaiting_elevation` 或显示二次 UAC 文案。

### 7.2 实现 UI

- 更新 TypeScript 联合类型和 invoke 返回值；删除 `requiresElevation` 与旧阶段。
- 搜索/详情共享稳定图标和文本行布局，长值使用 ellipsis 加原生 title。
- 安装范围改为“当前管理员账户 / 所有用户”的分段控件文案。
- 已安装页使用内容驱动的范围列和 `white-space: nowrap` badge；窄视口改为稳定键值 grid。
- 所有异步状态保留显式 loading/success/partial/error，禁止扫描完成后无反馈。

### 7.3 验证

```bash
pnpm test
pnpm build
pnpm exec playwright test
```

## Task 8：文档同步、全量验证与暂存

**文件：**

- 修改：`README.md`（保留用户已暂存的中文改动，在其基础上合并）
- 修改：`docs/support-matrix.md`
- 修改：`docs/release.md`
- 修改：`docs/diagnostics.md`
- 新建：`docs/archive/2026-10-03-pre-admin-runtime-redesign/README.md`
- 修改：`docs/superpowers/specs/2026-10-03-admin-runtime-package-management-redesign.md`
- 修改：`task_plan.md`
- 修改：`findings.md`
- 修改：`progress.md`

### 8.1 文档与静态清理

- README 说明管理员启动、双范围语义、无 Broker 构建和“当前管理员账户”含义。
- support matrix 只写实际重新取得的 E0/E1/E2/E3 证据；旧 Broker E2 记录作为历史，不继承到新架构。
- release 文档和脚本一致：单主程序、manifest/PE 检查、NSIS currentUser、无 sidecar。
- diagnostics 删除 Broker/UAC 任务事件，加入 inventory/update partial 的封闭计数事件。
- 归档 README 明确历史文档只描述被替换架构，不修改归档正文。
- 更新当前计划、发现和进度，记录所有未完成的 E2/E3 门。

### 8.2 全量质量门

按顺序运行，避免 Windows 构建缓存竞争：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --lib --tests
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm test
pnpm build
pnpm exec playwright test
pnpm exec tauri build --debug --no-bundle
```

随后执行静态审计：

```bash
rg -n "broker|AwaitingElevation|requires_elevation|requiresElevation|elevation_cancelled" src-tauri/src src-tauri/migrations src package.json scripts src-tauri/tauri.conf.json docs --glob '!docs/archive/**'
test "$(find src-tauri/migrations -maxdepth 1 -type f -name '*.sql' | wc -l)" -eq 1
git diff --check
git status --short
```

### 8.3 受控 Windows 验收

仅在现有机器、测试包和回滚材料满足设计规格时执行 E2：

- 非提权启动触发 UAC；取消后无应用/worker 残留。
- CurrentUser 安装、更新/等价版本收敛、卸载与前后清单恢复。
- AllUsers stage/provision、更新/等价版本收敛、deprovision/remove 与前后清单恢复。
- 机器范围清单与系统事实一致，更新候选 scope 与现有安装方式一致。

若材料不足，明确记录“E2 未执行”，不阻塞代码层完成，也不伪造通过结论。

### 8.4 审阅并暂存

- 分别审阅用户原 `README.md` 暂存 diff、本轮工作区 diff、删除列表和生成锁文件。
- 只用显式路径 `git add` 暂存本次归档、规格、实现、测试、脚本和文档；不使用 `git add .`。
- 运行 `git diff --cached --check`、`git diff --cached --stat` 和关键 staged diff 抽查。
- 不执行 `git commit`；向用户给出一条英文 Conventional Commit 建议：

```text
refactor: replace broker deployment with an elevated app runtime
```

## 完成定义

- 当前生产源码、当前测试、构建和发布路径不含 Broker、`AwaitingElevation`、`requiresElevation` 或任务级 UAC 取消分支。
- `src-tauri/migrations/` 只保留 `0001_initial.sql`；新数据库直接得到最终 schema version 1，且 schema 不含 `requires_elevation`。
- 主 EXE 静态资源确认 `requireAdministrator`，bundle 不含 Broker sidecar。
- 搜索与详情显示应用名、包名、PFN、发布者、图标、格式和统一选择预览。
- 已安装页显示机器范围主包、应用名/包名/发布者，来源 badge 不折行。
- 更新扫描始终返回可解释摘要，缺失关联时按 PFN 查询并严格核验，候选范围匹配现有安装状态。
- 所有 E1 质量门通过；E2/E3 结论与实际证据一致。
- 范围内文件已暂存，用户原有 `README.md` 改动被保留，没有创建提交。
