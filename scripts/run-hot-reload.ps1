$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$workspaceRoot = Split-Path -Parent $PSScriptRoot
$cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
$cargoExecutable = Join-Path $cargoBin "cargo.exe"
$watchexecExecutable = Join-Path $cargoBin "watchexec.exe"
$nodeExecutable = (Get-Command node.exe -ErrorAction SilentlyContinue).Source

if (-not (Test-Path -LiteralPath $cargoExecutable)) {
    throw "Rust Cargo was not found at $cargoExecutable. Install Rust with rustup first."
}
if (-not (Test-Path -LiteralPath $watchexecExecutable)) {
    throw "watchexec was not found at $watchexecExecutable. Run: cargo install --locked watchexec-cli"
}
if (-not $nodeExecutable) {
    throw "Node.js was not found. Install Node.js 22 or newer first."
}

$env:PATH = "$cargoBin;$env:PATH"
$env:SENTINEL_BIND = "127.0.0.1:8080"
$env:SENTINEL_DATA_DIR = Join-Path $workspaceRoot "data\development"
$env:SENTINEL_SCHEDULER_ENABLED = "false"
$env:SENTINEL_WEB_DIR = Join-Path $workspaceRoot "web"
$env:SENTINEL_WATCHEXEC = $watchexecExecutable
$env:SENTINEL_CARGO = $cargoExecutable
$env:SENTINEL_LIVE_RELOAD_PORT = "3000"
$env:RUST_LOG = "sentinel_server=info,tower_http=info"

Write-Host "Starting Hypernet Sentinel development server at http://127.0.0.1:8080"
Write-Host "Live-reload dashboard: http://127.0.0.1:3000"
Write-Host "Rust changes rebuild and restart the server; web changes reload the browser."

Push-Location $workspaceRoot
try {
    & $nodeExecutable (Join-Path $PSScriptRoot "live-reload.mjs")
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
