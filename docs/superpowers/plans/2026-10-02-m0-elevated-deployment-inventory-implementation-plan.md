# M0 提权部署与双层包清单实现计划

> 状态：Task 1–7 已完成并合并到提交 `157c23c`；本计划中的任务按一次最终提交交付，未按 Task 拆分提交。真实 Windows 验收使用既有自签 `.msix`，实时 Store/FE3、下载和 MSIXVC 仍不在本计划证据范围内。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** 在不提权 Tauri 主进程中完成当前用户部署，并通过一次性 UAC Broker 完成全用户部署、全用户卸载和双层包清单枚举；使用既有自签测试证书完成 Windows 验收，并在测试结束后按指纹清理证书存储。

**Architecture:** 主进程保持 asInvoker，当前用户路径直接调用 PackageManager。全用户路径由独立 deployment-broker Windows crate 通过 runas 启动，使用版本化命名管道协议、调用方校验和管理员专属暂存副本执行 stage/provision/deprovision/remove。inventory 输出统一项目 DTO。

**Tech Stack:** Rust 2021、Tauri 2.12.1、windows 0.62.2、windows-collections 0.3.2、serde/serde_json、Windows PackageManager、Win32 ShellExecuteExW/命名管道、Windows 10 build 19045 x64 验收机、PowerShell 仅作为独立验收脚本。

**Spec:** docs/superpowers/specs/2026-10-02-m0-elevated-deployment-inventory-design.md

## Global Constraints

- Tauri 主进程保持普通权限；不设置 requireAdministrator。
- 部署范围只有 CurrentUser 和 AllUsers；不实现指定其他用户安装。
- 包范围只包含 .msix、.appx、.msixbundle、.appxbundle 及经过验证的 eAppx；MSIXVC、Xbox、EXE、MSI 不进入部署路径。
- 产品二进制不启动 PowerShell、winget、Microsoft Store UI 或 Windows Update 扫描。
- 用户可写源包必须复制到 Broker 创建的管理员专属目录后再交给 WinRT。
- 测试只使用既有自签证书；不得生成新证书。清理只针对显式指纹和显式 stores，并在 finally 后复核不存在。
- 保留当前工作区已有未提交改动；禁止 reset 或覆盖无关文件。

## Review Focus

- 校验后替换：Task 3 测试句柄校验、管理员专属副本和部署顺序。
- UAC 取消/调用方不可信：Task 4 测试稳定错误和无状态变化。
- FindPackages 权限不足被错误转换为空清单：Task 2 测试 complete 与 InventoryAccessDenied。
- 全用户卸载残留：Task 3 测试 deprovision、RemoveForAllUsers 和后置清单。
- 证书清理过宽或失败仍报告通过：Task 6 测试指纹范围和 stores 复核。

---

### Task 1: 固化部署与 Broker 契约

**Files:** Create src-tauri/src/broker_protocol.rs; modify src-tauri/src/deployment.rs, src-tauri/src/lib.rs, src-tauri/Cargo.toml, src-tauri/Cargo.lock; test src-tauri/tests/m0_contract.rs.

**Interfaces:** BROKER_PROTOCOL_VERSION: u16 = 1；BrokerOperation::{InstallAllUsers, UninstallAllUsers, ScanAllUsers}；BrokerRequest/BrokerResponse；BrokerErrorCode；PackageFileRequest { path, sha256_hex, expected_identity }；AllUsersRemovalRequest { package_family_name, package_full_names }；PackageIdentity { name, publisher, version: [u16;4], architecture, resource_id }。

- [x] Write failing tests for CurrentUser/AllUsers serialization, version/operation allowlist, request ID/nonce requirements and unsupported package formats.
- [x] Run cargo test --manifest-path src-tauri/Cargo.toml --test m0_contract; expected FAIL because the versioned protocol does not exist.
- [x] Implement serde DTOs, bounded JSON frame payloads and stable deployment error mapping. Remove only the old all-users hard rejection; execution still requires Broker.
- [x] Add exact Windows features needed later: ApplicationModel, System, Win32_Foundation, Win32_Security, Win32_Security_Authorization, Win32_Storage_FileSystem, Win32_System_IO, Win32_System_Pipes, Win32_System_Threading, Win32_System_WindowsProgramming and Win32_UI_Shell.
- [x] Run cargo fmt --manifest-path src-tauri/Cargo.toml and cargo test --manifest-path src-tauri/Cargo.toml --all-targets; expected M0/M1 tests pass.
- [x] 已在最终提交 `157c23c` 中交付；未按 Task 拆分提交。

### Task 2: 实现当前用户与机器范围双层清单

**Files:** Create src-tauri/src/inventory.rs and src-tauri/tests/m0_inventory.rs; modify src-tauri/src/lib.rs and src-tauri/Cargo.toml.

**Interfaces:** InventorySource::{CurrentUser, AllUsersElevated}；InventorySnapshot { source, captured_at, os_build, complete, records, warnings }；PackageInventoryRecord with identity, PFN/full name, version, architecture, resource ID, package kind, signature/status, current-user flag, user count, other-user flag and provisioned flag；WindowsInventory::scan_current_user()；WindowsInventory::scan_all_users()。

- [x] Write failing DTO tests for four-part versions, package kind, empty resource ID, counts, provisioned state and complete=false.
- [x] Run cargo test --manifest-path src-tauri/Cargo.toml --test m0_inventory; expected FAIL because the module is absent.
- [x] Implement current-user scan using PackageManager::FindPackagesByUserSecurityId(&HSTRING::new()); never convert API failure to an empty success snapshot.
- [x] Implement elevated scan using FindPackages, FindProvisionedPackages and FindUsers; merge a current-user full-name set to populate installed_for_current_user, count only installed user states, and return counts/flags rather than raw SIDs.
- [x] Add a Windows test that a non-elevated machine scan returns InventoryAccessDenied.
- [x] 已在最终提交 `157c23c` 中交付；未按 Task 拆分提交。

### Task 3: 完成当前用户与全用户原生部署操作

**Files:** Modify src-tauri/src/deployment.rs and src-tauri/src/lib.rs; create src-tauri/src/package_validation.rs and src-tauri/tests/m0_validation.rs; extend src-tauri/tests/m0_deployment_acceptance.rs.

**Interfaces:** VerifiedPackageSet；WindowsDeploymentBackend::install_current_user(&VerifiedPackageSet)；remove_current_user(full_name)；stage_and_provision_all_users(&VerifiedPackageSet, protected_root)；deprovision_and_remove_all_users(pfn, full_names)；copy_and_verify_to_protected_root(source, root)。

- [x] Write failing tests for absolute paths, supported extensions, UNC/device paths, reparse points, hash/identity mismatch, protected-root escape and all-users uninstall postconditions.
- [x] Run cargo test --manifest-path src-tauri/Cargo.toml --test m0_validation; expected FAIL.
- [x] Implement handle-based source hashing and copy to a Broker-owned protected directory; hash and inspect the copy again before creating WinRT URIs.
- [x] Keep AddPackageAsync/RemovePackageAsync for CurrentUser and require a complete inventory postcondition.
- [x] Implement all-users stage, single-argument ProvisionPackageForAllUsersAsync, deprovision and RemovePackageWithOptionsAsync with RemovalOptions::RemoveForAllUsers; do not delete shared dependencies.
- [x] Run the existing signed-package ignored current-user round trip with M0_PACKAGE_PATH and M0_PACKAGE_FULL_NAME; pass only after install, inventory, uninstall and absence recheck.
- [x] 已在最终提交 `157c23c` 中交付；未按 Task 拆分提交。

### Task 4: 建立 UAC Broker、受限 IPC 和 manifest

**Files:** Create src-tauri/broker/Cargo.toml, src-tauri/broker/build.rs, src-tauri/broker/src/main.rs, src-tauri/broker/broker.manifest.xml, src-tauri/src/broker_launcher.rs and src-tauri/tests/m0_broker_protocol.rs; modify src-tauri/Cargo.toml workspace members and src-tauri/Cargo.lock.

**Interfaces:** BrokerLauncher::install_all_users(request)；uninstall_all_users(request)；scan_all_users(request)；Broker accepts one validated request and exits；manifest uses requireAdministrator；main app uses asInvoker。

- [x] Write failing tests for length-prefixed frames, size limits, version/nonce mismatch, parent PID mismatch, UAC cancellation and exit mapping.
- [x] Run the focused broker test; expected FAIL.
- [x] Implement named-pipe DACL for current user/Administrators/SYSTEM, random 128-bit nonce, bounded timeouts and ShellExecuteExW with runas; no automatic UAC retry.
- [x] Implement Broker high-integrity check, caller/session/signature validation and protected-copy deployment calls.
- [x] Embed the isolated manifest and assert Broker requireAdministrator and main asInvoker in build tests.
- [x] Run cargo test --manifest-path src-tauri/Cargo.toml --all-targets and cargo build --manifest-path src-tauri/broker/Cargo.toml --target x86_64-pc-windows-msvc; 已在最终提交 `157c23c` 中交付，未按 Task 拆分提交。

### Task 5: 接入 Tauri 命令、事件和打包产物

**Files:** Create src-tauri/src/deployment_coordinator.rs and src-tauri/tests/m0_coordinator.rs; modify src-tauri/src/lib.rs, src-tauri/tauri.conf.json and src-tauri/.gitignore.

**Interfaces:** DeploymentCoordinator::scan(scope)；install(scope, package_set)；uninstall(scope, target)；Tauri commands scan_installed_packages, install_package, uninstall_package。

- [x] Write failing route tests: CurrentUser never launches Broker; AllUsers always uses Broker; UAC cancel/incomplete snapshot/postcondition failures map to stable DTOs.
- [x] Implement scope routing and complete postcondition scan before reporting success.
- [x] Add external Broker binary to tauri.conf.json and ignore generated target-triple binaries.
- [x] Run cargo test --manifest-path src-tauri/Cargo.toml --all-targets, pnpm build and pnpm exec tauri build --debug --no-bundle; verify main is asInvoker and Broker is present.
- [x] 已在最终提交 `157c23c` 中交付；未按 Task 拆分提交。

### Task 6: 既有自签证书的双范围验收与存储清理

**Files:** Create scripts/m0-deployment-acceptance.ps1 and docs/evidence/m0/README.md; modify src-tauri/tests/m0_deployment_acceptance.rs and the M0 manifest fixture.

**Interfaces:** Inputs M0_PACKAGE_PATH, M0_PACKAGE_FULL_NAME, M0_PACKAGE_FAMILY_NAME, M0_CERT_THUMBPRINT and M0_CERT_STORE_LOCATIONS；redacted JSON evidence output。

- [x] Split ignored tests into CurrentUser round trip, AllUsers round trip, UAC cancellation and inventory postcondition cases; missing inputs are failures, not skipped passes.
- [x] Add -WhatIf preflight; it must not generate/import/delete certificates or packages.
- [x] Require the existing certificate; never call New-SelfSignedCertificate. Verify exact thumbprint, validity, code-signing usage and package signature match; import only an explicitly supplied existing CER/PFX if needed.
- [x] Run CurrentUser install, current-user scan, uninstall and absence check under a non-elevated process.
- [x] Run AllUsers install with manual UAC confirmation, stage/provision/machine scan, then deprovision/remove-for-all-users/current-machine-provisioned scans. Run a UAC-cancel case and assert no state change.
- [x] In PowerShell try/finally, clean the package first, then remove only the exact thumbprint from explicit stores. Prohibit broad certificate-store deletion; after finally query all stores and fail if any exact match remains.
- [x] Run the full script and archive evidence without private key, full SID, URL, token or full certificate blob; verify M0 deployment scopes and certificate cleanup.

### Task 7: 文档同步与最终回归

**Files:** Modify docs/support-matrix.md, task_plan.md, progress.md, findings.md and the existing implementation plan; include docs/evidence/m0/README.md.

- [x] Record only fresh evidence for CurrentUser, AllUsers, UAC cancel, inventory completeness, HRESULTs and certificate-store cleanup.
- [x] Update M0/M5 boundaries so Broker implementation and real acceptance are not conflated.
- [x] Run cargo fmt --manifest-path src-tauri/Cargo.toml --all, cargo test --manifest-path src-tauri/Cargo.toml --all-targets, cargo check --manifest-path src-tauri/broker/Cargo.toml, pnpm build, pnpm exec tauri build --debug --no-bundle and git diff --check.
- [x] Verify git status --short: unrelated edits remain, no certificate/private key is tracked, and evidence matches actual output.
- [x] 已在最终提交 `157c23c` 中交付；未按 Task 拆分提交。

## Dependencies and Execution Order

Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6 → Task 7.

Task 1 freezes shared types; Task 2 supplies postcondition scans; Task 3 supplies native operations; Task 4 supplies elevation; Task 5 exposes the stable Tauri surface; Task 6 mutates Windows package/certificate state; Task 7 records only fresh evidence.

## Rollback and Cleanup Rules

- Cleanup uses only the exact package full names/PFN from the same test request.
- Broker protected staging cleanup is limited to its request ID and protected root.
- Certificate cleanup is limited to the explicit thumbprint and stores; failure to prove removal fails acceptance.
- No task resets the branch, deletes broad target directories or removes broad certificate-store paths.

## 本次执行状态（2026-10-02）

- Task 1–5 已实现并通过对应 TDD/全量 Rust 测试；主进程保持普通权限，AllUsers 统一由一次性 UAC Broker 执行。
- Task 6 已用既有自签 `.msix` 完成 CurrentUser 与 AllUsers 真实回环；`-WhatIf` 预检通过；finally 包清理后，显式证书存储中的目标指纹复核为零匹配。
- Task 7 的 fresh evidence 已同步到 `task_plan.md`、`progress.md`、`findings.md`、`docs/support-matrix.md` 和 `docs/evidence/m0/README.md`。
- 按用户要求，本次不执行 Git commit；完成回归后只暂存实现/文档/测试路径，并在交付消息中给出可直接执行的 commit 语句。
