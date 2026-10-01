# M0 支持矩阵与部署 Spike 记录

> 记录日期：2026-10-01
>
> 本文件只记录 M0 基线和已执行的部署 API 探针，不把未执行的真实包安装写成已支持。

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
| `.msix` / `.appx` | broker 请求校验接受 | 仅证明请求边界，未执行真实安装 |
| `.msixbundle` / `.appxbundle` | broker 请求校验接受 | 仅证明请求边界，未执行真实安装 |
| `.eappx` / `.eappxbundle` | broker 请求校验接受 | 授权和部署 API 仍待后续里程碑验证 |
| `.msixvc` / Xbox 包 | broker 请求校验拒绝 | 保持 M9 能力门，不下载、不安装、不更新 |
| `.exe` / `.msi` | broker 请求校验拒绝 | 第一阶段不执行供应商安装器 |

## 权限与部署探针

| 能力 | 状态 | 证据 |
|---|---|---|
| Windows `PackageManager` 激活 | 已验证 | `WindowsDeploymentBackend::probe()` 在 Windows 测试中通过 |
| 当前用户部署路径 | API 可用，真实安装未验证 | 探针返回 `Available`；当前没有批准的测试包载荷 |
| 全用户部署/预配 | `RequiresElevation` | 探针明确返回提权门；broker 目前只有拒绝 `AllUsers` 的惰性请求形状原型 |
| PowerShell / winget 子进程 | 未使用 | M0 Rust 代码和 Tauri 配置没有 shell 调用 |
| UAC broker 实际启动 | 未验证 | 需要独立签名 broker、IPC 和授权测试，留到 M5 前置验证 |

测试和桌面构建目标限定为 Windows；其他平台不属于产品支持范围。

## M0 停止条件

- 没有测试包载荷时，不运行真实安装/卸载，不伪造安装成功证据。
- 全用户安装在 broker/UAC Spike 通过前保持条件性状态。
- 任何 `.msixvc`、Xbox、`.exe` 或 `.msi` 流程都必须停在能力门。
- broker 的 M0 `validate()` 只校验请求形状，不能作为提权授权；IPC/UAC 前必须补可信暂存目录、重解析点防护、身份/签名/哈希验证和安全文件打开。
