param(
    [ValidateSet('x64', 'arm64', 'all')]
    [string]$Architecture = 'all',
    [switch]$SkipChecks
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path -Parent $PSScriptRoot
$config = Get-Content -LiteralPath (Join-Path $repoRoot 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$targetByArchitecture = @{
    x64 = 'x86_64-pc-windows-msvc'
    arm64 = 'aarch64-pc-windows-msvc'
}

function Invoke-Checked([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed with exit code $LASTEXITCODE" }
}

function Get-Sha256([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $algorithm = [System.Security.Cryptography.SHA256]::Create()
        try {
            return ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
        } finally { $algorithm.Dispose() }
    } finally { $stream.Dispose() }
}

function Write-Utf8NoBom([string]$Path, [string]$Value) {
    $encoding = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($Path, $Value, $encoding)
}

function Get-PeMachine([string]$Path) {
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $reader = [System.IO.BinaryReader]::new($stream)
        $stream.Position = 0x3c
        $peOffset = $reader.ReadInt32()
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) { throw "Not a PE image: $Path" }
        return $reader.ReadUInt16()
    } finally { $stream.Dispose() }
}

function Assert-TargetMachine([string]$Path, [string]$Arch) {
    $expected = if ($Arch -eq 'x64') { 0x8664 } else { 0xaa64 }
    $actual = Get-PeMachine $Path
    if ($actual -ne $expected) { throw "Unexpected PE machine 0x$($actual.ToString('x4')) for $Path" }
}

function Find-ManifestTool {
    $command = Get-Command mt.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
    $candidate = Get-ChildItem -LiteralPath $kits -Filter mt.exe -File -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\mt\.exe$' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $candidate) { throw 'Windows manifest tool mt.exe was not found' }
    return $candidate.FullName
}

function Assert-RequireAdministrator([string]$Path) {
    $manifestPath = Join-Path $env:TEMP ("yamstore-manifest-{0}.xml" -f [guid]::NewGuid().ToString('N'))
    try {
        Invoke-Checked (Find-ManifestTool) @('-nologo', "-inputresource:$Path;#1", "-out:$manifestPath")
        $manifest = Get-Content -LiteralPath $manifestPath -Raw
        if ($manifest -notmatch 'requestedExecutionLevel\s+level="requireAdministrator"\s+uiAccess="false"') {
            throw "Main executable does not require administrator privileges: $Path"
        }
    } finally {
        Remove-Item -LiteralPath $manifestPath -Force -ErrorAction SilentlyContinue
    }
}

function Build-Architecture([string]$Arch) {
    $target = $targetByArchitecture[$Arch]
    $installedTargets = & rustup target list --installed
    if ($installedTargets -notcontains $target) {
        throw "Rust target $target is not installed. Run: rustup target add $target"
    }

    $previousTauriConfig = $env:TAURI_CONFIG
    try {
        Remove-Item Env:TAURI_CONFIG -ErrorAction SilentlyContinue
        Write-Warning "Building unsigned $Arch artifact. Windows may show an unknown-publisher warning."
        $buildStarted = (Get-Date).AddSeconds(-2)
        Invoke-Checked pnpm @('exec', 'tauri', 'build', '--target', $target, '--bundles', 'nsis')
    } finally {
        if ($null -eq $previousTauriConfig) { Remove-Item Env:TAURI_CONFIG -ErrorAction SilentlyContinue }
        else { $env:TAURI_CONFIG = $previousTauriConfig }
    }

    $mainExecutable = Join-Path $repoRoot "src-tauri/target/$target/release/yet-another-microsoft-store.exe"
    Assert-TargetMachine $mainExecutable $Arch
    Assert-RequireAdministrator $mainExecutable
    $bundleRoot = Join-Path $repoRoot "src-tauri/target/$target/release/bundle/nsis"
    $brokerArtifacts = @(Get-ChildItem -LiteralPath $bundleRoot -Recurse -File |
        Where-Object { $_.Name -match 'broker' })
    if ($brokerArtifacts.Count -ne 0) { throw 'Release bundle contains a Broker artifact' }
    $installers = @(Get-ChildItem -LiteralPath $bundleRoot -Filter '*-setup.exe' -File |
        Where-Object { $_.LastWriteTime -ge $buildStarted })
    if ($installers.Count -ne 1) { throw "Expected one fresh NSIS installer for $Arch, found $($installers.Count)" }
    $installer = $installers[0]

    $artifactRoot = Join-Path $repoRoot "release-artifacts/$Arch"
    if (Test-Path -LiteralPath $artifactRoot) {
        Remove-Item -LiteralPath $artifactRoot -Recurse -Force
    }
    New-Item -ItemType Directory -Path $artifactRoot -Force | Out-Null
    Copy-Item -LiteralPath $installer.FullName -Destination $artifactRoot -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot 'src-tauri/resources/THIRD_PARTY_LICENSES.json') -Destination $artifactRoot -Force
    $artifact = Join-Path $artifactRoot $installer.Name
    $hash = Get-Sha256 $artifact
    Write-Utf8NoBom (Join-Path $artifactRoot 'SHA256SUMS.txt') "$hash  $($installer.Name)`n"
    $metadata = [ordered]@{
        schemaVersion = 1
        version = $config.version
        architecture = $Arch
        target = $target
        signed = $false
        installer = $installer.Name
        sha256 = $hash
        commit = (& git rev-parse HEAD).Trim()
        dirty = [bool](& git status --porcelain --untracked-files=no)
    }
    Write-Utf8NoBom (Join-Path $artifactRoot 'BUILD-METADATA.json') "$(ConvertTo-Json $metadata)`n"
    Write-Output "Release artifact ready: $artifact"
}

Push-Location $repoRoot
try {
    if (-not $SkipChecks) {
        Invoke-Checked pnpm @('test')
        Invoke-Checked pnpm @('build')
        Invoke-Checked cargo @('test', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets')
    }
    Invoke-Checked pnpm @('run', 'generate:licenses')
    $architectures = if ($Architecture -eq 'all') { @('x64', 'arm64') } else { @($Architecture) }
    foreach ($arch in $architectures) { Build-Architecture $arch }
} finally { Pop-Location }
