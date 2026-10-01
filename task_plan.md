# Yet Another Microsoft Store 架构与实现计划

## 目标

设计一个以 Rust + Tauri 为基本技术栈、通过 Microsoft 官方交付端点直连获取包的第三方 Microsoft Store 客户端。

## 当前状态

- 范围与架构：已完成
- 证据评审：已完成
- 中文规格文档：已完成，已批准进入 M0
- 中文实现计划与里程碑：已完成，已批准进入 M0
- M0：基线与部署 API 探针完成；真实部署验收开放（等待批准的测试包载荷/UAC broker Spike）
- 产品代码：已建立 Tauri/Rust/React 基线，后续模块尚未开始

## 已确认决策

- 采用直连 DCAT/FE3 路径。
- 使用 Tauri 2、Vite + React + TypeScript 和 shadcn/ui。
- 隔离 `storelib_rs` 适配器。
- 支持 x64、ARM64、x86，多市场以及多语言/资源包。
- 支持禁用、系统、自定义 HTTP(S) 和 SOCKS5 代理模式。
- 支持配置缓存目录和保留策略。
- 使用 NSIS 分发。
- 普通 MSIX/AppX 支持全用户安装。
- 安装来源只作观测元数据，使官方 Store 和本客户端可以看到同一包身份与版本。
- MSIXVC 作为目标能力，但 Xbox 包延后到第一阶段之后。
- EXE 和 MSI 延后到第一阶段之后。
- 官方 Store 与第三方客户端的互操作按“身份/授权兼容”处理，不承诺任一客户端控制另一方更新队列。

## 下一步门槛

1. 补齐经批准的测试包载荷后，验证当前用户安装/卸载并完成 M0 部署验收。
2. M0 部署验收后进入 M1 Store 协议适配。

## 约束

- 将获取、选择、下载、验证和部署放在相互独立的 Rust 接口之后。
- 将 DCAT/FE3 视为不稳定的外部契约，固定协议 fixture 并测试。
- 将跨渠道 Store 互操作视为身份/授权兼容性，不承诺任一客户端控制另一方更新队列。
- 在专门部署要求验证前，绝不宣称支持 MSIXVC 安装。
- 外部网页研究结果只写入 `findings.md`。

## 里程碑索引

- M0：基线与部署 Spike
- M1：Store 协议适配
- M2：领域模型与持久化
- M3：适用性与资源选择
- M4：下载、缓存与代理
- M5：原生部署与跨渠道清单
- M6：Tauri API 与前端主流程
- M7：更新与互操作验证
- M8：NSIS 与发布加固
- M9：MSIXVC 研究门（后续）

详细文件级任务见 `docs/superpowers/plans/2026-10-01-third-party-store-client-implementation-plan.md`。
