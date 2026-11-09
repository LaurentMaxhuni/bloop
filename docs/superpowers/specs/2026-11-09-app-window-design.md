# App window: onboarding and dashboard outside the notch

Date: 2026-11-09. Status: approved in chat, awaiting spec review.

## Goal

bloop gets a normal desktop app window for setup and configuration. The notch and its
pop-out panel become purely for using bloop (chatting). Today every setting lives in the
notch's "Customize" hub (`src/ui/hub.rs`) and first-run onboarding is drawn inside the
notch (`src/ui/onboarding.rs`). Launching bloop from Start while it runs does nothing
visible: the second copy exits silently (`main.rs`, `platform::single_instance`).

Success: a first-time user is set up in a real window; a returning user who opens bloop
from Start lands on a dashboard; signing in to Windows shows only the notch; the notch's
settings icon always gets them back to the window.

## Launch routing

| Launch | Shows |
|---|---|
| First launch (`config.onboarded == false`), any launch kind | App window on onboarding; notch hidden until onboarding finishes |
| Manual launch (Start, desktop shortcut, installer "Launch bloop") after onboarding | Notch + app window on the dashboard Home page |
| `--background` launch after onboarding (autostart, self-update relaunch) | Notch only |
| Any launch while bloop already runs | Existing instance opens/focuses the app window; new process exits |
| Notch settings icon, tray "Settings" | App window on the dashboard |

`--background` is the only new argument. Unknown arguments are ignored. Parsing is a pure
function `launch_kind(args) -> Launch { Manual, Background }` with unit tests, and the
decision table above is a pure function of `(Launch, onboarded)` with unit tests.

Installer (`installer/bloop.iss`):
- Run key value becomes `"{app}\bloop.exe" --background`. Inno rewrites it on every
  install, so existing users get it on their next self-update.
- The self-update relaunch `[Run]` entry gets `Parameters: "--background"`.
- The postinstall "Launch bloop" entry stays without arguments, so a fresh install opens
  onboarding.

## Second-instance activation

`platform::single_instance` keeps the `Local\bloop-app` mutex (Inno's update flow depends
on it). Added: a named auto-reset event `Local\bloop-open`.

- First instance: a thread waits on the event and sends `platform::Event::OpenWindow` to
  the UI through the existing platform event channel.
- Second instance: calls `AllowSetForegroundWindow(ASFW_ANY)`, sets the event, exits 0.
  A second instance started with `--background` exits without signalling.
- All Win32 stays in `platform.rs`. Non-Windows: no-op.

## Windows and the app shell

Switch from `iced::application` to `iced::daemon` (Iced 0.14 multi-window). One process,
one `Island` state, two windows identified by `window::Id`:

- **Notch window**: created exactly as today (same `window::Settings`), at startup unless
  onboarding is pending, in which case it opens when onboarding finishes.
- **App window**: opened on demand with `window::open`. Decorated, resizable, shown in the
  taskbar, normal level, centered, default 900 × 620 logical px, minimum 720 × 520. Title
  "bloop". Uses the existing `installer/art/bloop.ico` via the exe's embedded icon.
  If it is already open, opening again focuses it (`window::gain_focus`) and restores it
  if minimized.
- `view(&self, id)` and `theme(&self, id)` dispatch on the window id. The notch keeps its
  transparent background; the app window gets the opaque theme background.
- Closing the app window (title bar X, `CloseRequested` for its id) closes only that
  window; its id is cleared. bloop keeps running. Closing the notch window is not
  possible from the UI (unchanged). Quit remains in the tray and still waits for the
  save as today.
- The daemon does not exit when the app window closes; it exits only through the existing
  Quit path.

Window-specific state (which window ids exist, current dashboard page) lives on `Island`.
`ui.rs` is 2,515 lines; the new window code goes in new modules rather than growing it:

- `src/ui/app_window.rs`: open/focus/close handling, sidebar shell, page dispatch.
- `src/ui/dashboard.rs`: Home page and the page bodies moved from `hub.rs`.
- `src/ui/onboarding.rs`: kept, re-laid out for the larger window.
- `src/ui/hub.rs`: removed once its pages move to `dashboard.rs`.

## Onboarding

Same steps and logic (`src/onboarding.rs`, unchanged): Welcome, Model, How it works,
Make it yours, All set. Drawn in the app window with a centered content column (max
560 px) and the mascot. Finishing sets `config.onboarded = true` (existing
`finish_onboarding`), then: opens the notch window, peeks the notch so the user sees where
bloop lives, and switches the app window to the dashboard Home page (it stays open).
Closing the window mid-onboarding leaves `onboarded = false`, opens the notch, and the
next launch of any kind shows onboarding again. The existing "run setup again" action
(sets `onboarded = false`) moves to the dashboard and reopens onboarding in the window.

## Dashboard

Left sidebar (icons from the existing Lucide subset, word labels as fallback) and a
scrollable page area.

- **Home**: connection status of the configured provider (reuses the existing readiness
  check the notch uses), provider and model, global shortcut, version and update status
  with the existing install action. Each item links to its page.
- **Appearance**: today's Look, Motion and Notch hub pages, as sections. Apply live, as now.
- **Model**: today's Model page fields (provider, base URL, model, API key via
  `secrets.rs`) with its Save button and validation.
- **Shortcut & startup**: global shortcut (with the existing conflict warning) and
  "Start bloop when I sign in". The toggle writes or removes the HKCU Run value
  (`"<exe>" --background`) through a new `platform::set_autostart(bool)`; its state is
  read from the registry, not stored in SQLite.
- **About**: version, update checks on/off, check now, license, data folder path,
  run setup again.

All settings keep their current storage (`Config` in SQLite) and save path. Because both
windows render from the same `Island`, changes show in the notch immediately.

## Notch changes

- Remove the hub pages and the in-notch onboarding from the notch's views.
- The header's settings icon (and tray "Settings", and `Message::Settings`) now opens
  the app window on the dashboard instead of the hub.
- Everything else (chat, mascot, motion, peeks, fullscreen hiding) is unchanged.

## Errors

- If `window::open` fails, show the existing desktop warning text in the notch and log it.
- If the activation event cannot be created, the first instance runs without it and logs
  a warning; a second launch then exits silently, as today.
- If the Run key write fails, the toggle reverts and the page shows the error inline.

## Testing

- Unit: `launch_kind` parsing; the routing table for every `(Launch, onboarded)` pair.
- UI state tests (existing style in `ui.rs`): `Message::Settings` requests the app window;
  opening twice focuses rather than duplicates; closing the app window keeps the app
  running; finishing onboarding opens the notch and switches to Home; closing mid-setup
  keeps `onboarded == false`.
- Moved tests: existing hub and onboarding tests follow their code to the new modules.
- Manual verification (added to `docs/verification.md`): fresh install → onboarding
  window; sign out/in → notch only; Start launch while running → window focuses; update
  relaunch → notch only; notch icon and tray open the window.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

## Out of scope

Chat in the app window, chat history browser, multiple monitors for the window, a
separate settings process, and any change to provider or update behaviour.
