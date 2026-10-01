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
