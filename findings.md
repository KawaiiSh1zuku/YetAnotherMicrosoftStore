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

## 规格与计划细化审查基线（2026-10-02）

- 当前 Git 基线为 `master` / `86cae46`，工作区在本轮文档修改前干净；提交历史包含 M0 基线、M0 提权部署验收、M1 协议适配、状态同步和 M2 持久化。
- 总规格的状态摘要已经承认 M0-M2 完成，但“审批门槛”和“发布阶段”仍保留 M0 尚未开始时的叙述，需要改为当前执行门与 M0-M9 一致的分阶段证据模型。
- 总实施计划已把 M0-M2 标记为完成，但需要进一步拆开每个里程碑的产物、自动化验证、真实环境验证、明确未覆盖项和下一里程碑前置条件，避免把 fixture/build 证据扩大为实时 Store 或跨渠道验收。
- 本轮审查采用四级证据：静态代码存在、自动化测试/构建通过、受控 Windows 真实验收通过、外部 Store/在线服务互操作通过；后一级不能由前一级推断。
- M0、M1、M2 的最终状态将在专项文档、实现文件、测试文件和可复现命令完成交叉核验后写入总计划与进度日志。

## M0 与总计划匹配审查（2026-10-02）

- M0 专项规格、Task 1-7 实现计划、`docs/support-matrix.md`、`docs/evidence/m0/README.md`、提交 `157c23c` 和当前实现文件形成一致证据链；状态可判定为“完成（受 Windows 10 build 19045 x64 与既有自签测试包范围约束）”。
- 历史真实验收覆盖 CurrentUser 安装/清单/卸载、AllUsers stage/provision/机器清单/deprovision/RemoveForAllUsers、一次性 UAC Broker、包与证书清理；本轮不会仅为文档审查重复执行会改变 Windows 包和证书状态的脚本。
- M0 自动化与静态证据覆盖协议帧、清单 DTO、协调路由、路径/哈希/身份校验和 ignored 真实包测试入口；真实验收记录不能由普通 `cargo test` 重建，必须保持为独立证据层。
- M0 专项设计标题仍写“待用户审阅”，与实现计划和真实验收冲突；应改为“已批准并完成”，同时保留验证机、载荷和未覆盖 bundle/eAppx 的限制。
- 总计划 M5 中“实现普通安装、全用户 broker、机器清单”的描述与 M0 已完成产物重复；M5 应改成复用 M0 部署基础，补齐包图安装、更新比较、Store 身份关联和来源无关更新编排。

## M1 与总计划匹配审查（2026-10-02）

- `src-tauri/src/catalog.rs` 与 `resolver.rs` 将 `storelib_rs 0.1.11` 限制在 adapter 边界，向项目其余层暴露自有 trait/DTO；依赖版本在 `Cargo.toml` 中精确固定。
- `m1_protocol` fixture 测试覆盖 DCAT 搜索/产品身份、非 HTTPS URL 拒绝、项目自有 provider 类型、FE3 包/依赖边、坏 XML 和缺失 moniker；测试只证明离线解析与规范化。
- production adapter 已有实时方法，但本仓库没有实时 DCAT/FE3、授权、地区和临时 CDN URL 的验收证据。因此 M1 应标记为“完成（离线协议适配）”，并把线上契约 smoke test 明确留在 M4/M5 前的受控验收门。
- 总计划 M1 原任务同时提到 `PackageIdentity`/`InstalledPackage`，但这些 Windows 身份/清单结构实际由 M0 inventory/broker 协议和 M2 领域模型承担；应从 M1 退出条件中剥离，避免错误归属。
- 原始 FE3 `package_uri` 仍存在于 Rust resolver DTO，用于后续下载层；当前没有穿过 Tauri 命令边界。M4 必须在网络/下载边界增加 host allowlist、URL 过期和日志脱敏验证，不能把 M1 的结构解析当作 URL 安全验收。

## M2 与总计划匹配审查（2026-10-02）

- M2 已建立项目自有 domain/error/job 类型、SQLite schema v1、repository、migration 重放/事务回滚和重启恢复；`rusqlite 0.40.2` 精确固定并使用 bundled SQLite。
- Job 已冻结请求市场、架构、语言和部署范围；下载/解析/验证中断恢复为 `Interrupted + ReResolve`，部署中断恢复为 `NeedsReconciliation + ReconcileInventory`，避免盲目重复部署。
- 设置持久化只保存代理模式、host、port 和凭据策略，不保存用户名/密码；诊断 operation 使用枚举，符合 M2 的凭据和诊断边界。
- M2 当前退出条件所列身份名/PFN、版本、架构、语言、市场、来源和恢复语义已有实现/测试，可判定为“完成（本地域模型与持久化）”。
- M3 入口前仍需扩展包 publisher、resource ID、package kind、最低 OS/build 和资源限定字段；这些不是把 M2 降为未完成的理由，但必须成为 M3 的前置子任务和 schema v2/migration 决策。
- `SafeErrorDetail { key, value }` 仍允许任意字符串。进入实时目录/下载前应改为封闭 detail 枚举或按错误码限定字段，避免 URL、令牌和服务原文进入前端 DTO/持久诊断。
- 当前 M0 Tauri 命令 `scan_installed_packages`、`install_package`、`uninstall_package` 仍以 `Result<_, String>` 暴露错误，且脚手架 `greet`/`probe_deployment` 仍注册；这属于 M0 调试接口，不满足总规格的稳定 `AppErrorDto` 契约。M6 前必须移除/隔离调试命令并把部署错误映射到封闭前端 DTO。

## M3 领域加固与选择器发现（2026-10-02）

- `storelib_rs 0.1.11::PackageInstance` 已提供 `family_metadata.publisher`、`package_identity_name`、`package_content_id`、`main_package`、`is_appx_framework`、`default_properties_language` 和 applicability target platform；项目 adapter 可复用这些 typed fields，无需解析任意 extra attribute。
- FE3 package moniker 的右侧段可提供四段版本、架构和 resource ID；项目从右向左拆分并把版本收紧为四个 `u16`。最低 OS 的 packed `u64` 同样转换为四段版本，比较不再依赖字符串顺序。
- schema v1 旧记录没有可信 publisher/resource/package-kind/minimum-OS/neutral/content-ID，migration 使用 `unknown`/`NULL` 保持“未知”，不伪造 neutral 或 main 语义。
- 安全错误详情现为 `SafeErrorDetail::Field { SafeField }`；只有显式允许的协议字段可进入 DTO，未知字段被丢弃，URL、令牌和响应正文没有自由字符串入口。
- 架构兼容矩阵属于主机能力，而不是选择器常量。这样 ARM64 对 x64/x86 的实际支持可由后续 Windows 探针提供，算法本身只消费明确能力。
- BCP-47 选择先匹配完整 tag，再匹配主语言；无语言的 scale 等资源包不应被语言过滤器删除，neutral 资源与最佳语言资源可以同时进入 bundle 包图。
- 缺失 prerequisite 或 bundled update 均是 `dependency_unresolved`，不能静默丢边；已安装同身份且版本不低于要求的框架可满足 prerequisite。
- M3 只证明本地选择契约。`PackageFormat::Msixvc` 可被识别并由支持格式列表拒绝，但这不证明 MSIXVC 下载或部署；bundle/eAppx 同样没有真实载荷验收。
- M3 复审确认 schema v1 中已持久化的开放错误详情不能直接按新封闭枚举读取；当前兼容层只恢复白名单 `field`，其余旧详情统一变为 `redacted`，避免迁移丢任务或继续暴露任意值。
- 依赖遍历必须区分 visiting/completed：selected 去重本身不能终止环，也不能替代 bundle 子项的传递依赖遍历。当前环返回稳定依赖错误，所有实际选中节点都继续解析其边。
- 已安装依赖匹配至少需要 identity、publisher 和 architecture；只比较 identity 会把 ARM64/x86 framework 误当成 x64 依赖。Update 还必须先匹配已安装对象，缺失时不得退化为 Install。
- BCP-47 回退应按 exact → 较短同 script → 较长同 script → 同 primary language 排序，并按资源 identity 分组选择；全局只保留一个语言资源会漏掉独立资源组。
- 当前失败路径返回稳定 `ApplicabilityError`，但不会携带此前累计的逐包拒绝解释；M3 尚未把选择器暴露为 Tauri 命令，M6 设计安全 API/UI 投影时必须补失败解释 DTO，不能直接序列化含 `package_uri` 的内部 `SelectionResult`。

## M4 网络、下载与缓存发现（2026-10-02）

- `WinHttpGetIEProxyConfigForCurrentUser` 返回当前用户 Internet Options 的静态代理、bypass、自动检测和 PAC URL 标志；M4 只把静态值转换为显式 `reqwest::Proxy`。PAC/WPAD 需要按目标 URL 调用 Windows 自动代理解析，不能用环境变量或单一代理 URL冒充。
- WinHTTP 代理字符串可能按协议给出多个端点；Microsoft 包 URL 使用 HTTPS，因此解析优先选择 `https=`，再退到通用端点。bypass 的分号列表转换为 reqwest 逗号列表，`*.domain` 收紧为 `.domain`；`<local>` 不扩张为不精确的任意主机规则。
- `ClientBuilder` 先 `no_proxy()` 再安装选定 route，保证 disabled/custom/system 不与 reqwest 环境代理叠加；自定义 SOCKS5 使用 `socks5h`，让代理端解析 DNS。
- 续传不能只依赖本地文件长度。只有 sidecar 的 update ID、期望大小、期望 SHA-256 和非空 ETag 全部匹配时才发送 `Range` + `If-Range`；服务端 ETag、range start 或总长度变化均清理旧 partial 并至多从零重启一次。
- Content-Length 仅作早期拒绝，最终信任边界仍是落盘后的实际大小和 SHA-256。校验失败会清理 partial，取消/传输失败则保留可恢复 partial；verified 文件只由同一缓存卷内的 rename 提升。
- 签名 URL 只存在于内存中的 `DownloadRequest`；partial sidecar 只含 job/update、ETag、期望大小/哈希和访问时间，SQLite cache entry 只含本地路径与内容元数据。
- 缓存根在创建前后拒绝重解析点，文件操作同时核对原始绝对路径和 canonical path；根外记录只去索引不删除外部文件。共享内容按唯一物理路径计费且只有最后一个引用淘汰时才删除，活动 job 的 partial 即使超额也保留。
- 同一 `DownloadManager` 内相同 cache key 串行化，避免并发写同一 partial/sidecar；未知长度响应会在下一个 chunk 超出期望大小前终止，校验/rename 前重复检查取消。限速器只预留未来发送时隙，不积累无限空闲额度，也不持锁睡眠。
- verified 内容提升后、SQLite 入库前崩溃会留下孤儿文件；无活动下载的启动协调会清理未索引 verified 文件。有活动任务时跳过该清理，避免删除刚提升但尚未入库的内容。
- 受控在线 smoke 表明 `storelib_rs 0.1.11` 在指定输入和时间点仍能返回 DCAT/FE3 包图，但同时揭示 ARM32 moniker 是真实输入。该证据不覆盖付费/授权产品、其他市场、CDN 字节下载、包签名或 Windows 安装。

## M5 编排与签名预检发现（2026-10-03）

- Microsoft 的 MSIX 说明把 `AppxSignature.p7x` 与 `AppxBlockMap.xml` 作为签名和包内容完整性基础；所有可安装 MSIX 都必须签名，Windows 部署仍负责最终签名/依赖验证。
- Microsoft 的 `WinVerifyTrustEx` 文档说明 `WINTRUST_ACTION_GENERIC_VERIFY_V2` 用默认 Authenticode policy 校验文件/对象，返回值虽声明为 HRESULT，实际必须按 Win32 error code 与零比较，不能用 `SUCCEEDED`/`FAILED`。
- M5 签名预检应直接调用现有 `windows` crate 的 WinTrust API，禁止引入 SignTool/PowerShell/winget 子进程；真实受信任签名成功路径仍需要显式签名包环境，普通自动化只覆盖无签名/损坏载荷拒绝和接口契约。
- M5 不扩展 M0 `DeploymentCoordinator`：新增编排端口负责 verified 包图装配、严格版本差异、身份关联和部署后收敛；实际 CurrentUser/AllUsers 执行仍唯一委托既有 M0 direct/Broker 路径。
- Windows `canonicalize()` 返回本地磁盘 verbatim 路径；路径策略必须允许 `Prefix::VerbatimDisk`，同时继续拒绝 UNC、VerbatimUNC 和 DeviceNS。回归测试确认正确 manifest/hash 的无签名包能到达 WinTrust 并返回 `SignatureInvalid`。
- AllUsers 的目标状态是 `provisioned_for_future_users`，不能用“任意用户已安装同版本”替代；当前用户与全用户分别使用 scope-aware 已安装/收敛谓词。
- `ProvisionPackageForAllUsersAsync` 只显式预配主包，framework 可由已预配主包的依赖关系隐式保留且不出现在 `FindProvisionedPackages`。因此 AllUsers 后置条件要求主包显式预配，但允许完整机器清单中的足够版本 framework；optional/resource 仍不放宽。
- 清单可能同时存在同一 identity 的多个版本。防降级必须检查全部 scope-relevant 记录，主包后置条件寻找精确目标版本，依赖允许不低于目标版本，均不得依赖枚举顺序。
- 已验证部署关联是比 identity/publisher 推断更强的证据；相同 PFN/product/identity/publisher 的 no-op 只刷新观测，不得把 `VerifiedDeployment` 降级。
