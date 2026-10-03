[CmdletBinding()]
param(
    [string]$PackagePath = $env:M0_PACKAGE_PATH,
    [string]$PackageFullName = $env:M0_PACKAGE_FULL_NAME,
    [string]$PackageFamilyName = $env:M0_PACKAGE_FAMILY_NAME,
    [string]$CertificateThumbprint = $env:M0_CERT_THUMBPRINT,
    [string[]]$CertificateStoreLocations = @($env:M0_CERT_STORE_LOCATIONS -split ';' | Where-Object { $_ }),
    [string]$CertificateFile = $env:M0_CERT_FILE,
    [switch]$WhatIf
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$cargoManifest = Join-Path $repoRoot 'src-tauri/Cargo.toml'
$evidencePath = Join-Path $env:TEMP ("m0-deployment-evidence-{0}.json" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
$windowsPowerShell = Join-Path $env:WINDIR 'System32/WindowsPowerShell/v1.0/powershell.exe'

function Require-Input([string]$Name, [string]$Value) {
    if ([string]::IsNullOrWhiteSpace($Value)) { throw "$Name is required" }
}

function Invoke-WindowsPowerShell([string]$Command, [switch]$Elevated) {
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($Command))
    if ($Elevated) {
        $temp = Join-Path $env:TEMP ("m0-cleanup-{0}.ps1" -f [guid]::NewGuid().ToString('N'))
        Set-Content -LiteralPath $temp -Value $Command -Encoding UTF8
        try {
            $process = Start-Process -FilePath $windowsPowerShell -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $temp) -Verb RunAs -Wait -PassThru
            if ($process.ExitCode -ne 0) { throw "elevated Windows PowerShell exited with $($process.ExitCode)" }
        } finally {
            Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue
        }
    } else {
        & $windowsPowerShell -NoProfile -ExecutionPolicy Bypass -EncodedCommand $encoded
        if ($LASTEXITCODE -ne 0) { throw "Windows PowerShell exited with $LASTEXITCODE" }
    }
}

function Assert-Certificate([string]$Thumbprint, [string[]]$Stores, [string]$ExistingFile) {
    Require-Input 'M0_CERT_THUMBPRINT' $Thumbprint
    if ($Stores.Count -eq 0) { throw 'M0_CERT_STORE_LOCATIONS must list explicit certificate stores' }
    $normalized = $Thumbprint.Replace(' ', '').ToUpperInvariant()
    $found = @()
    foreach ($store in $Stores) {
        if ($store -notmatch '^Cert:\\(CurrentUser|LocalMachine)\\[^*?]+$') { throw "certificate store is not explicit: $store" }
        $found += @(Get-ChildItem -LiteralPath $store | Where-Object { $_.Thumbprint -eq $normalized })
    }
    if ($found.Count -eq 0 -and $ExistingFile) {
        if (-not (Test-Path -LiteralPath $ExistingFile -PathType Leaf)) { throw "M0_CERT_FILE does not exist" }
        if (-not $WhatIf) {
            Import-Certificate -FilePath $ExistingFile -CertStoreLocation $Stores[0] | Out-Null
            $found += @(Get-ChildItem -LiteralPath $Stores[0] | Where-Object { $_.Thumbprint -eq $normalized })
        }
    }
    if ($found.Count -eq 0 -and -not $WhatIf) { throw 'existing signing certificate was not found in the explicit stores' }
    if ($found.Count -gt 0) {
        $certificate = $found[0]
        if ($certificate.NotAfter -lt (Get-Date) -or $certificate.NotBefore -gt (Get-Date)) { throw 'certificate is outside its validity period' }
        $codeSigning = $certificate.Extensions |
            ForEach-Object { $_.EnhancedKeyUsages } |
            Where-Object { $_.Value -eq '1.3.6.1.5.5.7.3.3' }
        if (-not $codeSigning) { throw 'certificate does not contain the code-signing EKU' }
    }
    $signature = Get-AuthenticodeSignature -FilePath $PackagePath
    if ($signature.Status -ne 'Valid') { throw "package signature status is $($signature.Status)" }
    if ($signature.SignerCertificate.Thumbprint -ne $normalized) { throw 'package signer thumbprint does not match M0_CERT_THUMBPRINT' }
}

function Remove-ExactCertificate([string]$Thumbprint, [string[]]$Stores) {
    if ($WhatIf) { return }
    $normalized = $Thumbprint.Replace(' ', '').ToUpperInvariant()
    $localMachineStores = @($Stores | Where-Object { $_ -like 'Cert:\LocalMachine\*' })
    $currentUserStores = @($Stores | Where-Object { $_ -like 'Cert:\CurrentUser\*' })
    $removeCommand = @'
$thumb = '__THUMB__'
$stores = @(__STORES__)
foreach ($store in $stores) {
    Get-ChildItem -LiteralPath $store | Where-Object { $_.Thumbprint -eq $thumb } | Remove-Item -Force
}
'@
    if ($currentUserStores.Count -gt 0) {
        $command = $removeCommand.Replace('__THUMB__', $normalized).Replace('__STORES__', (($currentUserStores | ForEach-Object { "'$($_)'" }) -join ','))
        Invoke-WindowsPowerShell $command
    }
    if ($localMachineStores.Count -gt 0) {
        $command = $removeCommand.Replace('__THUMB__', $normalized).Replace('__STORES__', (($localMachineStores | ForEach-Object { "'$($_)'" }) -join ','))
        Invoke-WindowsPowerShell $command -Elevated
    }
}

function Assert-CertificateRemoved([string]$Thumbprint, [string[]]$Stores) {
    if ($WhatIf) { return }
    $normalized = $Thumbprint.Replace(' ', '').ToUpperInvariant()
    foreach ($store in $Stores) {
        if (@(Get-ChildItem -LiteralPath $store | Where-Object { $_.Thumbprint -eq $normalized }).Count -ne 0) {
            throw "certificate thumbprint remains in $store"
        }
    }
}

Require-Input 'M0_PACKAGE_PATH' $PackagePath
Require-Input 'M0_PACKAGE_FULL_NAME' $PackageFullName
Require-Input 'M0_PACKAGE_FAMILY_NAME' $PackageFamilyName
if (-not (Test-Path -LiteralPath $PackagePath -PathType Leaf)) { throw 'M0_PACKAGE_PATH does not exist' }

$isAdministrator = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdministrator -and -not $WhatIf) { throw 'run the acceptance script from an elevated process' }

$evidence = [ordered]@{
    startedAt = (Get-Date).ToUniversalTime().ToString('o')
    packageFullName = $PackageFullName
    packageFamilyName = $PackageFamilyName
    whatIf = [bool]$WhatIf
    currentUser = 'not-run'
    allUsers = 'not-run'
    certificateCleanup = 'pending'
}

try {
    Assert-Certificate $CertificateThumbprint $CertificateStoreLocations $CertificateFile
    if ($WhatIf) {
        $evidence.currentUser = 'preflight-only'
        $evidence.allUsers = 'preflight-only'
    } else {
        $env:M0_PACKAGE_PATH = $PackagePath
        $env:M0_PACKAGE_FULL_NAME = $PackageFullName
        $env:M0_PACKAGE_FAMILY_NAME = $PackageFamilyName
        $env:M0_PACKAGE_SHA256 = (Get-FileHash -LiteralPath $PackagePath -Algorithm SHA256).Hash.ToLowerInvariant()
        $env:M0_PACKAGE_IDENTITY_NAME = $env:M0_PACKAGE_IDENTITY_NAME
        $env:M0_PACKAGE_PUBLISHER = $env:M0_PACKAGE_PUBLISHER
        $env:M0_PACKAGE_VERSION = $env:M0_PACKAGE_VERSION
        $env:M0_PACKAGE_ARCHITECTURE = $env:M0_PACKAGE_ARCHITECTURE
        & cargo test --manifest-path $cargoManifest --test m0_deployment_acceptance current_user_install_and_uninstall_round_trip -- --ignored --nocapture
        if ($LASTEXITCODE -ne 0) { throw 'current-user acceptance failed' }
        $evidence.currentUser = 'passed'
        & cargo test --manifest-path $cargoManifest --test m0_deployment_acceptance all_users_stage_provision_deprovision_and_remove_round_trip -- --ignored --nocapture
        if ($LASTEXITCODE -ne 0) { throw 'all-users acceptance failed' }
        $evidence.allUsers = 'passed'
    }
} finally {
    if (-not $WhatIf) {
        try {
            $cleanup = "Remove-AppxPackage -Package '$PackageFullName' -AllUsers -ErrorAction SilentlyContinue"
            Invoke-WindowsPowerShell $cleanup -Elevated
        } catch { $evidence.packageCleanupError = $_.Exception.Message }
        try {
            Remove-ExactCertificate $CertificateThumbprint $CertificateStoreLocations
            Assert-CertificateRemoved $CertificateThumbprint $CertificateStoreLocations
            $evidence.certificateCleanup = 'passed'
        } catch {
            $evidence.certificateCleanup = 'failed'
            $evidence.certificateCleanupError = $_.Exception.Message
            throw
        }
    } else {
        $evidence.certificateCleanup = 'preflight-only'
    }
    $evidence.completedAt = (Get-Date).ToUniversalTime().ToString('o')
    $evidence | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $evidencePath -Encoding UTF8
    Write-Output "M0 evidence: $evidencePath"
}
