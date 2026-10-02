$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$targetRoot = Join-Path $repoRoot 'src-tauri/target/x86_64-pc-windows-msvc'
$candidates = @(
    (Join-Path $targetRoot 'debug/deps/deployment_broker.exe'),
    (Join-Path $targetRoot 'release/deps/deployment_broker.exe')
)
$source = $candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
if (-not $source) { throw 'deployment_broker.exe was not produced' }
$destination = Join-Path $repoRoot 'src-tauri/broker/deployment-broker-x86_64-pc-windows-msvc.exe'
Copy-Item -LiteralPath $source -Destination $destination -Force
Write-Output "Copied broker: $destination"
