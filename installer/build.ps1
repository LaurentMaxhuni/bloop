# Builds bloop in release mode and packages dist\bloop-setup-<version>.exe.
#   powershell -ExecutionPolicy Bypass -File installer\build.ps1 [-SkipCargo]
# -SkipCargo packages the existing target\release\bloop.exe, e.g. one CI has just signed.
param([switch]$SkipCargo)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

$manifest = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if ($manifest -notmatch '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"') {
    throw 'Could not read the package version from Cargo.toml.'
}
$version = $Matches[1]

$iscc = @(
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue | ForEach-Object Source),
    (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
    (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) {
    throw 'Inno Setup 6 not found. Install it with: winget install JRSoftware.InnoSetup'
}

Push-Location $root
try {
    if (-not $SkipCargo) {
        cargo build --release --locked
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)." }
    }
    if (-not (Test-Path 'target\release\bloop.exe')) { throw 'target\release\bloop.exe is missing.' }
    & $iscc /Qp "/DAppVersion=$version" (Join-Path $PSScriptRoot 'bloop.iss')
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed ($LASTEXITCODE)." }
} finally {
    Pop-Location
}
$out = Join-Path $root "dist\bloop-setup-$version.exe"
'{0} ({1:N1} MB)' -f $out, ((Get-Item $out).Length / 1MB)
