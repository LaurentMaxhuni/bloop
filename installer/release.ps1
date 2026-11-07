# Builds the installer and publishes it as a GitHub release that bloop's updater picks up.
#   powershell -ExecutionPolicy Bypass -File installer\release.ps1 [-Notes "What changed"]
# Needs the GitHub CLI signed in once: gh auth login
param([string]$Notes = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$repo = 'LaurentMaxhuni/bloop'

# Runs gh quietly and reports success; gh's own error text is not a script failure.
function Test-Gh([string[]]$Arguments) {
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & gh @Arguments *> $null; return $LASTEXITCODE -eq 0 } finally { $ErrorActionPreference = $previous }
}

Push-Location $root
try {
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
        throw 'GitHub CLI not found. Install it with: winget install GitHub.cli'
    }
    if (-not (Test-Gh @('auth', 'status'))) { throw 'Not signed in to GitHub. Run `gh auth login` once.' }
    if (git status --porcelain) { throw 'Commit or stash your changes first; releases are built from a clean tree.' }

    $manifest = Get-Content (Join-Path $root 'Cargo.toml') -Raw
    if ($manifest -notmatch '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"') {
        throw 'Could not read the package version from Cargo.toml.'
    }
    $version = $Matches[1]
    $tag = "v$version"
    if (Test-Gh @('release', 'view', $tag, '--repo', $repo)) {
        throw "Release $tag already exists. Bump the version in Cargo.toml first."
    }

    & (Join-Path $PSScriptRoot 'build.ps1')
    $name = "bloop-setup-$version.exe"
    $exe = Join-Path $root "dist\$name"
    $hash = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    $sha = "$exe.sha256"
    # sha256sum format, LF line ending: "<hex>  <file name>"
    [System.IO.File]::WriteAllText($sha, "$hash  $name`n")

    if (-not $Notes) { $Notes = "bloop $version" }
    gh release create $tag $exe $sha --repo $repo --title "bloop $version" --notes $Notes --target (git rev-parse HEAD)
    if ($LASTEXITCODE -ne 0) { throw "gh release create failed ($LASTEXITCODE)." }
    "Published $tag with $name ($hash)"
} finally {
    Pop-Location
}
