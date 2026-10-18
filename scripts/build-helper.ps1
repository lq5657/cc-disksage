param(
    [switch]$Release
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

$profile = if ($Release) { "release" } else { "debug" }
if ($Release) {
    cargo build --release -p disksage-helper
} else {
    cargo build -p disksage-helper
}

$source = Join-Path $root "target\$profile\disksage-helper.exe"
if (-not (Test-Path $source)) {
    throw "helper binary not found at $source"
}

$binaries = Join-Path $root "src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $binaries | Out-Null
$target = Join-Path $binaries "disksage-helper-x86_64-pc-windows-msvc.exe"
Copy-Item $source $target -Force
Write-Host "Helper ready: $target"
