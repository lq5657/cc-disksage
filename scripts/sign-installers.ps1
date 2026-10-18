param(
    [string]$Directory = "dist-installers",
    [switch]$NoTimestamp
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$target = if ([System.IO.Path]::IsPathRooted($Directory)) {
    $Directory
} else {
    Join-Path $root $Directory
}

$installers = Get-ChildItem $target -Filter "cc-disksage-*-setup.exe" -ErrorAction Stop
if (-not $installers) {
    throw "No installers found in $target"
}

$signScript = Join-Path $PSScriptRoot "sign-binary.ps1"
foreach ($installer in $installers) {
    if ($NoTimestamp) {
        & $signScript -Path $installer.FullName -NoTimestamp
    } else {
        & $signScript -Path $installer.FullName
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Signing failed for $($installer.FullName)"
    }
}
