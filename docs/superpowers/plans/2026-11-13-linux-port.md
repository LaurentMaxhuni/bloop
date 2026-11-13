# Linux Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** bloop runs on Linux (GNOME, KDE, Hyprland/Sway, X11 desktops) with the notch, tray, shortcuts, keyring, autostart and AppImage self-update, without changing Windows behaviour.

**Architecture:** On Linux bloop forces winit's X11 backend (XWayland on Wayland sessions) and implements every `platform.rs` function in a new `src/platform/linux/` module tree using `x11rb` (window properties, SHAPE, RandR, EWMH), `ksni` (tray), `global-hotkey` (X11 shortcuts), `ashpd` (portals: shortcuts, settings, file chooser) and `oo7` (Secret Service). Shared files change only behind `cfg(target_os = "linux")` or in ways covered by unchanged Windows tests. A Linux CI job and an AppImage release job sit next to the Windows job.

**Tech Stack:** Rust 1.92 (edition 2024), iced 0.14 (`x11`, `wayland` features on Linux), wgpu 27 (`vulkan`, `gles` on Linux), x11rb, ksni 0.3, ashpd 0.14, oo7 0.7, global-hotkey 0.9, libc, appimagetool, GitHub Actions `ubuntu-22.04`.

**Spec:** `docs/superpowers/specs/2026-11-13-linux-port-design.md`

## Global Constraints

- Every Linux addition sits behind `cfg(target_os = "linux")`; Windows behaviour, look, installer and update flow must not change.
- Existing Windows tests keep their exact expectations. A test may only be moved behind `#[cfg(windows)]` or retargeted to an explicit Windows parameter (e.g. `AssetNames::WINDOWS`) with identical assertions.
- Public `platform::` functions keep their signatures unless a task below names the change.
- Linux build baseline: x86_64, glibc 2.35 (`ubuntu-22.04` runner).
- Secrets are never written in plain text. No keyring → a clear error.
- A missing desktop feature (tray host, portal, XWayland) degrades with a one-time `platform::Event::Warning`, never a crash or hang.
- `std::env::remove_var`/`set_var` are `unsafe` in edition 2024; call them only at the top of `main`, before any thread exists.
- Every `unsafe` block carries a `// SAFETY:` comment (`clippy::undocumented_unsafe_blocks = "deny"`).
- Fast local loop on Windows: `cargo check --target x86_64-unknown-linux-gnu --all-targets` (target is installed). Linux tests run in the CI job from Task 1; push the branch to run them.
- Exact user-facing copy (from the spec):
  - No XWayland: `Install XWayland for the notch to stay on top.`
  - No tray: `No tray found. On GNOME, enable the AppIndicator extension.`
  - Fatal: `bloop could not start. A Vulkan or OpenGL-capable graphics driver is required.`

## Review Focus

1. bloop crashed and left `instance.sock` behind → the next launch must start normally, not exit as a "second copy". (Task 3, `stale_socket_is_replaced`)
2. `XDG_RUNTIME_DIR` unset (some su/ssh/minimal sessions) → socket goes to `/tmp/bloop-$UID/` created 0700, and an existing dir owned by someone else is refused. (Task 3, `runtime_dir_fallback_is_private`)
3. AppImage in a folder the user can't write (e.g. `/opt`) or bloop not run as an AppImage → "update available" opens the release page; never a half-written file. (Task 11, `install_mode_*`)
4. Executable path containing spaces or `%` → `.desktop` `Exec=` lines stay valid. (Task 7, `exec_line_quotes_spaces_and_percent`)
5. Keyring locked and the user dismisses the prompt, or no Secret Service at all → saving a key or ChatGPT sign-in shows a readable error and nothing reaches SQLite in clear text. (Task 9, `chatgpt_session_never_stored_in_sqlite_on_linux`)

---

## File Structure

| File | Responsibility |
|---|---|
| `src/platform.rs` | Unchanged Windows code; each `#[cfg(not(windows))]` branch becomes a call into `linux::` (or stays a no-op on other OSes) |
| `src/platform/linux/mod.rs` | Declares the submodules below; `Session` |
| `src/platform/linux/session.rs` | Backend choice, original `WAYLAND_DISPLAY`, `child_env` |
| `src/platform/linux/instance.rs` | Unix-socket single instance and requests |
| `src/platform/linux/x11.rs` | x11rb connection, notch properties, SHAPE, icon, RandR, EWMH fullscreen |
| `src/platform/linux/tray.rs` | ksni tray |
| `src/platform/linux/shortcuts.rs` | global-hotkey (X11) and GlobalShortcuts portal |
| `src/platform/linux/desktop.rs` | `.desktop` files: autostart and launcher, icon install, tiling-desktop rules |
| `src/platform/linux/portal.rs` | Settings portal, FileChooser, `xdg-open`, fatal dialog |
| `src/linux_update.rs` (lib) | Pure AppImage install decision and replace step |
| `installer/linux/` | `bloop.desktop`, `bloop.png`, `build-appimage.sh` |

`src/platform.rs` becomes `src/platform.rs` + `src/platform/linux/…` (Rust 2018 module layout, `#[cfg(target_os = "linux")] mod linux;` at the top of `platform.rs`).

---

### Task 1: Linux builds and tests in CI

**Files:**
- Modify: `Cargo.toml`
- Modify: `.github/workflows/release.yml`
- Modify: `tests/cli.rs:278-290`, plus any other test that fails on Linux only because it assumes Windows (paths like `C:/`, `.exe`)
- Modify: `src/update.rs` (asset names only, see below)

**Interfaces:**
- Produces: a green `linux` job that later tasks rely on to run their tests.
- Produces in `src/update.rs`:
  `pub struct AssetNames { pub prefix: &'static str, pub suffix: &'static str, pub max_size: u64 }`,
  `pub const WINDOWS: AssetNames = AssetNames { prefix: "bloop-setup-", suffix: ".exe", max_size: 50 * 1024 * 1024 };`,
  `pub const LINUX: AssetNames = AssetNames { prefix: "bloop-", suffix: "-x86_64.AppImage", max_size: 150 * 1024 * 1024 };`,
  `pub const CURRENT: AssetNames` (= `LINUX` on Linux, `WINDOWS` elsewhere),
  `pub fn installer_name_for(names: &AssetNames, version: &Version) -> String`,
  `pub fn parse_release_for(json: &[u8], current: &Version, names: &AssetNames) -> Result<Option<Release>, Error>`,
  `pub fn ready_for(dir: &Path, current: &Version, names: &AssetNames) -> Option<(Version, PathBuf)>`.
  `installer_name`, `parse_release`, `ready` keep their signatures and call the `_for` versions with `&CURRENT`. `MAX_INSTALLER` stays as the Windows value; download uses `CURRENT.max_size`.

- [ ] **Step 1: Cargo.toml.** Move `global-hotkey = "0.9"` to `[target.'cfg(any(windows, target_os = "linux"))'.dependencies]`. Add `[target.'cfg(target_os = "linux")'.dependencies]` with `iced` (same version and features as the main entry plus `"x11", "wayland"`), `wgpu = { version = "27", default-features = false, features = ["std", "wgsl", "vulkan", "gles"] }`, `x11rb = { version = "0.13", features = ["shape", "randr"] }`, `ksni = "0.3"`, `ashpd = { version = "0.14", default-features = false, features = ["tokio"] }`, `oo7 = { version = "0.7", default-features = false, features = ["tokio", "native_crypto"] }` (pick the pure-Rust crypto feature if `native_crypto` pulls OpenSSL), `libc = "0.2"`. Pin to the versions `cargo` resolves today.
- [ ] **Step 2: Run** `cargo check --target x86_64-unknown-linux-gnu --all-targets` and `cargo check --all-targets`. Expected: both succeed (Linux code is still the existing stubs).
- [ ] **Step 3: Retarget update tests.** In `src/update.rs` tests, every test that asserts `.exe` names calls `parse_release_for(..., &WINDOWS)` / `installer_name_for(&WINDOWS, ..)` / `ready_for(.., &WINDOWS)` with identical assertions. Add:

```rust
#[test]
fn linux_asset_names() {
    let v = Version::parse("0.6.0").unwrap();
    assert_eq!(installer_name_for(&LINUX, &v), "bloop-0.6.0-x86_64.AppImage");
    // A Windows asset is ignored by the Linux parser and vice versa.
}
```
  with a JSON fixture holding both an `.exe` and an `.AppImage` (+ `.sha256` each) asserting each parser picks its own file.
- [ ] **Step 4: Gate Windows-only tests.** `tests/cli.rs::find_in_prefers_exe_then_cmd` gets `#[cfg(windows)]`. Fix or gate any other test that fails on Linux only for a Windows path reason (identical assertions). Linux replacements come in Task 10.
- [ ] **Step 5: CI job.** In `release.yml`, add job `linux` on `ubuntu-22.04`, same triggers and path filters, steps: checkout, `sudo apt-get install -y pkg-config libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev`, `dtolnay/rust-toolchain@stable` with clippy+rustfmt, `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`. Leave the Windows job untouched.
- [ ] **Step 6: Push and check.** Push the branch; expected: `linux` and the Windows `build` jobs both pass.
- [ ] **Step 7: Commit** `ci: build and test on Linux`.

### Task 2: Backend choice and child environment

**Files:**
- Create: `src/platform/linux/mod.rs`, `src/platform/linux/session.rs`
- Modify: `src/platform.rs` (top: `#[cfg(target_os = "linux")] mod linux;`), `src/main.rs`

**Interfaces:**
- Produces:
  `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Session { X11, XWayland, WaylandOnly }`,
  `pub struct Backend { pub session: Session, pub drop_wayland_display: bool, pub original_wayland_display: Option<OsString> }`,
  `pub fn choose_backend(var: impl Fn(&str) -> Option<OsString>) -> Backend` (pure),
  `pub fn init_backend()` (reads env, calls `remove_var` when told to, stores `Backend` in a `OnceLock`; called first thing in `main` on Linux),
  `pub fn session() -> Session` (defaults to `X11` when `init_backend` never ran, e.g. in tests),
  `pub fn child_env(command: &mut std::process::Command)` and `pub fn child_env_tokio(command: &mut tokio::process::Command)` (re-add the original `WAYLAND_DISPLAY` if one was removed). All re-exported from `platform` as `platform::linux::…` for later tasks.

- [ ] **Step 1: Failing tests** in `session.rs`:

```rust
fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
    move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.into())
}
#[test] fn x11_session_keeps_everything() {
    let b = choose_backend(env(&[("DISPLAY", ":0"), ("XDG_SESSION_TYPE", "x11")]));
    assert_eq!(b.session, Session::X11); assert!(!b.drop_wayland_display);
}
#[test] fn wayland_with_xwayland_drops_wayland_display() {
    let b = choose_backend(env(&[("DISPLAY", ":0"), ("WAYLAND_DISPLAY", "wayland-0")]));
    assert_eq!(b.session, Session::XWayland); assert!(b.drop_wayland_display);
    assert_eq!(b.original_wayland_display.as_deref(), Some(OsStr::new("wayland-0")));
}
#[test] fn wayland_without_display_stays_wayland() {
    let b = choose_backend(env(&[("WAYLAND_DISPLAY", "wayland-0")]));
    assert_eq!(b.session, Session::WaylandOnly); assert!(!b.drop_wayland_display);
}
#[test] fn backend_override_keeps_wayland() {
    let b = choose_backend(env(&[("DISPLAY", ":0"), ("WAYLAND_DISPLAY", "w"), ("BLOOP_BACKEND", "wayland")]));
    assert!(!b.drop_wayland_display);
}
#[test] fn child_env_restores_wayland_display() { /* build Backend by hand, apply to a Command, assert get_envs() contains WAYLAND_DISPLAY=wayland-0 */ }
```
  (`XDG_SESSION_TYPE=wayland` with `DISPLAY` set and no `WAYLAND_DISPLAY` also counts as `XWayland`.)
- [ ] **Step 2: Run** the Linux CI job (or `cargo check --target x86_64-unknown-linux-gnu --all-targets` for compile). Expected: FAIL, `choose_backend` not defined.
- [ ] **Step 3: Implement** the interface above. `child_env` takes the `Backend` from the `OnceLock`; a private `apply_child_env(backend: &Backend, cmd)` is what the test calls.
- [ ] **Step 4: Wire `main.rs`:** first statement of `main`, `#[cfg(target_os = "linux")] platform::linux::init_backend();`. After the iced app starts, if `session() == WaylandOnly`, `platform::events` sends `Event::Warning("Install XWayland for the notch to stay on top.")` once (wired in Task 5's `events`).
- [ ] **Step 5: Run** tests. Expected: PASS on Linux CI; `cargo test` on Windows unchanged.
- [ ] **Step 6: Commit** `feat(linux): pick the X11 backend and keep Wayland for child processes`.

### Task 3: Single instance and `--toggle` / `--ask`

**Files:**
- Modify: `src/launch.rs`, `src/main.rs`, `src/platform.rs` (`Instance`, `single_instance`, `signal_running_copy`, `open_requests`), `src/ui.rs:630` and `:813`
- Create: `src/platform/linux/instance.rs`

**Interfaces:**
- Produces in `launch.rs`: `Launch::{Manual, Background, Toggle, Ask}`; `launch_kind` returns `Toggle`/`Ask` when `--toggle`/`--ask` is present (`--background` wins if several are given); `startup` maps `Toggle`/`Ask` like `Manual`; `pub fn request(launch: Launch) -> Option<Request>` where `pub enum Request { Open, Toggle, Ask }` (`Manual → Open`, `Background → None`). `signals_running_copy(launch)` = `request(launch).is_some()`.
- Produces in `platform`: `pub fn signal_running_copy(request: bloop::launch::Request)` (Windows ignores the variant and keeps its exact behaviour); `pub fn open_requests() -> impl Stream<Item = bloop::launch::Request>` (Windows yields `Request::Open`). `ui.rs` maps `Open → Message::OpenWindow`, `Toggle → Message::Toggle`, `Ask → Message::AskClipboard`.
- Produces in `instance.rs`: `pub fn socket_dir(runtime_dir: Option<&Path>, uid: u32) -> PathBuf`, `pub fn bind(dir: &Path) -> std::io::Result<Option<Listener>>` (`None` = another copy is live), `pub fn send(dir: &Path, request: Request) -> std::io::Result<()>`, `pub fn parse_request(line: &str) -> Option<Request>`; wire format one line: `open`, `toggle`, `ask`.

- [ ] **Step 1: Failing tests** in `launch.rs` (existing tests untouched):

```rust
#[test] fn toggle_and_ask_flags() {
    assert_eq!(launch_kind(args(&["bloop", "--toggle"])), Launch::Toggle);
    assert_eq!(launch_kind(args(&["bloop", "--ask"])), Launch::Ask);
    assert_eq!(launch_kind(args(&["bloop", "--toggle", "--background"])), Launch::Background);
}
#[test] fn requests_per_launch() {
    assert_eq!(request(Launch::Manual), Some(Request::Open));
    assert_eq!(request(Launch::Background), None);
    assert_eq!(request(Launch::Toggle), Some(Request::Toggle));
    assert_eq!(startup(Launch::Ask, true), Startup::Dashboard);
}
```
  and in `instance.rs` (`#[cfg(test)]`, temp dirs under `std::env::temp_dir()`):
  `second_bind_is_refused` (first `bind` → `Some`, second → `None`), `send_reaches_listener` (`send(dir, Toggle)` → listener reads `Some(Request::Toggle)`), `stale_socket_is_replaced` (create the socket file with no listener → `bind` returns `Some`), `runtime_dir_fallback_is_private` (`socket_dir(None, 1000)` = `/tmp/bloop-1000`; after `bind`, the dir mode is `0o700`; a pre-existing dir with mode `0o777` makes `bind` return `Err`), `parse_request_rejects_unknown` (`parse_request("rm -rf") == None`).
- [ ] **Step 2: Run** tests. Expected: FAIL (variants/functions missing).
- [ ] **Step 3: Implement.** `socket_dir`: `<runtime>/bloop` or `/tmp/bloop-<uid>`; create with `DirBuilder::mode(0o700)`; refuse (`PermissionDenied`) a dir not owned by `uid` or with group/other bits set. `bind`: `UnixListener::bind(dir/"instance.sock")`; on `AddrInUse`, try `UnixStream::connect` — success → `Ok(None)`, `ConnectionRefused` → remove file, bind again. `Instance` on Linux holds the listener path and removes the file in `Drop`. `open_requests` accepts on a blocking thread and forwards parsed lines.
- [ ] **Step 4: Update `main.rs`:** `signal_running_copy(request)` only when `request(launch)` is `Some`.
- [ ] **Step 5: Run** tests on both platforms. Expected: PASS; Windows launch tests unchanged.
- [ ] **Step 6: Commit** `feat(linux): single instance over a Unix socket, --toggle and --ask`.

### Task 4: Notch window on X11 (type, taskbar, shape, icon)

**Files:**
- Create: `src/platform/linux/x11.rs`
- Modify: `src/ui.rs:75-95` (`notch_settings`) and the place the notch window is opened, `src/platform.rs` (`shape`, `apply_app_icon`, new `prepare_notch`)

**Interfaces:**
- Produces: `pub fn prepare_notch(window: &dyn iced::window::Window)` in `platform` (no-op off Linux); in `x11.rs`: `pub fn scaled_outline(outline: &[(f32, f32)], scale: f32) -> Vec<(i16, i16)>` (pure), `fn xid(window: &dyn iced::window::Window) -> Option<u32>` (from `RawWindowHandle::Xlib`/`Xcb`), a lazily opened shared `x11rb::rust_connection::RustConnection`.
- Notch window title: `pub const NOTCH_TITLE: &str = "bloop notch";` in `ui.rs`, used by `Island::title` for the notch id (Task 6 relies on it).

- [ ] **Step 1: Failing test:**

```rust
#[test] fn outline_scales_and_rounds() {
    assert_eq!(scaled_outline(&[(0.0, 0.0), (10.5, 4.0)], 1.5), vec![(0, 0), (16, 6)]);
}
```
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.** On Linux `notch_settings()` sets `visible: false` and `platform_specific.application_id = "bloop".into()`. Where the notch opens, chain `window::run(id, platform::prepare_notch)` then `window::set_mode(id, Mode::Windowed)` (show). `prepare_notch` sets `_NET_WM_WINDOW_TYPE = [_NET_WM_WINDOW_TYPE_UTILITY]` and `_NET_WM_STATE = [_NET_WM_STATE_ABOVE, _NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER]` before the window is mapped. `shape` scales with `window.scale_factor()` and sets `ShapeKind::BOUNDING` and `ShapeKind::INPUT` from one polygon rectangle list (`shape::rectangles` from a scanline fill of the polygon, or `shape::mask` from a 1-bit pixmap; implementer's choice), silently returning if SHAPE is absent. `apply_app_icon` on Linux decodes the embedded PNG and calls `window.set_window_icon` equivalent via iced `window::Icon` at window creation instead (set `icon` in both `notch_settings` and `app_settings` on Linux).
- [ ] **Step 4: Run** unit tests (PASS) and manual check in WSLg (`wsl -d Ubuntu`, run the Linux build): notch is borderless, not in a taskbar, clicks outside the outline reach the window below.
- [ ] **Step 5: Commit** `feat(linux): notch window type, shape and icon on X11`.

### Task 5: Placement and fullscreen hiding

**Files:**
- Modify: `src/platform/linux/x11.rs`, `src/platform.rs` (`place_on`, `place`, `primary_position`, `fullscreen_active`, `events`)

**Interfaces:**
- Consumes: shared `physical_rect` (already `cfg(any(windows, test))` → widen to `cfg(any(windows, target_os = "linux", test))`).
- Produces in `x11.rs`: `pub struct Monitor { pub x: i32, pub y: i32, pub width: u32, pub height: u32, pub primary: bool }`, `pub fn pick_monitor(monitors: &[Monitor], pointer: Option<(i32, i32)>, current: Option<usize>) -> Option<usize>` (pure), `pub fn covers_monitor(window: (i32, i32, u32, u32), monitor: &Monitor) -> bool` (pure), `pub fn monitors() -> Vec<Monitor>` (RandR `get_monitors`), `pub fn fullscreen_on(monitor: &Monitor) -> bool` (EWMH).

- [ ] **Step 1: Failing tests:** `pointer_picks_its_monitor` (pointer at (2000, 10) with monitors 0..1920 and 1920..3840 → `Some(1)`), `no_pointer_keeps_current` (`pointer None, current Some(1)` → `Some(1)`), `no_pointer_no_current_uses_primary`, `fullscreen_must_cover_the_monitor` (a window of exactly the monitor's rect → true; one 10 px shorter → false).
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.** Pointer comes from `QueryPointer` only when `session() == X11`; on `XWayland` pass `None` (spec: fresh only over X11 windows). Work area from `_NET_WORKAREA` intersected with the monitor, falling back to the monitor rect. `fullscreen_active` = the `_NET_ACTIVE_WINDOW` has `_NET_WM_STATE_FULLSCREEN` and `covers_monitor` the notch's monitor; any X error → `false`. In `events`, the Linux branch starts the tray (Task 6) and sends the XWayland warning from Task 2 when `session() == WaylandOnly`.
- [ ] **Step 4: Run** tests (PASS); manual WSLg check: each `NotchPosition` lands where it does on Windows.
- [ ] **Step 5: Commit** `feat(linux): place the notch per monitor and hide it for fullscreen apps`.

### Task 6: Tray, global shortcuts and tiling-desktop rule

**Files:**
- Create: `src/platform/linux/tray.rs`, `src/platform/linux/shortcuts.rs`
- Modify: `src/platform.rs` (`Controller`, `validate_shortcut`, `events`), `src/platform/linux/desktop.rs` (rule text), `src/config.rs` (new field), `src/ui/dashboard.rs` (shortcut note, rule message)

**Interfaces:**
- Produces: `Controller` on Linux holds a `tokio::sync::mpsc::UnboundedSender<ShortcutCommand>` where `enum ShortcutCommand { Shortcut(String), AskShortcut(String) }`; `validate_shortcut` on Linux = the Windows `global_hotkey` parse.
- `shortcuts.rs`: `pub fn portal_trigger(shortcut: &str) -> Option<String>` (pure; bloop's `"Ctrl+Shift+Space"` → portal `"CTRL+SHIFT+space"`, i.e. modifiers upper-case `CTRL`/`ALT`/`SHIFT`/`LOGO`, key as an XKB keysym name), `pub enum Backend { X11, Portal, None }`, `pub fn backend(session: Session, portal_available: bool) -> Backend` (pure).
- New `platform::Event::ShortcutsManagedBySystem(Vec<(String, String)>)` (id, trigger description from the portal), shown read-only under the shortcut setting with the note `Your desktop manages this shortcut.`
- `desktop.rs`: `pub enum Tiling { Hyprland, Sway, I3 }`, `pub fn tiling(var: impl Fn(&str) -> Option<OsString>) -> Option<Tiling>` (pure: `HYPRLAND_INSTANCE_SIGNATURE` → Hyprland, `SWAYSOCK` → Sway, `I3SOCK` → I3, else `XDG_CURRENT_DESKTOP` contains `Hyprland`/`sway`/`i3`), `pub fn rule(t: Tiling) -> &'static str`.
- `config.rs`: `pub tiling_rule_dismissed: bool` (serde default `false`).

- [ ] **Step 1: Failing tests:** `portal_trigger_maps_modifiers` (`"Ctrl+Shift+Space"` → `Some("CTRL+SHIFT+space")`, `"Alt+K"` → `Some("ALT+k")`, `""` → `None`); `backend_choice` (`(X11, _) → X11`, `(XWayland, true) → Portal`, `(XWayland, false) → None`, `(WaylandOnly, true) → Portal`); `tiling_detection` (each env var → its variant, GNOME → `None`); `rules_match_notch_title` (every `rule(..)` contains `"bloop notch"`); `config_default_rule_not_dismissed`.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement tray.** `ksni::Tray` with menu items `Show / collapse bloop` → `Event::Toggle`, `Settings` → `Event::Settings`, `Quit bloop` → `Event::Quit`; `activate` (left click) → `Event::Toggle`; icon from the embedded PNG as ARGB32. If spawning fails or no `StatusNotifierWatcher` exists, send `Event::Warning("No tray found. On GNOME, enable the AppIndicator extension.")` once.
- [ ] **Step 4: Implement shortcuts.** `X11`: a thread with `GlobalHotKeyManager` mirroring the Windows `Shortcuts` slot logic (two slots: toggle, ask). `Portal`: `ashpd::desktop::global_shortcuts::GlobalShortcuts` session, `bind_shortcuts` with ids `toggle` (description `Show or collapse bloop`) and `ask` (`Ask bloop about the clipboard`) and `preferred_trigger = portal_trigger(..)`; `activated` signals → `Event::Toggle`/`Event::Ask`; send `ShortcutsManagedBySystem` with what the portal returned. `None`: one `Event::Warning` naming the commands: `This desktop has no global shortcut service. Bind "bloop --toggle" and "bloop --ask" to keys in your keyboard settings.` Then `Event::ShortcutApplied` as on Windows. `Event::Ready(Controller)` is sent in every case.
- [ ] **Step 5: Tiling rule UI.** On Linux, when `tiling(..)` is `Some` and `!config.tiling_rule_dismissed`, the dashboard shows a dismissable card: title `Keep the notch on top`, body `Your desktop tiles windows. Add this to its config so the notch floats above them:`, the rule in a monospace block, buttons `Copy` (clipboard write) and `Dismiss` (sets the flag and saves). Rules (exact; the implementer verifies on the real desktop and fixes syntax if needed):
  - Hyprland: `windowrulev2 = float, pin, noborder, noshadow, nofocus, title:^(bloop notch)$` — remove `nofocus` if it stops the prompt taking focus during testing.
  - Sway: `for_window [title="^bloop notch$"] floating enable, sticky enable, border none`
  - i3: same line as Sway.
- [ ] **Step 6: Run** tests (PASS). Manual WSLg check: tray appears if a host exists, else the warning; X11 shortcut toggles the notch.
- [ ] **Step 7: Commit** `feat(linux): tray, global shortcuts and tiling-desktop rule`.

### Task 7: Desktop files (autostart, launcher entry, icon)

**Files:**
- Modify: `src/platform/linux/desktop.rs`, `src/platform.rs` (`autostart`, `set_autostart`, `migrate_autostart`), `src/main.rs` (call `ensure_launcher_entry` after `migrate_autostart`)
- Create: `installer/linux/bloop.png` (256 × 256, generated from `installer/art/bloop.ico` by extending `installer/make-art.ps1`), embedded with `include_bytes!`

**Interfaces:**
- Produces: `pub fn exec_line(exe: &Path, args: &[&str]) -> String` (pure, freedesktop Exec quoting: wrap in `"…"`, escape `"`, `` ` ``, `$`, `\` with `\`, double `%`), `pub fn desktop_entry(exe: &Path, background: bool) -> String` (pure), `pub fn current_exe_for_desktop() -> PathBuf` (`$APPIMAGE` else `current_exe()`), `pub fn autostart_path() -> PathBuf` (`$XDG_CONFIG_HOME` or `~/.config` + `autostart/bloop.desktop`), `pub fn launcher_path() -> PathBuf` (`$XDG_DATA_HOME` or `~/.local/share` + `applications/bloop.desktop`), `pub fn ensure_launcher_entry()`.

- [ ] **Step 1: Failing tests:** `exec_line_quotes_spaces_and_percent` (`/home/a b/100%/bloop.AppImage` with `["--background"]` → `"\"/home/a b/100%%/bloop.AppImage\" --background"`), `desktop_entry_fields` (contains `Type=Application`, `Name=bloop`, `Icon=bloop`, `StartupWMClass=bloop`, `X-GNOME-Autostart-enabled=true` only when `background`), `migration_detects_moved_appimage` (reuse shared `needs_migration(Some(old_exec), new_exe)`).
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.** `set_autostart(true)` writes `desktop_entry(exe, true)`; `false` removes the file (missing file is OK); `autostart()` = the file exists and has no `Hidden=true`. `migrate_autostart` rewrites a stale `Exec=`. `ensure_launcher_entry` writes the launcher file and `~/.local/share/icons/hicolor/256x256/apps/bloop.png` when missing or when `Exec=` differs, then best-effort runs `update-desktop-database` on the folder (ignore errors), and, when `ashpd` reports host Registry support, calls `ashpd::register_host_app("bloop")`.
- [ ] **Step 4: Run** tests (PASS).
- [ ] **Step 5: Commit** `feat(linux): start on login and launcher entry`.

### Task 8: System settings, save dialog, links, fatal error

**Files:**
- Create: `src/platform/linux/portal.rs`
- Modify: `src/platform.rs` (`light_theme_system`, `reduce_motion_system`, `save_file_dialog`, `open_url`, `show_fatal`)

**Interfaces:**
- Produces: `pub fn color_scheme_is_light(value: u32) -> bool` (pure: `1 → false`, `0 | 2 → true`, other → true), `pub fn xdg_open_command(url: &str) -> std::process::Command` (pure-ish: program `xdg-open`, one argument, `child_env` applied).

- [ ] **Step 1: Failing tests:** `color_scheme_values`, `xdg_open_takes_url_as_one_argument` (`get_args()` yields exactly one arg equal to the URL, even with spaces and `;`).
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.** `light_theme_system`/`reduce_motion_system` read the Settings portal with a short-lived current-thread Tokio runtime (these are called from sync UI code; keep a 500 ms timeout and fall back to today's defaults). `reduce_motion_system` reads `org.gnome.desktop.interface` `enable-animations` (false → reduce). `save_file_dialog` uses `ashpd` `SaveFileRequest` with `current_name(suggested_name)`; cancel → `Ok(None)`; no portal → `Err("No file dialog is available on this desktop.")`. `open_url` keeps the allow-list and spawns `xdg_open_command`. `show_fatal` on Linux uses the fatal copy from Global Constraints, prints to stderr, then tries `zenity --error --text <msg>` and else `kdialog --error <msg>`, ignoring failures.
- [ ] **Step 4: Run** tests (PASS); manual: dark/light follows the system on GNOME (WSLg has no portal: defaults apply, no hang).
- [ ] **Step 5: Commit** `feat(linux): system theme, save dialog, links and startup error`.

### Task 9: Secrets in the system keyring

**Files:**
- Modify: `src/secrets.rs`, `src/chatgpt.rs:105-123`

**Interfaces:**
- Produces in `secrets.rs`: `pub(crate) fn save_chatgpt_session(data: &mut [u8]) -> Result<(), String>`, `pub(crate) fn load_chatgpt_session() -> Result<Option<Vec<u8>>, String>`. Windows: exactly today's `chatgpt_blob` + `storage::save_chatgpt`/`load_chatgpt`. Linux: Secret Service item label `bloop: chatgpt`, attributes `{application: bloop, kind: chatgpt}`; `storage::save_chatgpt` is never called. `chatgpt.rs` `Store::load`/`save` call only these two.
- Linux `read`/`write` for API keys: label `bloop: <target>`, attributes `{application: bloop, target: <target>}`. Errors map through `pub(crate) fn keyring_error(e: &oo7::Error) -> String` with copy `bloop needs a keyring to save sign-ins (GNOME Keyring, KWallet or KeePassXC). Install or unlock one and try again.` for no-service / dismissed-prompt cases.

- [ ] **Step 1: Failing tests (Linux, `#[cfg(target_os = "linux")]`):** `chatgpt_session_never_stored_in_sqlite_on_linux` (with `BLOOP_DATA_DIR` set to a temp dir and the keyring call stubbed through a `trait Vault` with an in-memory fake, `save_chatgpt_session` leaves `storage::load_chatgpt()` as `Ok(None)`), `vault_round_trip` (fake vault: save, load equal, empty key deletes), `keyring_error_copy` (no-service error → message above). The existing Windows tests (`credential_round_trip`, `chatgpt_blob_protects_a_full_session`, …) get `#[cfg(windows)]` with identical bodies if they aren't already gated.
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.** A small `trait Vault { fn get(&self, target: &str) -> Result<Option<Vec<u8>>, String>; fn set(&self, target: &str, value: Option<&[u8]>) -> Result<(), String>; }` with an `oo7` implementation (blocking via a current-thread runtime, like Task 8) and the test fake. Never fall back to SQLite or a file.
- [ ] **Step 4: Run** tests (PASS on both); manual: save an API key on GNOME, see it in Seahorse as `bloop: …`.
- [ ] **Step 5: Commit** `feat(linux): keep keys and sign-ins in the system keyring`.

### Task 10: CLI providers on Linux

**Files:**
- Modify: `src/cli.rs:620-633` (`find_in`), `src/cli.rs:880-960` (`job` module), the place `chat` builds its `tokio::process::Command`
- Test: `tests/cli.rs`

**Interfaces:**
- `find_in(name, path_var)` keeps its signature; on Linux: first `dir/name` that is a file with any execute bit (`metadata.permissions().mode() & 0o111 != 0`).
- `job::Job::adopt` keeps its signature; Linux stores the child's pid as process-group id and `Drop` calls `libc::killpg(pgid, SIGKILL)`.

- [ ] **Step 1: Failing tests** in `tests/cli.rs` (`#[cfg(unix)]`): `find_in_needs_execute_bit` (file without `+x` skipped, with `+x` found, a directory named `codex` skipped).
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement.** In `chat`, on Linux: `command.process_group(0)`, `platform::linux::child_env_tokio(&mut command)` — `cli.rs` is in the library crate, so add a `pub fn child_env_hook(f: fn(&mut tokio::process::Command))` setter in `cli.rs` that `main.rs` sets on Linux (keep the lib free of `platform`), and an `unsafe { command.pre_exec(|| { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL); Ok(()) }) }` with a SAFETY comment (async-signal-safe call only).
- [ ] **Step 4: Run** tests (PASS); manual: start a CLI reply, kill bloop with `kill -9`, check the CLI process is gone.
- [ ] **Step 5: Commit** `feat(linux): find and clean up CLI providers`.

### Task 11: AppImage self-update

**Files:**
- Create: `src/linux_update.rs` (declared in `src/lib.rs` as `#[cfg(unix)] pub mod linux_update;`, since it uses Unix permissions)
- Modify: `src/platform.rs` (`launch_installer` Linux branch), `src/ui.rs:2074-2100` (install step), `src/ui.rs` update status for the release-page case

**Interfaces:**
- Produces: `pub enum InstallMode { Replace { appimage: PathBuf }, ReleasePage }`, `pub fn install_mode(appimage: Option<&Path>, dir_writable: bool) -> InstallMode` (pure), `pub fn replace(new_file: &Path, appimage: &Path) -> std::io::Result<()>` (copy to `<dir>/.bloop-update.tmp`, `chmod 0o755`, `rename` over `appimage`; remove the tmp file on any error), `pub fn release_page(version: &Version) -> String` (`https://github.com/LaurentMaxhuni/bloop/releases/tag/v<version>`).
- `launch_installer(path)` on Linux: `install_mode($APPIMAGE, writable)`; `Replace` → `replace`, then spawn the AppImage with `--background` and `child_env`; `ReleasePage` → `Err` is not used: instead the UI checks `install_mode` before downloading and shows the button `Open release page` (calls `open_url(release_page(v))`) in place of download/install.

- [ ] **Step 1: Failing tests:** `install_mode_without_appimage_opens_release_page`, `install_mode_unwritable_dir_opens_release_page`, `install_mode_writable_replaces`, `replace_swaps_file_and_keeps_it_executable` (temp dir: old file "A", new file "B" → appimage reads "B", mode has `0o111`, no `.bloop-update.tmp` left), `replace_failure_leaves_original` (new file path missing → `Err`, original still "A", no tmp file).
- [ ] **Step 2: Run.** Expected: FAIL.
- [ ] **Step 3: Implement** the interface. `dir_writable` = `libc::access(dir, W_OK) == 0` via a safe helper in `platform`.
- [ ] **Step 4: Run** tests (PASS on Linux; Windows unchanged).
- [ ] **Step 5: Commit** `feat(linux): AppImage self-update`.

### Task 12: AppImage packaging and release

**Files:**
- Create: `installer/linux/bloop.desktop`, `installer/linux/build-appimage.sh`
- Modify: `.github/workflows/release.yml`, `installer/release.ps1` (only if it reads release assets)

**Interfaces:**
- Produces: `installer/linux/build-appimage.sh <version>` → `dist/bloop-<version>-x86_64.AppImage` and `dist/bloop-<version>-x86_64.AppImage.sha256` (same line format `parse_checksum` reads: `<hex>  <file name>`).

- [ ] **Step 1: Script.** Builds `cargo build --release --locked`, lays out `AppDir/` (`usr/bin/bloop`, `bloop.desktop`, `bloop.png`, `AppRun` symlink to `usr/bin/bloop`), downloads a pinned `appimagetool` release and verifies its SHA-256 (hard-coded in the script), runs it, writes the checksum file. `set -euo pipefail`.
- [ ] **Step 2: Run locally in WSL** (`wsl -d Ubuntu`, after installing rustup there) or in CI. Expected: the AppImage starts (`--appimage-extract-and-run` if FUSE is missing) and its size fits under `LINUX.max_size`; if not, raise `LINUX.max_size` in Task 1's constant and its test.
- [ ] **Step 3: Workflow.** The `linux` job gains: run `build-appimage.sh`, upload `dist/*` as artifact `linux`. The Windows `build` job's final `gh release create` step is replaced by uploading its signed `bloop-setup-*.exe` and `.sha256` as artifact `windows` (all earlier steps unchanged). New job `publish` (`if: github.ref_type == 'tag'`, `needs: [build, linux]`, `ubuntu-latest`, `contents: write`) downloads both artifacts and runs the same `gh release create` command the Windows job ran, with both file sets and one extra notes line: `The Linux AppImage isn't code-signed. Check it against the .sha256 file next to it.`
- [ ] **Step 4: Check** on a pull request: both jobs green and both artifacts present. Tag flow is checked at the next release.
- [ ] **Step 5: Commit** `ci: package and publish the Linux AppImage`.

### Task 13: Docs and desktop verification

**Files:**
- Modify: `README.md` (Status and limits; install on Linux; tiling rules), `docs/architecture.md` (Linux section, replacing the line at :96), `docs/verification.md` (new Linux section), `AGENTS.md:10`

- [ ] **Step 1: README.** Replace "Windows only. Linux is planned; macOS isn't." with the supported desktops, the AppImage download, the GNOME AppIndicator note, the soft-under-fractional-scaling note, and the tiling rules from Task 6.
- [ ] **Step 2: verification.md.** One table per desktop (GNOME Wayland, KDE Plasma Wayland, Hyprland or Sway, Xfce X11) with rows: placement per `NotchPosition`, stays on top, shape and click-through, fullscreen hiding (X11 app and native Wayland app), tray, toggle shortcut, ask shortcut, prompt focus after shortcut, single instance and `--toggle`, autostart, keyring, theme, export dialog, links, self-update. Values: pass / fail / not possible on this desktop.
- [ ] **Step 3: Fill in** what WSLg can show (build, X11 path, single instance, autostart file, links) and mark the rest `not yet checked` for the user to run on real desktops.
- [ ] **Step 4: Windows regression pass.** Run `cargo test`, `cargo clippy --all-targets -- -D warnings` on Windows and rerun the existing Windows checks in `docs/verification.md` against a build of this branch.
- [ ] **Step 5: Commit** `docs: Linux support`.
