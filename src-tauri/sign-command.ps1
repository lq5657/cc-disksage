$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
try {
    & (Join-Path $root "scripts\sign-binary.ps1") @args
    exit $LASTEXITCODE
} catch {
    Write-Error $_.Exception.Message
    exit 1
}
