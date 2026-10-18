param(
    [switch]$Lite
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
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "npm not found. Install Node.js first."
}

$version = (Get-Content (Join-Path $root "package.json") | ConvertFrom-Json).version
$suffix = if ($Lite) { "lite" } else { "offline" }

Write-Host "Building CC DiskSage $version ($suffix)..."
npm install
npm run build:helper:release
if ($Lite) {
    Write-Host "Lite build expects WebView2 to already be installed on the target machine."
    npm run tauri build -- --config src-tauri/tauri.lite.conf.json
} else {
    npm run tauri build
}

$bundle = Join-Path $root "src-tauri\target\release\bundle\nsis"
$output = Join-Path $root "dist-installers"
New-Item -ItemType Directory -Force -Path $output | Out-Null

$artifacts = Get-ChildItem $bundle -Filter "*.exe" -ErrorAction Stop
if (-not $artifacts) {
    throw "No NSIS installer found in $bundle"
}
foreach ($artifact in $artifacts) {
    $target = Join-Path $output "cc-disksage-$version-$suffix-setup.exe"
    Copy-Item $artifact.FullName $target -Force
    Write-Host "Installer: $target"
}
