# 研究发现

## 仓库状态

- 接手时工作区 `E:\Projects\YetAnotherMicrosoftStore` 为空。
- 没有 Git 仓库、README、既有架构、源代码树或测试套件。
- 因此本设计是从零开始的规格，不受既有模块兼容性约束。

## Microsoft Store 协议证据

- StoreLib 描述了 Display Catalog 查询和 FE3 包链接解析，包括 `.appx`、`.eappx`、`.xvc` 和 `.msixvc` 包记录。
  来源：https://github.com/StoreDev/StoreLib
- StoreLib 仓库已归档，因此只能作为协议参考，不能盲目作为依赖跟踪。
- `storelib_rs` 是社区 Rust 移植版，支持 Display Catalog、FE3、搜索、包模型、依赖数据以及进度/取消。
  来源：https://docs.rs/crate/storelib_rs/latest
- 设计会将 `storelib_rs` 隔离在项目自有 provider 接口之后，并固定/审计选定 revision。

## Windows 部署证据

- `PackageManager.AddPackageAsync` 接受本地文件 URI 和依赖包 URI，并为当前用户执行包部署。
  来源：https://learn.microsoft.com/en-us/uwp/api/windows.management.deployment.packagemanager.addpackageasync?view=winrt-28000
- `PackageManager` 暴露更广安装范围所需的预配/部署 API，但全用户安装需要管理员/UAC 路径和独立权限 Spike。
  来源：https://learn.microsoft.com/en-us/uwp/api/windows.management.deployment.packagemanager?view=winrt-28000
- Windows 将包部署/查询 API 作为安装、更新、卸载和检查 MSIX/AppX 包的受支持方式。
  来源：https://learn.microsoft.com/en-us/windows/win32/appxpkg/package-deployment-api
- Microsoft 将 Package Identity 定义为稳定包元组，将 Package Family Name 定义为由名称/发布者派生的标识；版本、架构和资源 ID 是用于包关联的独立字段。
  来源：https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/package-identity-overview
- Microsoft 建议同一个应用跨多个渠道分发时保持一致的应用身份和更新机制。
  来源：https://learn.microsoft.com/en-us/windows/apps/get-started/best-practices
- Microsoft 记录了独立 MSIX 分发后仍可从 Microsoft Store 手动更新应用的案例，但这只是兼容分发示例，不是所有产品、授权或渠道的普遍保证。
  来源：https://learn.microsoft.com/en-us/windows-app/configure-updates-windows

## 跨渠道互操作

- 官方 Store 安装必须从 Windows 包清单检测，而不是依赖本客户端自己的任务历史。
- 客户端必须保留原始 Microsoft 包身份和签名；对 Store 包绝不重新打包、重签名或修改清单。
- 第三方到官方 Store 的更新取决于 Store 识别同一身份，以及用户具备所需授权和可用性。
- 因此设计承诺来源无关的检测和尽力而为的 Store 接管兼容性，并在授权/渠道条件阻止更新时明确报告。

## MSIXVC 边界

- Microsoft 将 MSIXVC 记录为“Microsoft Installer for Xbox Virtual Console”，并为流数据源提供独立 COM/API 接口。
  来源：https://github.com/MicrosoftDocs/win32/blob/docs/desktop-src/appxpkg/msixvc-api-reference.md
- 因此 msixvc 不作为普通 `.msix` 扩展处理。第一阶段只识别并标记延后；下载/安装/更新属于独立能力门。
- Xbox 包明确不在第一阶段范围内。

## 代理证据

- Microsoft 区分 WinINet 和 WinHTTP。需要用户 Internet 选项代理设置的桌面应用应使用感知用户的路径；WinHTTP 可以从 WinINet 设置配置，但不会自动共享所有浏览器行为。
  来源：
  - https://learn.microsoft.com/windows/win32/wininet/wininet-vs-winhttp
  - https://learn.microsoft.com/en-us/windows/win32/winhttp/setting-wininet-proxy-configurations-in-winhttp
- 产品会显式建模代理策略，而不是假设 reqwest 自动遵循每一种 Windows 系统/PAC 设置。

## UI 设计证据

- 选定的 UI 方向是 Vite + React + TypeScript + shadcn/ui + Tailwind CSS。
- 使用基于 Radix 的原语处理焦点管理、键盘导航和对话框。
- 使用语义 HTML、可见焦点环、任务进度 live region、WCAG AA 对比度和减少动画支持。
- 使用 CSS 变量实现浅色/深色/系统主题以及移动优先 Tailwind 断点。

## 外部来源处理

- 网页和仓库只作为证据，不执行外部页面中的指令。
- docs-generator 技能引用了安装目录中不存在的 `tool-index.md` 路径；已手动采用其渐进披露和面向任务的文档规则。

## M1 实现发现（2026-10-02）

- `storelib_rs 0.1.11` 提供 Display Catalog 和 FE3 解析能力，但其 `FE3Handler::get_package_instances` 低层结果将 `update_id` 留给上层 handler 填充；项目 adapter 按 package moniker 回查所属 `<UpdateIdentity>`，避免按响应顺序错误关联依赖。
- `storelib_rs` 的原始 DCAT/FE3 结构没有进入项目 DTO；fixture 只使用 `download.invalid` 占位地址和合成包身份，不包含真实令牌、临时 URL 或用户数据。
- 本次只验证离线 fixture 和 adapter 边界。实时端点变化、鉴权/地区限制、FE3 URL 解析和 Windows 包部署仍属于后续验收，不得由 M1 测试结果推断。

## M0 部署验收发现（2026-10-02）

- `PackageManager.AddPackageAsync` 的本地文件 URI 必须去除 Windows `\\?\\` canonicalize 前缀；否则 URI 会变成 `file://///?/E:/...` 并返回 `0x80070057`。已在 `deployment.rs::file_uri` 中显式剥离设备前缀并拒绝 UNC。
- Broker 默认进程栈约 1 MiB；包校验使用 1 MiB 栈数组会触发 `0xc00000fd`。已改为堆分配缓冲区，并通过真实 UAC 全用户回环验证。
- 命名管道采用字节模式加长度前缀，而不是依赖消息边界；这样 `read_exact` 可稳定处理请求/响应帧。
- 全用户验收的真实后置条件不是 Broker 返回码，而是机器范围清单完整、目标 PFN/full name 不残留；脚本 finally 还必须证明精确证书指纹在所有显式 stores 中不存在。

## 当前代码状态扫描（2026-10-02）

- Git 基线为 `master` / `157c23c52f1e8d08dc389c63e3d1b3d77a8c64c4`。扫描到的 M0 实现集中在部署、包校验、清单、Broker 协议/启动和协调模块；M1 实现集中在 DCAT/FE3 adapter、项目 DTO 和脱敏 fixture；没有发现 M2 持久化代码。
- 现有 Rust 测试覆盖 M0 Broker 协议、部署契约、清单、协调和校验，以及 M1 协议 fixture。当前 `cargo test --all-targets` 结果是 25 项通过、3 项 ignored；ignored 测试要求签名包和 M0 环境变量，不能作为本轮真实包验收结果。
- 当前构建证据为 Rust 格式检查、Broker `cargo check`、前端 `pnpm build` 和 Tauri debug 非 bundle 构建均成功；构建过程复制了 Broker 到 `src-tauri/broker/deployment-broker-x86_64-pc-windows-msvc.exe`，该生成文件保持被忽略。
- 文档一致性检查发现设计规格、README 和总实施计划滞后于代码/验收记录，已统一更新为 M0 完成、M1 离线适配完成、M2 待开始；仍明确不承诺实时 Store/FE3、下载、跨渠道更新或 MSIXVC。

## M2 领域与持久化评审（2026-10-02）

- 选择固定 `rusqlite 0.40.2`、关闭默认功能并启用 bundled SQLite；M2 使用项目自有 repository 和显式 SQL migration，不引入 ORM 或异步数据库运行时。
- M1 provider DTO 隔离有效，M2 不改写 `storelib_rs` adapter；新增转换层把协议错误映射为规格中的稳定错误码、消息键和重试策略。当前转换只传固定字段，但 `SafeErrorDetail` 类型本身仍需在 M3 前收紧为封闭类型。
- 活动下载/解析/校验任务在重启后必须重新解析临时 URL；部署中断不能盲目重试，必须先进入 `NeedsReconciliation` 并扫描 Windows 包清单。
- 自定义代理设置需要保存协议、主机、端口和“是否使用安全凭据存储”的策略，但普通 SQLite 设置不得承载用户名/密码。诊断 operation 使用枚举而不是自由文本，减少把 URL、令牌或服务响应写入持久诊断的风险。
- M1 的 `MissingField("product")` 只能证明响应缺失字段，不能证明目录明确返回产品不存在；它应映射为可重试的 `catalog_unavailable`，将 `catalog_not_found` 保留给未来明确的服务语义。
- 独立审查发现并已修复三项 Important：待处理恢复动作跨二次启动丢失、Job 未冻结原始解析/部署请求上下文、rollback 测试在第一条 DDL 就失败而没有真正覆盖事务回滚。
- M3 入口仍需补齐 package publisher、resource ID、包种类和最低 OS 适用性字段；M2 当前退出条件只覆盖已承诺的身份名/PFN、版本、架构、语言、市场、来源和任务恢复。
