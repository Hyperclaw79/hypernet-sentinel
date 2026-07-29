$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$workspaceRoot = Split-Path -Parent $PSScriptRoot
$cargoExecutable = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"

if (-not (Test-Path -LiteralPath $cargoExecutable)) {
    throw "Rust Cargo was not found at $cargoExecutable. Install Rust with rustup first."
}

$env:SENTINEL_BIND = "127.0.0.1:8080"
$env:SENTINEL_DATA_DIR = Join-Path $workspaceRoot "data\development"
$env:SENTINEL_SCHEDULER_ENABLED = "false"
$env:RUST_LOG = "sentinel_server=info,tower_http=info"

Write-Host "Starting Hypernet Sentinel at http://127.0.0.1:8080"
Write-Host "Development data: $env:SENTINEL_DATA_DIR"
Write-Host "Automatic scheduling is disabled; dashboard-triggered tests still run normally."

& $cargoExecutable run --locked --package sentinel-server --bin sentinel-server
exit $LASTEXITCODE
