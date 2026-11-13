# Linux port

Date: 2026-11-13. Status: approach approved in chat (X11 everywhere, via XWayland on
Wayland desktops); builder check done; awaiting design approval.

## Intent

bloop runs on Linux with the same experience as on Windows: the notch at the top of the
screen, the tray icon, the global shortcuts, the app window, saved sign-ins, start on login
and self-update. It should work on every common desktop (GNOME, KDE Plasma,
Hyprland/Sway and other wlroots compositors, and plain X11 desktops such as Xfce,
Cinnamon and MATE) and on any mainstream distro.

Windows must not regress. Every Linux addition sits behind `cfg(target_os = "linux")`,
and CI builds and tests both platforms on every pull request.

## Goals

- One Linux build that runs on any mainstream x86_64 distro from roughly 2022 onwards
  (glibc 2.35, the Ubuntu 22.04 baseline).
- The notch looks and behaves as it does on Windows: borderless, transparent, shaped,
  always on top, placed by `NotchPosition`, and hidden while a fullscreen app is in front
  where the desktop allows bloop to detect it.
- Tray menu with the same items as Windows: show/collapse, Settings and Quit.
- Both global shortcuts (toggle and ask-from-clipboard) on X11 sessions and on Wayland
  desktops that offer the shortcuts portal.
- Sign-ins and API keys kept in the system keyring.
- Start on login, single instance, a second launch opens the app window, light/dark and
  reduced-motion follow the system, the save dialog and opening links work.
- Self-update when running as the AppImage.

## Non-goals

- macOS.
- Native Wayland drawing through the layer-shell protocol (sharp notch on KDE and
  wlroots desktops). It's a later upgrade on top of this design.
- Flatpak, Snap, `.deb` or `.rpm` packages. AppImage only for now.
- ARM (aarch64) builds.
- Pixel-perfect sharpness on Wayland with fractional scaling (see Constraints).

## Anti-goals

- Any change in Windows behaviour, look, installer or update flow.
- Storing secrets in plain text when no keyring is available. Without a keyring bloop
  says so and asks the user to install or unlock one.
- A Linux path that crashes or hangs when a desktop lacks a feature (tray, portal,
  XWayland). Missing features degrade with a one-time warning, as `platform::Event::Warning`
  does today.

## Constraints

- **Windows first.** Shared files (`launch.rs`, `update.rs`, `cli.rs`, `ui.rs`, `main.rs`,
  `platform.rs`) may change only in ways that leave Windows behaviour identical. Their
  existing Windows tests must keep passing unchanged.
- **Wayland rules.** On Wayland, an app can't put its own window on top, can't pick its
  position, and can't read global input. This is why bloop runs through XWayland there.
  XWayland windows keep those powers among other X11 windows, and GNOME and KDE honour
  "keep above" for them.
- **XWayland scaling.** With fractional scaling, compositors either upscale XWayland
  windows (slightly soft) or render them at native size (KDE and Hyprland options).
  Accepted.
- **GNOME has no tray** without the AppIndicator extension (included on Ubuntu, not on
  stock Fedora).
- **Global shortcuts on Wayland** only work through the `org.freedesktop.portal.GlobalShortcuts`
  portal: KDE Plasma 6, GNOME 48 and later, and Hyprland. The system, not bloop, has the
  final say on the key combination. Sway and older GNOME have no such portal.
- **GPU.** Windows uses DirectX 12 only. Linux uses Vulkan, with OpenGL ES as the fallback.
- **Toolchain.** Rust 1.92, edition 2024: `std::env::remove_var` is `unsafe` and must run
  before any thread starts.

## Approaches

1. **X11 everywhere, through XWayland on Wayland desktops (chosen).** One code path
   behaves the same on every desktop, GNOME included, and reuses Iced/winit's X11 backend
   as is. The cost is a slightly soft notch under fractional scaling, and it depends on
   XWayland, which every mainstream Wayland desktop ships.
2. **Native Wayland per desktop.** The layer-shell protocol gives a sharp, correctly
   placed notch on KDE and wlroots desktops, but Iced 0.14 can't create layer-shell
   surfaces. That would mean a second windowing path next to the existing one, and GNOME
   still needs approach 1 as a fallback. Much more code and testing. Deferred.
3. **Normal-window mode.** No notch on Linux, only tray, shortcut and app window. Quick,
   but it drops what makes bloop bloop. Rejected.

Picked by the user: approach 1.

## Design

### Backend selection (`main.rs`)

At the very start of `main`, before tracing, Tokio or Iced start any thread, on Linux
only:

- If `DISPLAY` is set, remove `WAYLAND_DISPLAY` from the environment, so winit picks X11
  (XWayland on a Wayland session).
- Record whether the session is Wayland (`XDG_SESSION_TYPE == "wayland"` or
  `WAYLAND_DISPLAY` was set) in a `platform::Session { X11, XWayland, WaylandOnly }`
  value. Shortcuts, fullscreen detection and warnings read it later.
- If `DISPLAY` isn't set (Wayland without XWayland), leave Wayland in place. The notch
  opens as an ordinary window (position and "on top" are up to the compositor), and bloop
  shows one warning: "Install XWayland for the notch to stay on top."
- `BLOOP_BACKEND=wayland` skips the override, for testing and as an escape hatch.
- The original `WAYLAND_DISPLAY` is kept in `platform::Session` and put back into the
  environment of every process bloop starts (`xdg-open`, CLI providers, the relaunch after
  an update), so browsers and CLIs still run as native Wayland apps.

Cargo: Iced is built with `default-features = false`, which leaves out its `x11` and
`wayland` features. Linux gets its own `iced` dependency entry under
`cfg(target_os = "linux")` that adds `x11` and `wayland` (the latter for the WaylandOnly
fallback and `BLOOP_BACKEND=wayland`). The Windows feature set doesn't change.

### Notch window (`ui.rs`, `platform.rs`)

Iced 0.14's Linux `platform_specific` only offers `application_id` and
`override_redirect`. It can't set the X11 window type or keep a window out of the
taskbar. So on Linux:

- `notch_settings()` creates the notch with `visible: false` and `application_id: "bloop"`.
- Right after it opens, `platform::prepare_notch` uses `x11rb` on the window's X11 handle to
  set `_NET_WM_WINDOW_TYPE` to `UTILITY` (my call: `DOCK` windows usually can't take
  keyboard focus on Mutter, and the prompt needs it) and `_NET_WM_STATE_SKIP_TASKBAR` and
  `_NET_WM_STATE_SKIP_PAGER`. Then the window is shown. Window managers read these when
  the window is first shown, which is why it starts hidden.
- `level: AlwaysOnTop` already maps to `_NET_WM_STATE_ABOVE`.
- On Windows, `notch_settings()` is unchanged.

**Keyboard focus on Wayland sessions.** When a native Wayland app has focus, GNOME and KDE
may refuse to hand keyboard focus to an XWayland window that asks for it. So a shortcut
or `bloop --toggle` can open the notch without the prompt getting focus. Accepted
limitation (my call): clicking the notch always focuses the prompt, and the "ask" flow
still pre-fills the clipboard text, so only the first click is extra. The builder records
per desktop whether focus arrives in `docs/verification.md`.

`platform::shape` on Linux uses the X11 SHAPE extension (`x11rb`, `shape` feature):
the outline in logical points is scaled by the window's scale factor and set as both the
bounding shape and the input shape. Clicks outside the outline go to the window
underneath, as on Windows. If SHAPE is missing, it does nothing; the window stays
transparent wherever a compositor is running.

### Tiling desktops (Hyprland, Sway and other tiling window managers)

Tiling compositors tile XWayland windows and Hyprland ignores `_NET_WM_STATE_ABOVE`, so
the notch needs a window rule there (decided with the user: a config rule is fine;
`override_redirect` is not used, because it breaks keyboard focus for the prompt).

- On start, bloop reads `XDG_CURRENT_DESKTOP` (and `HYPRLAND_INSTANCE_SIGNATURE`,
  `SWAYSOCK`). On Hyprland or Sway it checks once whether the notch ended up floating
  and pinned on top, where that can be detected; if not, it shows the rule once, as a
  dismissable message in the app window with a Copy button.
- Hyprland rule: float, pin, no focus-stealing restrictions and no border, matched on
  `class:^(bloop)$` and the notch's title. Sway rule: `floating enable`, `sticky enable`,
  `border none`, matched on `instance="bloop"` and the notch's title. The builder writes
  and tests the exact lines and puts them in the README too.
- The notch window gets a distinct title (for example `bloop notch`) so rules can match
  it without also catching the app window.
- Other tiling X11 window managers (i3 and similar) get the same treatment where the
  rule is known (`i3`: `for_window ... floating enable, sticky enable, border none`).

### Placement (`platform::place`, `place_on`, `primary_position`)

Uses RandR (`x11rb`, `randr` feature) for the monitors and the primary monitor, and
`_NET_WORKAREA` for the work area where the window manager provides it. The rectangle
maths stays in the existing shared `physical_rect` function, which already has tests.

"Active monitor" means the one under the pointer (`QueryPointer`). Under XWayland the
pointer position is only fresh while the pointer is over an X11 window, so on Wayland
sessions this falls back to the monitor the notch is already on.

### Fullscreen hiding (`platform::fullscreen_active`)

On X11: true when `_NET_ACTIVE_WINDOW` has `_NET_WM_STATE_FULLSCREEN` and covers the
notch's monitor. On Wayland sessions X11 can't see native Wayland windows, so this only
catches fullscreen X11 apps. Native fullscreen Wayland apps are drawn above "keep above"
X11 windows by GNOME and KDE in normal use; the builder verifies this per desktop and
records the result in `docs/verification.md`. A screen saver or lock screen always covers
the notch on every desktop.

### Tray (`platform::events`)

`ksni` (pure Rust, StatusNotifierItem over D-Bus) with the same menu as Windows:
"Show / collapse bloop", "Settings", "Quit bloop". Left click toggles. If no tray host is
running, bloop sends one `Event::Warning`: "No tray found. On GNOME, enable the
AppIndicator extension." Everything else keeps working.

### Global shortcuts

- **X11 session:** `global-hotkey` (X11 backend). Same string format and the same
  `validate_shortcut` as Windows.
- **Wayland session with the shortcuts portal:** `ashpd` `GlobalShortcuts`. bloop registers
  two shortcuts, `toggle` and `ask`, and suggests the user's configured key combination.
  The desktop shows its own dialog the first time and may assign a different
  combination. The dashboard's shortcut setting shows the combination the portal
  reports, plus a note that the system manages it.
- **Wayland session without the portal (Sway, GNOME before 48):** no global shortcut.
  bloop warns once and explains how to bind `bloop --toggle` and `bloop --ask` to a key in
  the desktop's own keyboard settings.
- `--toggle` and `--ask` are new launch arguments. `launch_kind` gets `Launch::Toggle` and
  `Launch::Ask` variants with new unit tests; the existing tests stay as they are. On
  Linux they send the command to the running copy (see Single instance) and exit. On
  Windows `main.rs` treats them exactly like `Manual`, so Windows behaviour is unchanged.
- **App ID.** On first run the Linux build writes `~/.local/share/applications/bloop.desktop`
  (respecting `XDG_DATA_HOME`) and the icon into `~/.local/share/icons/hicolor/256x256/apps/`,
  with `Exec` pointing at `$APPIMAGE` or the current executable, and rewrites them when
  that path changes (my call). GNOME's shortcuts portal identifies apps by their desktop
  file, and the tray, autostart and app launcher all work better with one. Where
  xdg-desktop-portal 1.19 or later is running, bloop also registers the ID through the
  host Registry portal.

`Controller` gets a Linux implementation that forwards `shortcut` and `ask_shortcut`
to the shortcut backend's thread or task.

### Single instance and second launch

A Unix socket at `$XDG_RUNTIME_DIR/bloop/instance.sock`, falling back to
`/tmp/bloop-$UID/instance.sock` with the directory mode set to 0700.

- The first copy binds the socket. `platform::Instance` owns it and removes it on drop.
- A later copy connects and sends one line: `open`, `toggle` or `ask`. A plain launch
  sends `open`, a `--background` launch sends nothing and exits, matching Windows.
- A stale socket (connect is refused) is removed and bound again.
- `open_requests()` yields an event for each command received.

### Start on login

`~/.config/autostart/bloop.desktop` (respecting `XDG_CONFIG_HOME`) with
`Exec="<path>" --background`. `<path>` is `$APPIMAGE` when set, otherwise the current
executable. `migrate_autostart` rewrites the `Exec` line when the AppImage has moved,
mirroring the Windows Run-key migration (`needs_migration` is already shared).

### System settings

`ashpd` Settings portal:

- `light_theme_system`: `org.freedesktop.appearance` `color-scheme` (1 = dark,
  2 = light, 0 = no preference, which counts as light).
- `reduce_motion_system`: `org.gnome.desktop.interface` `enable-animations` where the
  portal exposes it; otherwise false.
- Without the portal, both return the existing defaults.

### Dialogs, links and fatal errors

- `save_file_dialog`: `ashpd` FileChooser portal `SaveFile`. Without the portal it returns
  an error, and the export UI shows it as for any other failed save.
- `open_url`: keeps the same allow-list, then runs `xdg-open` with the URL as one argument
  (no shell).
- `show_fatal`: logs, prints to stderr and tries `zenity --error` or `kdialog --error`
  if either is installed. The text drops "DirectX 12" on Linux and says a Vulkan or
  OpenGL-capable graphics driver is required.
- `apply_app_icon`: sets `_NET_WM_ICON` through winit's window icon from the embedded
  PNG.

### Secrets (`secrets.rs`)

`oo7` against the Secret Service (GNOME Keyring, KWallet 6, KeePassXC). One item per
secret, labelled `bloop: <name>`, with attribute `application = bloop`. The Secret
Service has no size limit worth worrying about, so the DPAPI split for large OAuth
sessions isn't needed: one store for everything. If no Secret Service is available, or
the keyring is locked and the user dismisses the unlock prompt, saving and loading return
a clear error that the existing UI shows. Never plain text.

Today the non-Windows stubs return "available on Windows" errors, and ChatGPT sign-in goes
through `chatgpt_blob(bytes, encrypt)`, which encrypts and decrypts with DPAPI. Windows
keeps that path. On Linux the ChatGPT session is stored and loaded as a Secret Service
item instead, so the call site in `chatgpt.rs` goes through a small
store/load/delete function in `secrets.rs` whose Windows version wraps the existing
DPAPI code unchanged.

### CLI providers (`cli.rs`)

- `find_in`: on Linux, the first `<name>` in `PATH` that is a regular file with an execute
  bit. The Windows extension list is unchanged.
- Process cleanup: on Linux, children start in their own process group
  (`CommandExt::process_group(0)`) and `Job`'s drop sends `SIGKILL` to the group, so
  helper processes a CLI spawns die with it. The child also gets `PR_SET_PDEATHSIG`
  (via `pre_exec` and `libc::prctl`) so it dies if bloop crashes.

### GPU (`Cargo.toml`)

Linux gets its own `wgpu` dependency entry with `vulkan` and `gles` features, so the
Windows-only `dx12` selection stays untouched.

### Packaging and release

- `installer/linux/`: `bloop.desktop`, a 256 px PNG icon generated from the existing art,
  and `build-appimage.sh`, which builds `target/release/bloop` and packages
  `bloop-<version>-x86_64.AppImage` with `appimagetool`, plus a `.sha256` file in the same
  format as the Windows checksum.
- `release.yml`: a second job, `build-linux`, on `ubuntu-22.04` (the glibc baseline)
  running fmt check, clippy, tests and the AppImage build, and uploading the AppImage as
  an artifact. Publishing moves out of the Windows job into a third job, `publish`, which
  runs on tags after both builds, downloads both artifacts and creates one GitHub release
  with both platforms' files (my call). The Windows job's build, test and signing steps
  stay as they are; it now uploads its signed files as an artifact instead of
  publishing them. The release notes gain one line saying the AppImage isn't signed and
  how to check its SHA-256.
- The Linux job also runs on pull requests, with the same path filters, so Windows-only
  changes that break the Linux build are caught too.

### Self-update (`update.rs`)

- Asset names become per platform: Windows keeps `bloop-setup-<v>.exe`; Linux looks for
  `bloop-<v>-x86_64.AppImage`. The parse and checksum logic is shared, and the existing
  Windows tests keep their exact expectations.
- `MAX_INSTALLER` becomes per platform. Windows keeps 50 MB, so its tests don't change;
  Linux gets a limit that fits the measured AppImage size with headroom.
- Linux uses the same UI states as Windows (my call): it downloads and checks the update
  in the background, shows the existing "Ready, restart to update" state, and installs on
  restart. The install step, only when `$APPIMAGE` is set, writes the new file next to
  the current AppImage as `.bloop-update.tmp`, sets the execute bit, `rename`s it over
  `$APPIMAGE` (atomic on the same filesystem), starts it with `--background`, and quits.
- If the AppImage's folder isn't writable, or bloop isn't running as an AppImage (built
  from source, unpacked), the update check still shows "update available" but offers to
  open the release page instead of installing.

## Verification

- `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass on
  both Windows and Linux CI.
- Windows: the existing manual checks in `docs/verification.md` are rerun on the final
  branch.
- Linux: a new section in `docs/verification.md` with one table per desktop (GNOME
  Wayland, KDE Plasma Wayland, Hyprland or Sway, an X11 desktop such as Xfce) covering
  notch placement, staying on top, shape and click-through, fullscreen hiding, tray,
  both shortcuts, single instance, autostart, keyring, theme, export dialog, links and
  self-update. Each result is recorded as pass, fail or "not possible on this desktop".
- This machine has WSL Ubuntu with WSLg, which is enough for build and smoke tests (X11
  through XWayland). Full desktop checks need real Linux machines or VMs.

## Left to the builder

- The exact X11 window type for the notch (dock or utility), chosen by testing.
- Crate versions within the families named here (`x11rb`, `ksni` 0.3, `ashpd` 0.14,
  `oo7` 0.7, `libc`).
- How the Linux code is split inside `platform.rs` (for example a `platform/linux.rs`
  submodule), as long as the public functions keep their signatures.
- The AppImage build tooling details (`appimagetool` versus `linuxdeploy`).
