# 支持矩阵与里程碑证据记录

> 记录日期：2026-10-03
>
> 本文件记录 M0 基线、原生部署路径、M4 网络边界、M5 编排、M6 单产品真实回环和 M8 unsigned 发布工程证据；不把本地 x64 构建或该产品的 CurrentUser 实测扩展为 ARM64 工件、普遍兼容、跨渠道更新、AllUsers 实机回环或 MSIXVC 能力。

## 构建基线

| 项目 | 当前值 | 证据/备注 |
|---|---|---|
| 操作系统 | Windows 10 专业版，版本 10.0.19045，x64 | 本机 `Win32_OperatingSystem` 读取 |
| Rust | stable-x86_64-pc-windows-msvc，rustc/cargo 1.98.1 | `rust-toolchain.toml` 与命令输出 |
| Node.js | 24.13.0 | 本机命令输出 |
| pnpm | 8.15.1 | `package.json`/`pnpm-lock.yaml` 使用 pnpm |
| Tauri CLI | 2.12.1 | `pnpm exec tauri --version` |
| Tauri Rust crates | tauri 2.12.1、tauri-build 2.7.1、opener 2.7.0 | `src-tauri/Cargo.toml` 与 `Cargo.lock` 精确锁定 |
| 最低 Windows 构建 | 暂定 19045（M0 验证机） | 其他构建尚未验证，不作兼容承诺 |

## 包格式边界

| 格式 | M0 行为 | 当前结论 |
|---|---|---|
| `.msix` / `.appx` | 当前用户直接部署；全用户经 Broker stage/provision | 既有自签 `.msix` 已完成双范围真实回环 |
| `.msixbundle` / `.appxbundle` | 请求校验和 Broker 路径接受 | M6 已对 Microsoft 签名 `.msixbundle` 完成一次 CurrentUser 真实回环；AllUsers 与其他 bundle 不作普遍兼容承诺 |
| `.eappx` / `.eappxbundle` | broker 请求校验接受 | 授权和部署 API 仍待后续里程碑验证 |
| `.msixvc` / Xbox 包 | broker 请求校验拒绝 | 保持 M9 能力门，不下载、不安装、不更新 |
| `.exe` / `.msi` | broker 请求校验拒绝 | 第一阶段不执行供应商安装器 |

## 权限与部署探针

| 能力 | 状态 | 证据 |
|---|---|---|
| Windows `PackageManager` 激活 | 已验证 | `WindowsDeploymentBackend::probe()` 在 Windows 测试中通过 |
| 当前用户部署路径 | 已验收 | 非提权 Rust 测试完成安装、清单后置校验、卸载和缺失复核 |
| 全用户部署/预配 | 已验收 | 一次性 `runas` Broker 完成 UAC、管理员暂存、stage/provision、机器清单、deprovision 和 `RemoveForAllUsers` |
| PowerShell / winget 子进程 | 未使用 | M0 Rust 代码和 Tauri 配置没有 shell 调用 |
| UAC broker 实际启动 | 已验收 | Broker manifest `requireAdministrator`、命名管道 ACL/帧协议、父 PID/session/nonce/镜像路径校验和 UAC 回环通过；按 unsigned 发布决策，Release/Debug 均不要求调用方 Authenticode |

测试和桌面构建目标限定为 Windows；其他平台不属于产品支持范围。

## M4 网络、下载与缓存边界

| 能力 | 当前结论 | 证据边界 |
|---|---|---|
| production adapter | 受控在线 smoke 通过 | 2026-10-02，产品 `9WZDNCRFJ3TJ`、市场 `US`、语言 `en`；20 个包、81 条依赖；未保存临时 URL |
| disabled / custom HTTP(S) / SOCKS5 | 已实现并通过自动化测试 | 自定义凭据只在运行时存在；普通设置与 SQLite 不保存用户名/密码 |
| system 代理 | 当前用户静态代理已实现 | 读取 WinHTTP 当前用户 IE proxy config；PAC/自动检测/WPAD 显式返回不支持，不作兼容承诺 |
| 下载策略 | 本地真实 socket fixture与单产品 CDN 下载通过 | production 仅允许两个 Microsoft delivery 精确主机的默认端口 HTTP/HTTPS，逐跳复核重定向；HTTP 同样必须有期望大小与 SHA-256 |
| 续传与取消 | 本地真实字节流测试通过 | 只有 ETag 可用时续传；ETag/Content-Range/总长度变化从零重启；等待响应、限速与流读取均可取消 |
| 完整性与落盘 | 本地 fixture 通过 | 期望大小与 SHA-256 流式校验后按内容哈希原子提升；失败不进入 verified |
| 缓存恢复与淘汰 | SQLite/文件系统测试通过 | 恢复 partial sidecar，核对 verified 哈希，先 retention 后 LRU，保护活动任务且拒绝缓存根逃逸 |

M4 没有执行 Microsoft CDN 真实包下载、包签名验证、磁盘空间故障、代理服务器互操作或 Windows 部署；这些结果不能从本地 HTTP fixture 或协议 smoke 推断。

## M5 编排、身份与签名预检边界

| 能力 | 当前结论 | 证据边界 |
|---|---|---|
| verified 包图计划 | 已实现并通过自动化测试 | 只接受 M3 选择结果和 M4 `CacheState::Verified`；核对 update ID、大小、SHA-256、绝对路径并按依赖拓扑稳定排序 |
| 包身份关联 | schema v3 与纯关联逻辑已实现 | PFN 精确匹配优先，其次是已验证部署记录或唯一 identity/publisher；歧义与无法关联保持显式状态，不猜测来源 |
| 版本决策 | 严格 install/update/no-op 与防降级通过测试 | Update 要求已安装目标；目录落后返回稳定错误；不把本客户端历史当作 Windows 清单 |
| 部署前预检 | manifest、哈希与 WinTrust 检查已接入 | 普通测试覆盖拒绝路径；真实受信任签名成功路径保留为 ignored 环境门，Windows 部署仍是最终校验者 |
| 部署执行 | production adapter 唯一委托 M0 `DeploymentCoordinator` | 自动化使用端口替身验证 CurrentUser/AllUsers 路由和依赖顺序；未重复实现 Broker |
| 部署后收敛 | 已实现并通过自动化测试 | 重扫 identity/publisher/version/architecture/resource；AllUsers 主包要求显式预配，framework 允许由主包依赖关系隐式保留；成功后才事务写入关联与 `ThisClient` 来源 |

M5 最终自动化结果包含 19 项聚焦测试；工作区全目标为 110 项通过、5 项环境测试 ignored。M5 达到本地 E1 编排证据，并复用 M0 已记录的单包 CurrentUser/AllUsers E2 原语证据。本轮未取得新的真实签名 Microsoft 包图、UAC、CDN 下载或官方 Store 跨渠道更新证据，不能把两层证据拼接成 M5 的 E2/E3 验收。

## M6 后台任务、产品 UI 与真实回环

| 能力 | 当前结论 | 证据边界 |
|---|---|---|
| 持久化 worker | schema v4 与自动化回归通过 | 追加事件为权威历史，`jobs` 为可重建投影，durable command inbox 去重，固定 worker lease 使用 generation fence；投影分歧/事件缺口 fail closed |
| 安全 Tauri API | 13 个封闭命令已接入 | 只广播 `job://changed` sequence 提示；前端 DTO/事件不包含 URL、路径、凭据、原始 HRESULT 或服务响应 |
| 桌面主流程 | 五视图完成 | 搜索、详情、队列、已安装和设置；Vitest 10/10、Playwright 键盘/焦点/360 px/axe 2/2、Vite 与 Tauri debug no-bundle 构建通过 |
| production URL 策略 | HTTP/HTTPS 精确白名单 | 仅 `dl.delivery.mp.microsoft.com`、`tlu.dl.delivery.mp.microsoft.com` 默认端口；拒绝其他主机、凭据、fragment 和非默认端口；下载强制大小与 SHA-256 |
| 真实 Store CurrentUser 回环 | 已完成单产品 E2 | 2026-10-03，Windows 10 build 19045 x64，`9P7KNL5RWT25` / `US` / `en-US`，`Microsoft.SysinternalsSuite_8wekyb3d8bbwe` `2026.9.0.0`，neutral `.msixbundle`，300,193,716 字节 |
| 签名与清理 | 已验证并恢复基线 | SHA-256、bundle identity、WinTrust 和 Windows 部署通过；未导入证书、未触发 UAC；卸载后 Windows PowerShell 5.1 复核目标包计数为 0 |
| 最终自动化门 | 全部通过 | M6 记录为 Rust 171 passed / 7 ignored，严格 Clippy、Broker check、Vitest 10/10、Playwright 2/2、前端构建和 Tauri debug no-bundle 通过 |

真实回环只证明上述产品、市场、语言、时间点、主机与 CurrentUser 范围。它没有使用官方 Store 队列，不证明官方 Store 可更新本客户端安装、AllUsers、付费/授权产品、其他架构/Windows 构建或通用代理互操作；这些仍属于 M7/M8 的独立验收门。

## M8 发布工程与诊断

| 能力 | 当前结论 | 证据边界 |
|---|---|---|
| NSIS 配置 | 本地 x64 未签名构建通过 | per-user、禁止降级和 WebView2 bootstrapper 已配置；尚未在干净机验证行为 |
| 双架构构建 | x64 本地脚本通过；ARM64 CI 已配置 | 脚本复核主程序/Broker PE machine；`windows-11-arm` workflow 尚未实际运行，不能宣称已有 ARM64 工件 |
| 发布真实性 | unsigned + SHA-256 | 项目不使用代码签名证书；tag 与普通 CI 路径一致，发布时必须同时提供 checksum 和 `BUILD-METADATA.json`，Windows 可能显示未知发布者 |
| 诊断与恢复 | 封闭、默认关闭且有界 | 设置关闭时不持久化；开启后仅固定事件，64 KiB 轮转、最多导出 200 条、单实例 session marker，不含 URL/path/raw error |
| 依赖许可 | 591 条锁定图记录已生成 | `THIRD_PARTY_LICENSES.json` 不含本机路径且无 `UNKNOWN`；项目自身仍未声明分发许可证 |
| 本地 x64 工件 | 编译与独立哈希复核通过 | `0.1.0` setup 为 3,561,882 bytes，SHA-256 `bbfefdc198ebce4a234eb96971441c1756ad288d9c4e6fc3d330c66a0ff1fe89`，`signed:false`、dirty source |

M8 当前是本地 E1 工程证据，不是发布验收。ARM64 实际工件、干净 x64/ARM64 哈希复核、安装/升级/降级拒绝/卸载、AllUsers 回滚、WebView2 与可访问性矩阵仍必须完成；M7 的跨渠道 E3 也保持延期且未通过。

## M0 停止条件

- 真实验收只使用既有自签证书；脚本结束后按显式指纹清理证书存储并复核为零匹配。
- 全用户安装必须通过 Broker，主进程不得永久提权。
- 任何 `.msixvc`、Xbox、`.exe` 或 `.msi` 流程都必须停在能力门。
- broker 的 M0 `validate()` 只校验请求形状，不能作为提权授权；实际 IPC/UAC 路径保留父 PID/session/nonce/镜像路径、可信暂存目录、重解析点防护、包身份/签名/哈希验证和安全文件打开。发布构建不得用 Debug Broker 替代 Release Broker。
