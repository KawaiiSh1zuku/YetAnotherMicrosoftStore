# 支持矩阵与里程碑证据记录

> 记录日期：2026-10-02
>
> 本文件记录 M0 基线、原生部署路径和 M4 网络边界证据；不把 fixture/协议 smoke 扩展为真实 CDN 下载、跨渠道更新或 MSIXVC 能力。

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
| `.msixbundle` / `.appxbundle` | 请求校验和 Broker 路径接受 | 真实 bundle 载荷未在本次 M0 验收 |
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
| UAC broker 实际启动 | 已验收 | Broker manifest `requireAdministrator`、命名管道 ACL/帧协议、父 PID/session/nonce/镜像路径校验和 UAC 回环通过；Release Broker 额外执行 Authenticode 校验，Debug 验收允许未签名测试宿主 |

测试和桌面构建目标限定为 Windows；其他平台不属于产品支持范围。

## M4 网络、下载与缓存边界

| 能力 | 当前结论 | 证据边界 |
|---|---|---|
| production adapter | 受控在线 smoke 通过 | 2026-10-02，产品 `9WZDNCRFJ3TJ`、市场 `US`、语言 `en`；20 个包、81 条依赖；未保存临时 URL |
| disabled / custom HTTP(S) / SOCKS5 | 已实现并通过自动化测试 | 自定义凭据只在运行时存在；普通设置与 SQLite 不保存用户名/密码 |
| system 代理 | 当前用户静态代理已实现 | 读取 WinHTTP 当前用户 IE proxy config；PAC/自动检测/WPAD 显式返回不支持，不作兼容承诺 |
| 下载策略 | 本地真实 socket fixture 通过 | HTTPS host allowlist 与逐跳重定向复核；只有显式测试策略允许 loopback HTTP |
| 续传与取消 | 本地真实字节流测试通过 | 只有 ETag 可用时续传；ETag/Content-Range/总长度变化从零重启；等待响应、限速与流读取均可取消 |
| 完整性与落盘 | 本地 fixture 通过 | 期望大小与 SHA-256 流式校验后按内容哈希原子提升；失败不进入 verified |
| 缓存恢复与淘汰 | SQLite/文件系统测试通过 | 恢复 partial sidecar，核对 verified 哈希，先 retention 后 LRU，保护活动任务且拒绝缓存根逃逸 |

M4 没有执行 Microsoft CDN 真实包下载、包签名验证、磁盘空间故障、代理服务器互操作或 Windows 部署；这些结果不能从本地 HTTP fixture 或协议 smoke 推断。

## M0 停止条件

- 真实验收只使用既有自签证书；脚本结束后按显式指纹清理证书存储并复核为零匹配。
- 全用户安装必须通过 Broker，主进程不得永久提权。
- 任何 `.msixvc`、Xbox、`.exe` 或 `.msi` 流程都必须停在能力门。
- broker 的 M0 `validate()` 只校验请求形状，不能作为提权授权；实际 IPC/UAC 路径已补可信暂存目录、重解析点防护、身份/签名/哈希验证和安全文件打开。发布构建不得用 Debug Broker 替代 Release Broker。
