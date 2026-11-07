# Auto-update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** bloop finds, downloads, verifies and (on one click) installs newer releases from GitHub; the developer publishes with one script.

**Architecture:** A library module `update` talks to the GitHub Releases API and downloads/verifies installers with `reqwest` + `sha2`. The UI runs a 6-hour check timer, tracks an `UpdateState`, peeks when ready, and launches the Inno installer with `/RELAUNCH=1` before quitting. A PowerShell script publishes releases with `gh`.

**Tech Stack:** Rust 2024, reqwest 0.13, `sha2 = "0.10"`, Iced 0.14, Inno Setup 6, GitHub CLI.

**Spec:** `docs/superpowers/specs/2026-11-07-auto-update-design.md`

## Global Constraints

- API: `GET https://api.github.com/repos/LaurentMaxhuni/bloop/releases/latest`, headers `User-Agent: bloop-updater`, `Accept: application/vnd.github+json`; 10 s timeout; body ≤ 256 KiB.
- Assets `bloop-setup-X.Y.Z.exe` and `bloop-setup-X.Y.Z.exe.sha256` both required; installer ≤ 50 MiB; checksum file ≤ 1 KiB, format `<64 hex>  <name>`.
- Redirects only to HTTPS `github.com` or hosts ending `.githubusercontent.com`; max 5.
- Download dir `<data dir>/updates`, `.part` while downloading, renamed after SHA-256 match; older installers removed; download timeout 10 min.
- Schedule: first check 30 s after start, then every 6 h, only when `config.auto_update` (default true).
- Copy: peek "Update X.Y.Z ready — click to restart"; statuses "Up to date", "Checking…", "Downloading X.Y.Z (N %)", "X.Y.Z ready", "Couldn't check for updates: <reason>".
- Installer args: `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /RELAUNCH=1`; never install while a reply streams.
- `release.ps1` refuses: existing tag, dirty tree, failed `gh auth status` ("Run `gh auth login` once").

## Review Focus

1. GitHub rate-limits (403) or returns HTML → scheduled check stays silent, no crash, retries next interval. Test in Task 2.
2. bloop quits mid-download → leftover `.part` never offered as ready, overwritten next time. Test in Task 3.
3. Release with a tag lower than or equal to the running version (or `0.10.0` vs `0.9.9`) → correct "no update"/"update". Test in Task 1.
4. Installer launch fails (file deleted, blocked by antivirus) → bloop stays open with the error, not quit. Test in Task 4.
5. A reply is streaming when the download finishes → no peek until it ends. Test in Task 4.

---

### Task 1: Versions, releases and checksums (pure)

**Files:** Create `src/update.rs`; modify `src/lib.rs`, `Cargo.toml` (`sha2 = "0.10"`).

**Interfaces — Produces:** `struct Version { major: u32, minor: u32, patch: u32 }` with `parse(&str) -> Option<Version>` (accepts leading `v`, exactly three numeric parts), `Ord`, `Display` ("X.Y.Z"), `Version::current()` from `CARGO_PKG_VERSION`; `struct Release { version, installer_url: String, checksum_url: String, size: u64 }`; `fn parse_release(json: &[u8], current: &Version) -> Result<Option<Release>, Error>`; `fn parse_checksum(text: &str, file_name: &str) -> Option<[u8; 32]>`; `fn sha256_file(path) -> io::Result<[u8; 32]>`; `enum Error { Network(String), Protocol(String), Io(String) }` (Display = message).

- [ ] Tests: `versions_parse_and_compare` (v1.2.3 == 1.2.3; "1.2" / "1.2.3.4" / "a.b.c" → None; 0.10.0 > 0.9.9); `release_json_maps_assets` (fixture JSON with both assets → Some; draft or prerelease → None; tag ≤ current → None; missing `.sha256` → None; size 60 MiB → None); `checksum_file_parses` (valid line; wrong name → None; bad hex → None; uppercase hex accepted); `sha256_of_known_bytes` ("abc" → ba7816bf…).
- [ ] Run → fail; implement; run → pass; commit `feat: update version and release parsing`.

### Task 2: Checking and downloading over HTTP

**Files:** Modify `src/update.rs`; create `tests/update.rs` (fixture server like `tests/transport.rs`, extended to serve several paths and a 302).

**Interfaces — Produces:** `async fn latest_from(api_url: &str, current: &Version) -> Result<Option<Release>, Error>` and `latest(current)` (uses the GitHub URL); `async fn download(release: &Release, dir: &Path, progress: impl FnMut(u8)) -> Result<PathBuf, Error>`; `fn ready(dir: &Path, current: &Version) -> Option<(Version, PathBuf)>` (newest `bloop-setup-X.Y.Z.exe` > current whose `.sha256` sidecar saved beside it still matches); `fn client() -> reqwest::Client` with the redirect policy from Global Constraints (tests use `client_for_tests()` that also allows `127.0.0.1`).

- [ ] Tests: `latest_finds_newer_release`; `latest_none_when_current`; `rate_limit_or_html_is_an_error_not_a_panic` (403 + HTML body → Err); `download_verifies_and_renames` (served exe + matching sha256 → `.exe` exists, no `.part`, progress hits 100); `checksum_mismatch_deletes_file`; `size_overrun_is_rejected` (server sends more than `size`); `redirect_to_other_host_refused` (302 to `http://example.com/x` → Err); `ready_ignores_part_and_older` (dir with `.part`, an older verified exe and a newer verified exe → newer).
- [ ] Run → fail; implement (stream `chunk()` into the `.part` file with `sha2::Sha256` updated per chunk; save the expected hex as `<exe>.sha256` beside the final file for `ready`); run → pass; commit `feat: check and download updates`.

### Task 3: Installer relaunch and release script

**Files:** Modify `installer/bloop.iss`; create `installer/release.ps1`; modify `README.md`.

**Interfaces — Produces:** `[Run]` entry `Filename: "{app}\bloop.exe"; Flags: nowait; Check: Relaunch` plus `[Code] function Relaunch(): Boolean; begin Result := ExpandConstant('{param:RELAUNCH|0}') = '1'; end;` (the existing `postinstall skipifsilent` entry stays for interactive installs). `release.ps1 [-Notes <text>]`: build via `build.ps1`, write sha256 sidecar in `dist/`, guard checks, `gh release create`.

- [ ] Verify `iscc` compiles; silent install of the built setup over the current install with `/RELAUNCH=1` (ask the user first: it restarts their bloop) relaunches bloop and keeps the Run value/shortcuts; `release.ps1` dry checks: dirty tree → refuses; without `gh` auth → refuses with the hint.
- [ ] Commit `feat: installer relaunch and release script`.

### Task 4: Update flow in the app

**Files:** Modify `src/config.rs` (`auto_update: bool`, default true), `src/ui.rs`, `src/ui/hub.rs` (Updates section on the Model page), `src/platform.rs` (`launch_installer`).

**Interfaces:**
- Consumes: Task 1–2 `Version`, `Release`, `latest`, `download`, `ready`.
- Produces: `enum UpdateState { Idle, Checking, Downloading(Version, u8), Ready(Version, PathBuf), Failed(String) }`; messages `CheckUpdates { manual: bool }`, `UpdateChecked(bool, Result<Option<Release>, String>)`, `UpdateProgress(Version, u8)`, `UpdateReady(Result<(Version, PathBuf), String>)`, `InstallUpdate`, `SetAutoUpdate(bool)`; `platform::launch_installer(path: &Path) -> Result<(), String>` (detached, `CREATE_NO_WINDOW` not set — the installer is silent anyway).
- Subscription: `every(30 s)` once then `every(6 h)` while `auto_update` (track `next_check: Instant`; a 60 s tick compares). On `Loaded`, `ready(dir)` → `Ready` without downloading.
- Peek when entering `Ready` unless a reply is streaming; on `Finished` with `Ready` pending, peek then. Pill status shows the ready text while `Ready`. Clicking the pill while `Ready` (OpenClick) installs; hub button "Restart to update" installs.
- `InstallUpdate`: `persist()` then `launch_installer` → on Ok `iced::exit()`, on Err `UpdateState::Failed(err)` and stay.

- [ ] Tests: `scheduled_check_failure_is_silent` (UpdateChecked(false, Err) → state Idle, no status text); `manual_check_failure_shows_error`; `newer_release_starts_download` (state Downloading); `ready_peeks_unless_replying` (Ready during request → no peek; Finished → peek with the copy); `install_failure_keeps_bloop_open` (launch fails via a test seam `installer_launcher: fn(&Path) -> Result<(), String>` field → Failed, not quitting); `auto_update_off_skips_scheduled_checks`; `config_auto_update_defaults_on`.
- [ ] Run → fail; implement; run → pass; clippy clean; commit `feat: update flow in the app`.

### Task 5: End-to-end, docs, review

- [ ] With the user's go-ahead and `gh auth login` done: publish `v0.2.1` via `release.ps1` from this branch; install 0.2.1; publish `v0.2.2` with a visible change; confirm a running 0.2.1 checks ("Check now"), downloads, peeks, updates on click and relaunches with chats and settings kept. Note results.
- [ ] `docs/architecture.md` (updater paragraph), `docs/verification.md` (results + not-checked list); `cargo test`, clippy `-D warnings`, `fmt --check` clean; commit `docs: auto-update verification`.
- [ ] Final whole-branch review (fresh reviewer), fix Critical/Important with RED→GREEN.
