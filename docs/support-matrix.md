# M0 支持矩阵与部署 Spike 记录

> 记录日期：2026-10-02
>
> 本文件记录 M0 基线、原生部署路径和已执行的双范围验收证据；不把 fixture/协议测试扩展为线上 Store 或 MSIXVC 能力。

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

## M0 停止条件

- 真实验收只使用既有自签证书；脚本结束后按显式指纹清理证书存储并复核为零匹配。
- 全用户安装必须通过 Broker，主进程不得永久提权。
- 任何 `.msixvc`、Xbox、`.exe` 或 `.msi` 流程都必须停在能力门。
- broker 的 M0 `validate()` 只校验请求形状，不能作为提权授权；实际 IPC/UAC 路径已补可信暂存目录、重解析点防护、身份/签名/哈希验证和安全文件打开。发布构建不得用 Debug Broker 替代 Release Broker。
