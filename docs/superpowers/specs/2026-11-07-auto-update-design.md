# Auto-update — design

Date: 2026-11-07 · Status: awaiting review

## Goal

bloop keeps itself current from GitHub Releases: it finds a newer version, downloads it in
the background, checks it, and installs it with one click ("restart to update"). Publishing a
new version is one script on the developer's PC.

Success: after `installer/release.ps1` publishes 0.3.0, a running 0.2.x bloop peeks
"Update 0.3.0 ready — click to restart" within its next check; one click closes bloop, the
installer runs silently with the user's previous choices, and 0.3.0 opens with chats and
settings intact.

Out of scope: code signing, rollback, channels/pre-releases, delta updates, updating while a
reply streams, non-Windows platforms.

## Publishing (developer)

- `installer/release.ps1`: runs `build.ps1`, writes `dist/bloop-setup-<v>.exe.sha256`
  (lower-case hex SHA-256, then two spaces and the file name), and runs
  `gh release create v<v> <exe> <sha256> --repo LaurentMaxhuni/bloop --title "bloop <v>"
  --notes-file <notes>` (notes from an optional `-Notes` argument, else "bloop <v>").
- Refuses to publish when the tag already exists, when the working tree is dirty, or when
  `gh auth status` fails ("Run `gh auth login` once"). Needs `gh` signed in once.

## Checking (app)

- `src/update.rs` (library): `latest(current: &Version) -> Result<Option<Release>, Error>`.
  - `GET https://api.github.com/repos/LaurentMaxhuni/bloop/releases/latest`, header
    `User-Agent: bloop-updater` and `Accept: application/vnd.github+json`, no auth, 10 s timeout,
    response capped at 256 KiB.
  - Ignores `draft`/`prerelease`. Parses `tag_name` `vX.Y.Z` (or `X.Y.Z`); newer only when
    greater than `CARGO_PKG_VERSION` by numeric (major, minor, patch).
  - Needs both assets `bloop-setup-X.Y.Z.exe` and `bloop-setup-X.Y.Z.exe.sha256`; otherwise
    "no update". Asset `size` must be ≤ 50 MiB.
- Schedule: first check 30 s after start, then every 6 h, while `config.auto_update` is on
  (default on). "Check now" in the hub runs one immediately regardless.
- Errors (offline, rate limit, bad JSON) are silent for scheduled checks and shown as a short
  status line for "Check now".

## Downloading

- To `%LOCALAPPDATA%\bloop\updates\bloop-setup-X.Y.Z.exe.part`, streamed, capped at the
  declared size and 50 MiB, 10 min overall timeout, then the `.sha256` asset (≤ 1 KiB).
- Redirects allowed only to `github.com` and `*.githubusercontent.com` over HTTPS.
- SHA-256 of the file must equal the published value, otherwise the file is deleted and the
  next check retries. On match it is renamed to `.exe`; older files in `updates\` are removed.
- At start, an already downloaded, verified installer for a newer version is offered without
  re-downloading (its hash is re-checked).

## Installing

- When ready, bloop peeks "Update X.Y.Z ready — click to restart" (not while a reply streams;
  then it waits until the reply finishes). The pill keeps that message as its status until
  the user acts or quits. Clicking it, or "Restart to update" in the hub, installs.
- Install: save state, start the installer with
  `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /RELAUNCH=1`, then quit bloop. The installer
  waits for bloop's mutex to be released (`AppMutex`), keeps the previously chosen tasks
  (Inno's `UsePreviousTasks`), and its `[Run]` entry starts bloop again when `/RELAUNCH=1`
  is passed (also in silent mode).
- If the installer fails to start, bloop stays open and shows the error.

## Hub

The Model & shortcut page gains an **Updates** section: "bloop X.Y.Z", status line (Up to
date · Checking… · Downloading 0.3.0 (42 %) · 0.3.0 ready · last error), "Check now",
"Restart to update" (when ready), and a "Check for updates automatically" switch
(`config.auto_update`, default true, saved immediately).

## Architecture

- `src/update.rs` (library): `Version`, `Release { version, installer_url, checksum_url, size }`,
  `latest`, `download(release, dir, progress) -> Result<PathBuf, Error>`, `verify(path, hex)`,
  `ready(dir, current) -> Option<(Version, PathBuf)>`. HTTP through `reqwest` with the redirect
  policy above; SHA-256 via the `sha2` crate.
- `src/ui.rs`: `Message::UpdateChecked`, `UpdateProgress`, `UpdateReady`, `CheckUpdates`,
  `InstallUpdate`; a subscription for the 6 h timer; state `update: UpdateState`.
- `src/platform.rs`: `launch_installer(path) -> Result<(), String>` (detached process).
- `installer/bloop.iss`: relaunch `[Run]` entry with `Check: Relaunch` reading
  `{param:RELAUNCH|0}`.

## Testing

- Unit: version parse/compare (v-prefix, missing parts rejected, 0.10.0 > 0.9.9); release
  JSON → Release (draft/prerelease ignored, missing assets, oversized asset); checksum file
  parse; verify match/mismatch; `ready` picks the newest verified file and ignores `.part`.
- Integration (local fixture server as in `tests/transport.rs`): latest → newer/none/HTTP
  error; download streams, rejects size overrun and checksum mismatch, refuses a redirect to
  another host.
- Manual: publish 0.2.1 then 0.3.0-style releases from `release.ps1`; a running older build
  finds, downloads and peeks; clicking it updates and relaunches with settings and chats kept;
  "Check now" offline shows an error; the auto switch off stops scheduled checks.
