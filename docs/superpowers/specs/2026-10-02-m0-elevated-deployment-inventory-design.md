# M0 按需提权部署与双层包清单设计规格

> 状态：已批准并完成。提交 `157c23c` 在 Windows 10 build 19045 x64 与既有自签测试 MSIX 上完成 M0 退出验收；该证据不扩展为其他 Windows 构建、架构、bundle/eAppx、实时 Store 或跨渠道更新保证。

## 1. 目标与已确认决策

M0 必须建立后续安装、更新检测和跨渠道识别可以直接复用的 Windows 部署基础，而不是只完成 `PackageManager` 激活探针。

已确认采用以下架构：

- Tauri 主进程始终以当前交互用户的普通权限运行。
- 当前用户安装、卸载和清单扫描直接在主进程中执行。
- 全用户安装、全用户卸载和机器范围清单扫描通过一次性原生 Broker 按操作触发 UAC。
- 清单同时提供当前用户视图和提权后的机器范围视图；全包清单不得删除或用客户端任务历史代替。
- 产品代码不启动 PowerShell、winget、Microsoft Store UI 或 Windows Update 扫描。
- 普通 MSIX/AppX 与 bundle 属于本路径；MSIXVC、Xbox、EXE 和 MSI 仍在既有能力门之外。

永久提权整个 Tauri 主进程不作为默认实现。只有产品未来明确转为纯管理员设备管理工具时，才重新评估 `requireAdministrator`；本次不为它保留双实现分支。

## 2. M0 成功标准

M0 只有在 Windows 10 x64 验证机上同时满足以下条件后才可标记完成：

1. 非提权主进程可以枚举当前用户包、安装受信任测试包、重新枚举并卸载该包。
2. 主进程可以启动独立 Broker，UAC 取消能返回稳定且可区分的结果。
3. 提权 Broker 可以把受信任测试包暂存到系统卷并为所有用户预配，随后由机器范围清单发现。
4. 提权 Broker 可以取消预配并从所有现有用户移除测试包，随后当前用户、机器范围和预配清单均确认其不存在。
5. 两层清单输出同一套项目自有 DTO，能提供更新检测需要的包身份、版本、架构、资源属性和安装范围。
6. Broker 拒绝协议版本不匹配、非法操作、路径越界、重解析点逃逸、文件哈希变化、包身份不匹配和不受信任签名。
7. 产品执行路径中不存在 shell、PowerShell 或 winget 子进程。
8. 普通构建、契约测试和非提权扫描在没有管理员权限时仍可运行；需要管理员权限的验收被明确隔离。

测试包必须是项目自有的最小合成 MSIX，不得使用或卸载系统/用户真实应用来证明流程。

## 3. 总体架构

~~~text
React UI
   |
Tauri command/event boundary
   |
DeploymentCoordinator (普通权限)
   |-- CurrentUserDeployer ------> Windows PackageManager
   |-- CurrentUserInventory -----> FindPackagesForUser("")
   |
   `-- BrokerLauncher -- UAC --> deployment-broker.exe
                                 |-- request authentication/validation
                                 |-- ElevatedDeployer
                                 `-- MachineInventory
~~~

`DeploymentCoordinator` 是唯一的业务编排入口。UI 不直接选择 WinRT API，也不接触 SID、句柄、命名管道、原始 HRESULT 或 Broker 命令行。

主进程和 Broker 复用一个不依赖 Tauri 的 Rust 核心库，包含请求/响应 DTO、包身份、错误码、路径与哈希校验以及 WinRT 结果转换。Broker 是独立 Windows 二进制，不加载 WebView，不提供通用命令执行能力，并在完成单个请求后退出。

## 4. 组件边界

### 4.1 `deployment`

负责把已经验证的包集合交给 Windows 部署 API：

- `install_current_user`：使用 `AddPackageAsync` 安装到调用用户。
- `remove_current_user`：使用 `RemovePackageAsync` 从调用用户移除指定 Package Full Name。
- `stage_for_all_users`：由 Broker 使用 `StagePackageAsync` 把主包及显式依赖暂存到系统卷。
- `provision_for_all_users`：由 Broker 使用 `ProvisionPackageForAllUsersAsync` 处理已暂存主包的 Package Family Name。
- `remove_for_all_users`：先取消 Package Family Name 的预配，再使用带 `RemovalOptions::RemoveForAllUsers` 的移除 API 清理现有用户的主包版本。

全用户卸载只自动移除目标主包族。框架和共享依赖可能仍被其他包使用，M0 不做激进的依赖垃圾回收。

`ProvisionPackageForAllUsersAsync(String)` 从 Windows 10 build 16299 起可用，要求管理员权限，且包必须已经暂存并位于系统卷。M0 验证机 build 19045 满足 API 版本下限。Windows 11 才提供的带 `PackageAllUserProvisioningOptions` 重载不进入 Windows 10 路径。

参考：

- [StagePackageAsync](https://learn.microsoft.com/en-us/uwp/api/windows.management.deployment.packagemanager.stagepackageasync?view=winrt-26100)
- [ProvisionPackageForAllUsersAsync](https://learn.microsoft.com/en-us/uwp/api/windows.management.deployment.packagemanager.provisionpackageforallusersasync?view=winrt-26100)
- [DeprovisionPackageForAllUsersAsync](https://learn.microsoft.com/en-us/uwp/api/windows.management.deployment.packagemanager.deprovisionpackageforallusersasync?view=winrt-26100)

### 4.2 `inventory`

负责 Windows 包对象到稳定项目 DTO 的转换，不执行目录查询或更新比较。

- `scan_current_user()` 使用空 SID 调用 `FindPackagesForUser("")`，普通权限可用。
- `scan_machine()` 只在 Broker 中运行，联合 `FindPackages()`、`FindProvisionedPackages()` 和 `FindUsers(package_full_name)` 生成机器范围快照。
- 当前用户扫描失败必须返回错误；不得把 `AccessDenied` 或 API 失败转换为空清单。
- 机器扫描可以返回带警告的部分快照，但必须标记 `complete = false`。更新检测不得把不完整快照中缺失的包解释为已卸载。

Microsoft 文档规定，空 SID 表示当前用户；查询其他用户 SID 需要管理员权限，否则返回访问拒绝：

- [FindPackagesForUser](https://learn.microsoft.com/en-us/uwp/api/windows.management.deployment.packagemanager.findpackagesforuser?view=winrt-26100)

### 4.3 `broker_protocol`

定义主进程与 Broker 共用的版本化协议。协议只包含白名单操作：

- `InstallAllUsers`
- `UninstallAllUsers`
- `ScanAllUsers`

Broker 不接受可执行文件路径、命令行、脚本、注册表修改、任意文件操作或由调用方指定的 WinRT 方法名。

每个请求包含：

- 协议版本和请求 ID。
- 操作类型。
- 发起进程 ID、交互会话 ID 和一次性随机 nonce。
- 主包与依赖的规范路径、预期 SHA-256、预期身份和发布者。
- 卸载时的 Package Family Name 与待移除 Package Full Name 集合。

每个响应包含：

- 同一请求 ID。
- 稳定结果码、阶段和可安全展示的消息。
- Windows HRESULT 与脱敏诊断文本。
- 部署前后观察到的身份或清单快照。

协议不承诺跨大版本向后兼容。版本不一致时 Broker 必须在执行任何部署 API 前失败。

### 4.4 `broker_launcher`

负责创建受限 IPC 端点并通过 Windows `runas` 启动 Broker：

- 每次操作生成独立命名管道名和至少 128 位随机 nonce。
- 管道 DACL 只允许发起用户 SID、Administrators 和 SYSTEM。
- 启动参数只包含协议版本、管道名、nonce 和父进程 ID，不包含包 URL、令牌或完整请求。
- UAC 被取消时返回 `ElevationCancelled`，不进行自动重试。
- 启动、连接和单次请求均有超时；超时后关闭管道并终止本次协调状态，但不得强杀未知进程。
- Broker 处理一个请求后立即退出，避免形成常驻高权限服务。

Broker 必须验证发起进程 PID、映像路径、会话、nonce 和管道对端身份，但 unsigned Release/Debug 均不要求调用方 Authenticode。高权限操作仍只接受封闭协议，并继续验证受保护暂存边界、包身份、哈希和 Microsoft 包签名。

## 5. Broker 安全边界

UAC 只证明用户同意启动高完整性进程，并不自动使来自普通进程的请求可信。Broker 在调用部署 API 前按以下顺序验证：

1. 确认自身处于提升后的管理员令牌；否则返回 `ElevationRequired`。
2. 验证协议版本、nonce、请求 ID、发起进程、用户 SID 和交互会话。
3. 验证操作属于固定白名单，字段数量和总消息大小不超过上限。
4. 将包路径规范化并限制在项目专用暂存根目录；拒绝 UNC、设备路径、相对路径和 Alternate Data Stream。
5. 使用禁止跟随重解析点的方式逐层打开路径，确认最终文件仍位于暂存根并且是普通文件。
6. 在同一个已打开源文件句柄上计算 SHA-256，并从该句柄复制到 Broker 创建的管理员专属暂存目录。
7. Broker 暂存目录位于受保护的机器范围目录，DACL 只允许 Administrators 和 SYSTEM；Broker 对副本重新计算哈希，并只把该副本的 URI 交给 Windows 部署 API。这样关闭用户可写源文件到 WinRT 再次打开文件之间的替换窗口。
8. 从受保护副本读取包清单，核对 Name、Publisher、Version、Architecture、ResourceId、主包/依赖角色和请求预期值。
9. 让 Windows 部署栈执行最终签名、依赖和适用性验证；Broker 不提供跳过签名验证的部署选项。
10. 部署完成后重新扫描 Windows 包状态，以观察结果决定成功，而不是只依赖异步调用已返回。

Broker 在正常完成后删除自己创建的单请求暂存目录。异常退出遗留目录只能由后续提升后的 Broker 按固定根目录、所有者、目录命名格式和保留时间清理；普通主进程不得删除或遍历管理员专属暂存目录。

Broker 日志不得记录原始 Store 下载 URL、身份令牌、完整 SID 或用户目录。请求 ID、包族、版本、阶段、HRESULT 和脱敏路径足以支持诊断。

## 6. 双层清单数据模型

项目内部使用统一记录，WinRT 对象不得越过 `inventory` 模块：

~~~text
PackageInventoryRecord
  identity_name
  publisher
  package_family_name
  package_full_name
  version { major, minor, build, revision }
  architecture
  resource_id
  package_kind { main, framework, resource, optional, bundle }
  signature_kind
  status
  installed_for_current_user
  installed_user_count
  has_other_users
  provisioned_for_future_users
~~~

快照元数据包含：

~~~text
InventorySnapshot
  source { current_user, all_users_elevated }
  captured_at
  os_build
  complete
  records[]
  warnings[]
~~~

机器范围结果不向 WebView 暴露原始 SID。M0 只返回当前用户标记、用户计数和是否存在其他用户；这足以支持包去重、版本比较和安装范围展示。以后若出现明确的逐用户管理需求，再通过独立权限设计扩展，不提前泄露用户标识。

更新检测以 `Package Family Name + architecture + resource_id` 关联包族，以 Package Full Name 和四段版本识别已安装实例。安装来源只作诊断元数据，不参与包身份判定。

## 7. 数据流

### 7.1 当前用户安装

1. 主进程完成下载、哈希和包身份预检。
2. `DeploymentCoordinator` 调用 `install_current_user`。
3. Windows 部署完成后执行当前用户清单重扫。
4. 只有预期 Package Full Name 出现在完整快照中才返回成功。

### 7.2 全用户安装

1. 主进程将已经验证的主包与依赖复制到用户侧专用暂存目录，并固定预期哈希和身份。
2. UI 展示包、版本、发布者和“所有用户”范围，用户确认后启动 UAC。
3. Broker 验证调用方、IPC 和每个源文件，并复制到管理员专属暂存目录后再次校验。
4. Broker 从受保护副本调用 `StagePackageAsync`，把包集合暂存到系统卷并取得实际 Package Family Name。
5. Broker 为主包族执行全用户预配。
6. Broker 执行机器范围重扫，确认主包已预配并记录现有用户状态。
7. Broker 清理管理员专属副本；主进程接收结构化结果、关闭 IPC，并清理由本任务创建且不再使用的用户侧暂存文件。

Windows 10 路径不假设 Windows 11 的可选包原子预配能力。包含复杂 optional/package set 的产品若无法通过已验证的 Windows 10 API 完整预配，必须返回 `UnsupportedPackageGraph`，不能退化为“主包看似成功”。

### 7.3 全用户卸载

1. 普通权限 UI 根据机器快照展示影响范围并要求明确确认。
2. Broker 验证目标 PFN/Full Name 与新鲜机器快照一致。
3. Broker 取消该主包族对未来用户的预配。
4. Broker 使用 `RemoveForAllUsers` 移除现有用户的目标主包版本。
5. Broker 重扫当前安装与预配状态；任一残留都返回部分失败，而不是成功。

## 8. 错误模型

稳定错误码至少包含：

- `ElevationRequired`
- `ElevationCancelled`
- `BrokerLaunchFailed`
- `BrokerTimeout`
- `BrokerProtocolMismatch`
- `BrokerCallerRejected`
- `BrokerStagingFailed`
- `InvalidPackagePath`
- `PackageChangedAfterValidation`
- `PackageIdentityMismatch`
- `PackageSignatureRejected`
- `UnsupportedPackageGraph`
- `InventoryAccessDenied`
- `InventoryIncomplete`
- `DeploymentFailed`
- `PostconditionFailed`

错误分为用户动作、权限/策略、输入完整性、Windows 部署和后置条件五类。UI 可以针对 UAC 取消提供再次尝试，但不得对签名、身份、路径或调用方验证失败提供“仍然继续”。

## 9. 测试与验收

### 9.1 不需要提权的自动测试

- 请求/响应 JSON 或二进制帧的往返、版本不匹配和消息大小限制。
- 路径格式、扩展名、UNC/设备路径、重解析点和暂存根逃逸测试。
- 包身份、版本、架构和清单 DTO 转换测试。
- 当前用户清单扫描在非提权进程中成功，并明确记录 `complete`。
- 当前用户测试包安装、清单发现、卸载和清单消失的串行验收。
- 产品源代码检查，保证没有 PowerShell、winget 或通用 shell 执行路径。

### 9.2 需要一次或多次 UAC 的 Windows 验收

- UAC 取消返回 `ElevationCancelled`，没有包状态变化。
- 错误 nonce、错误调用方 PID/session/镜像路径、越界路径、文件替换和包身份不匹配均在部署前被拒绝。
- 对项目合成测试包执行 stage → provision → machine scan。
- 对同一测试包执行 deprovision → remove-for-all-users → current/machine/provisioned scan。
- 记录每个阶段的 Package Family Name、Package Full Name、版本、HRESULT 和前后快照，但不记录私钥或完整用户 SID。

验收使用短期测试证书时，必须先记录证书指纹和安装位置，完成后从明确的测试证书存储中移除同一指纹。测试包、证书和私钥只能位于被 Git 忽略的构建目录，不进入提交。产品执行路径仍依赖 Windows 正常信任策略，不包含安装根证书或放宽签名验证的功能。

独立只读验证可以由开发命令调用 PowerShell 查询 AppX 状态，但这只是验收证据；产品二进制本身不得启动 PowerShell。

## 10. 影响范围与兼容性

实施阶段预计涉及：

- 拆分现有 `deployment.rs`，新增 `inventory`、`broker_protocol` 和 `broker_launcher` 模块。
- 将当前惰性 `broker.rs` 请求形状替换为可版本化且受验证的协议。
- 在 Cargo 中新增独立 `deployment-broker` Windows binary target 和所需 Win32/WinRT feature。
- 增加 Broker 的 `requireAdministrator` manifest；Tauri 主程序 manifest 保持 `asInvoker`。
- 扩展 Tauri 命令和事件，但不让前端直接依赖 Broker DTO。
- 更新 `docs/support-matrix.md`、根 `task_plan.md`、`progress.md` 和原实现计划，使 M0/M5 边界与本规格一致。

可能受影响的模块是未来 M2 领域模型、M5 部署层和 M6 Tauri API。通过统一清单 DTO 和 `DeploymentCoordinator` 提前稳定边界，后续里程碑只消费接口，不重复实现权限逻辑。

## 11. 明确不在本规格中的内容

- 后台无人值守提权、Windows 服务或计划任务。
- 为跳过 UAC 而缓存管理员凭据。
- 远程控制 Broker 或跨机器部署。
- MSIXVC/Xbox、EXE、MSI 安装。
- 绕过 Store 许可、Flight、地区或签名策略。
- 自动删除共享框架和依赖包。
- 对机器上全部包执行批量卸载。

## 12. 完成判定

构建或单元测试通过不能单独完成 M0。最终结论必须逐项区分：

- 静态/契约验证；
- 非提权当前用户实测；
- UAC 取消实测；
- 提权全用户实测；
- 独立清单复核；
- 清理和回滚结果。

缺少任何一类证据时，只能报告对应能力仍开放，不能将“Broker 可编译”表述为“全用户部署已验收”。
