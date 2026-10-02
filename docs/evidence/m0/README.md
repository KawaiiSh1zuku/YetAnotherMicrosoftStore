# M0 部署验收证据

M0 验收覆盖两条部署范围：

- `CurrentUser`：非提权主进程直接调用 `PackageManager.AddPackageAsync` / `RemovePackageAsync`，并以当前用户包清单完成后置校验。
- `AllUsers`：主进程通过一次性 `runas` UAC Broker，使用受限长度前缀命名管道传递请求；Broker 在管理员专属暂存目录中重新校验路径、SHA-256 和包身份后执行 stage/provision/deprovision/`RemoveForAllUsers`，再以机器范围清单完成后置校验。
- Broker 在接收请求前校验管道服务端 PID、会话、Nonce 和父进程镜像路径；Release Broker 对父进程执行 Authenticode 验证。Debug 验收使用未签名的测试宿主，故只启用镜像路径校验。

## 测试证书边界

验收脚本 `scripts/m0-deployment-acceptance.ps1` 只接受既有证书的显式指纹、显式证书存储和可选的既有 `.cer`/`.pfx` 文件。脚本不会调用 `New-SelfSignedCertificate`，不会删除通配证书，也不会清理未列出的存储。`try/finally` 顺序固定为：包清理 → 精确指纹证书清理 → 存储复核；复核失败时验收失败。

推荐命令（从非提权 PowerShell 运行，使用之前的自签证书）：

```powershell
$env:M0_CERT_THUMBPRINT = '<existing-thumbprint>'
$env:M0_CERT_STORE_LOCATIONS = 'Cert:\CurrentUser\Root;Cert:\CurrentUser\TrustedPeople;Cert:\LocalMachine\Root'
pwsh -File .\scripts\m0-deployment-acceptance.ps1
```

预检不会导入、安装或删除任何对象：

```powershell
pwsh -File .\scripts\m0-deployment-acceptance.ps1 -WhatIf
```

证据文件只写入临时目录；提交前不得把私钥、完整 SID、访问令牌、完整证书对象或未脱敏日志加入仓库。当前实现的可复核命令和结果应记录在 `task_plan.md`、`progress.md` 与 `findings.md`，不要把生成的包、证书和 Broker 二进制复制进 `docs/evidence/m0/`。
