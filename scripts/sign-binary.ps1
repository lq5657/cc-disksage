param(
    [Parameter(Mandatory = $true)]
    [string]$Path,
    [switch]$NoTimestamp
)

$ErrorActionPreference = "Stop"

function Find-SignTool {
    $command = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }

    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    if (Test-Path $kits) {
        $candidate = Get-ChildItem $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($candidate) { return $candidate.FullName }
    }
    throw "signtool.exe not found. Install the Windows SDK or add signtool to PATH."
}

if (-not (Test-Path -LiteralPath $Path)) {
    throw "File not found: $Path"
}

$signTool = Find-SignTool
$pfx = $env:CC_DISKSAGE_SIGN_PFX
$pfxPassword = $env:CC_DISKSAGE_SIGN_PFX_PASSWORD
$thumbprint = $env:CC_DISKSAGE_SIGN_THUMBPRINT
$timestampUrl = if ($env:CC_DISKSAGE_SIGN_TIMESTAMP_URL) {
    $env:CC_DISKSAGE_SIGN_TIMESTAMP_URL
} else {
    "http://timestamp.digicert.com"
}

if (-not $pfx -and -not $thumbprint) {
    throw "Set CC_DISKSAGE_SIGN_PFX (+ optional CC_DISKSAGE_SIGN_PFX_PASSWORD) or CC_DISKSAGE_SIGN_THUMBPRINT before signing."
}

$arguments = @(
    "sign",
    "/fd", "SHA256",
    "/d", "CC DiskSage"
)
if ($pfx) {
    $arguments += @("/f", $pfx)
    if ($pfxPassword) {
        $arguments += @("/p", $pfxPassword)
    }
} else {
    $arguments += @("/sha1", $thumbprint)
}
if (-not $NoTimestamp -and $timestampUrl -and $timestampUrl -ne "none") {
    $arguments += @("/tr", $timestampUrl, "/td", "SHA256")
}
$arguments += $Path

& $signTool @arguments
if ($LASTEXITCODE -ne 0) {
    throw "signtool sign failed for $Path (exit $LASTEXITCODE)"
}

if ($env:CC_DISKSAGE_SKIP_VERIFY -eq "1") {
    Write-Host "Signed (verification skipped by CC_DISKSAGE_SKIP_VERIFY=1): $Path"
} else {
    & $signTool verify /pa /v $Path
    if ($LASTEXITCODE -ne 0) {
        throw "signtool verify failed for $Path (exit $LASTEXITCODE)"
    }
    Write-Host "Signed and verified: $Path"
}
