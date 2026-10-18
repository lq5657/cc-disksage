param(
    [switch]$InstallConfig
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:Path = "$HOME\.cargo\bin;$env:Path"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo not found. Install Rust (MSVC) first."
}

Write-Host "Vendoring Rust crates into vendor/ ..."
New-Item -ItemType Directory -Force -Path (Join-Path $root "vendor") | Out-Null
cargo vendor vendor | Set-Content (Join-Path $root "vendor\cargo-config.toml") -Encoding utf8

if ($InstallConfig) {
    New-Item -ItemType Directory -Force -Path (Join-Path $root ".cargo") | Out-Null
    Copy-Item (Join-Path $root "vendor\cargo-config.toml") (Join-Path $root ".cargo\config.toml") -Force
    Write-Host "Installed .cargo/config.toml source replacement."
} else {
    Write-Host "Generated vendor\cargo-config.toml. Copy it to .cargo\config.toml for offline builds."
}

Write-Host ""
Write-Host "Remaining build-machine caches to prepare while online:"
Write-Host "  1. npm: run 'npm ci' once, or use 'npm ci --offline' with a warm npm cache."
Write-Host "  2. Tauri/NSIS: run one full 'npm run tauri build' to cache NSIS and the WebView2 installer."
Write-Host "  3. Windows SDK / MSVC Build Tools: install the offline layout on the build machine."
Write-Host ""
Write-Host "Target user machines need none of the above; the installer ships compiled binaries only."
