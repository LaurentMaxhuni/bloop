# Starts a release: tags the current main as v<version> (from Cargo.toml) and pushes the tag.
# GitHub Actions (.github/workflows/release.yml) then builds, signs when configured, and
# publishes the release that bloop's updater picks up.
#   powershell -ExecutionPolicy Bypass -File installer\release.ps1 -Notes "What changed"
# -Local builds on this PC and publishes directly instead (unsigned; the old route, for when
# Actions is unavailable). Needs the GitHub CLI signed in once: gh auth login
param([string]$Notes = '', [switch]$Local)
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
    if (-not $Notes) { $Notes = "bloop $version" }

    if ($Local) {
        & (Join-Path $PSScriptRoot 'build.ps1')
        $name = "bloop-setup-$version.exe"
        $exe = Join-Path $root "dist\$name"
        $hash = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLowerInvariant()
        $sha = "$exe.sha256"
        # sha256sum format, LF line ending: "<hex>  <file name>"
        [System.IO.File]::WriteAllText($sha, "$hash  $name`n")
        gh release create $tag $exe $sha --repo $repo --title "bloop $version" --notes $Notes --target (git rev-parse HEAD)
        if ($LASTEXITCODE -ne 0) { throw "gh release create failed ($LASTEXITCODE)." }
        "Published $tag with $name ($hash), built locally and unsigned."
        return
    }

    # Release only what is on GitHub's main, so the tag points at a commit CI can build.
    git fetch --quiet origin main
    if ($LASTEXITCODE -ne 0) { throw 'git fetch failed.' }
    $head = git rev-parse HEAD
    if ($head -ne (git rev-parse origin/main)) {
        throw 'HEAD is not origin/main. Merge your changes to main, pull, then release.'
    }
    if (git tag --list $tag) { throw "Tag $tag already exists locally. Delete it or bump the version." }
    # The annotated tag's message becomes the release notes.
    git tag --annotate $tag --message $Notes $head
    if ($LASTEXITCODE -ne 0) { throw 'git tag failed.' }
    git push origin $tag
    if ($LASTEXITCODE -ne 0) {
        git tag --delete $tag | Out-Null
        throw 'git push of the tag failed.'
    }
    "Pushed $tag. GitHub Actions is building the release: https://github.com/$repo/actions/workflows/release.yml"
} finally {
    Pop-Location
}
