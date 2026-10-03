param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$TargetTriple,

    [Parameter(Mandatory = $true)]
    [ValidateSet('debug', 'release')]
    [string]$Profile
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$source = Join-Path $repoRoot "src-tauri/target/$TargetTriple/$Profile/deployment-broker.exe"
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
    throw "Broker was not produced at the expected path: $source"
}
$destination = Join-Path $repoRoot "src-tauri/broker/deployment-broker-$TargetTriple.exe"
Copy-Item -LiteralPath $source -Destination $destination -Force
Write-Output "Copied broker: $destination"
