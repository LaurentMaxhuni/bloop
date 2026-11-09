# App Window Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move onboarding and all settings out of the notch into a normal desktop window, route each kind of launch (first run, manual, sign-in, update relaunch, second launch) to the right windows, and keep the notch for chatting only.

**Architecture:** One process, one `Island` state, two Iced windows under `iced::daemon` (the notch window, created as today, and an on-demand decorated app window). A pure `bloop::launch` module decides what opens; `platform.rs` gains a named Win32 event so a second launch asks the running copy to show its window. The hub's pages move into a sidebar dashboard in the app window.

**Tech Stack:** Rust stable, Iced 0.14 (`daemon`, `window::open`, multi-window views), `windows` 0.62, SQLite via existing `storage`, Inno Setup.

**Spec:** `docs/superpowers/specs/2026-11-09-app-window-design.md`

## Global Constraints

- One process, two windows via `iced::daemon`; no separate settings executable.
- The `Local\bloop-app` mutex stays (Inno's update flow waits on it). New named event: `Local\bloop-open`.
- `--background` is the only new argument; unknown arguments are ignored.
- App window: decorated, resizable, in taskbar, normal level, centered, default 900 × 620 logical px, minimum 720 × 520, title "bloop", icon from the exe's embedded resource (id 1, `installer/bloop.rc`).
- Closing the app window never quits bloop; Quit stays in the tray (and Alt+F4 on the notch, as today).
- All Win32 calls stay in `platform.rs`; non-Windows builds compile with no-op stubs, as existing functions do.
- Settings keep their current storage (`Config` in SQLite) and save path; Look/Motion/Notch apply live, Model keeps its Save button.
- `src/ui.rs` (2,515 lines) must not grow with window code: new code goes in `src/ui/app_window.rs` and `src/ui/dashboard.rs`; `src/ui/hub.rs` is deleted at the end.
- Each task ends green on `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

## Review Focus

1. **Pointer and focus in the app window leaking into the notch.** Today `Message::Cursor`/`Focus` come from any window; hovering the dashboard must not pop the pill, and focusing it must not count as notch activity. Test in Task 3 (`app_window_cursor_does_not_touch_the_notch`).
2. **Escape and the close button in the app window.** Escape there must not cancel a streaming reply or collapse the notch; X on the app window must not quit. Tests in Task 3 (`escape_in_app_window_leaves_the_notch_alone`, `closing_app_window_keeps_running`).
3. **Re-opening an already-open window discarding an unsaved Model draft.** Settings icon, tray and second launch all focus the existing window without resetting `draft_config`/`key_draft`. Test in Task 4 (`reopening_keeps_unsaved_model_draft`).
4. **A second launch during onboarding, or while the window is minimized.** Must focus/restore the window on the onboarding step it was on, not restart setup. Test in Task 5 (`open_request_during_setup_keeps_the_step`).
5. **Upgrading users whose Run value has no `--background`.** Until the next installer run they see the dashboard at sign-in; the in-app autostart toggle must rewrite the value with the flag. Test in Task 2 (`run_command_has_background_flag`).

---

### Task 1: Launch routing (pure)

**Files:**
- Create: `src/launch.rs`
- Modify: `src/lib.rs` (add `pub mod launch;`)
- Test: unit tests inside `src/launch.rs`

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Launch { Manual, Background }`
  - `pub fn launch_kind(args: impl IntoIterator<Item = String>) -> Launch` — `Background` iff any argument equals `--background`; the first item (program path) is skipped.
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Startup { Onboarding, Dashboard, NotchOnly }`
  - `pub fn startup(launch: Launch, onboarded: bool) -> Startup`
  - `pub fn signals_running_copy(launch: Launch) -> bool` — `true` for `Manual` only.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn no_args_is_manual() { assert_eq!(launch_kind(args(&["bloop.exe"])), Launch::Manual); }
#[test] fn background_flag() { assert_eq!(launch_kind(args(&["bloop.exe", "--background"])), Launch::Background); }
#[test] fn unknown_args_ignored() { assert_eq!(launch_kind(args(&["bloop.exe", "--foo", "x"])), Launch::Manual); }
#[test] fn program_path_named_like_flag_is_not_the_flag() { assert_eq!(launch_kind(args(&["--background"])), Launch::Manual); }
#[test] fn routing_table() {
    assert_eq!(startup(Launch::Manual, false), Startup::Onboarding);
    assert_eq!(startup(Launch::Background, false), Startup::Onboarding);
    assert_eq!(startup(Launch::Manual, true), Startup::Dashboard);
    assert_eq!(startup(Launch::Background, true), Startup::NotchOnly);
}
#[test] fn only_manual_launches_signal() {
    assert!(signals_running_copy(Launch::Manual));
    assert!(!signals_running_copy(Launch::Background));
}
```
(`args` is a test helper mapping `&[&str]` to `Vec<String>`.)

- [ ] **Step 2: Run** `cargo test --lib launch` — Expected: FAIL (module missing).
- [ ] **Step 3: Implement** the three functions in `src/launch.rs` with a `//!` doc line naming the spec.
- [ ] **Step 4: Run** `cargo test --lib launch` — Expected: 6 passed.
- [ ] **Step 5: Commit** `feat: launch routing for the app window`

### Task 2: Platform — open requests, background autostart, app icon, installer

**Files:**
- Modify: `src/platform.rs` (single instance ~452-500, autostart ~502-580)
- Modify: `Cargo.toml` only if a needed `windows` feature is missing (`Win32_System_LibraryLoader` for `GetModuleHandleW`)
- Modify: `installer/bloop.iss` (`[Registry]` RunKey value, the `Check: Relaunch` `[Run]` entry)
- Test: unit tests in `src/platform.rs`

**Interfaces:**
- Consumes: `bloop::launch::Launch` (Task 1).
- Produces:
  - `pub fn single_instance() -> Option<Instance>` — unchanged signature; additionally creates the auto-reset event `Local\bloop-open` and keeps its handle in `Instance` (closed in `Drop`).
  - `pub fn signal_running_copy()` — opens `Local\bloop-open`, calls `AllowSetForegroundWindow(ASFW_ANY)`, `SetEvent`; ignores failures (logs at `warn`).
  - `pub fn open_requests() -> impl iced::futures::Stream<Item = ()>` — `iced::stream::channel`; a dedicated `std::thread` opens the same event and loops on `WaitForSingleObject(INFINITE)`, sending `()` per signal. If the event cannot be opened, log a warning and never yield. Non-Windows: a stream that never yields.
  - `fn run_command(exe: &std::path::Path) -> String` — returns `"\"<exe>\" --background"`; `set_autostart(true)` writes this.
  - `pub fn apply_app_icon(window: &dyn iced::window::Window)` — `LoadImageW(GetModuleHandleW(None), MAKEINTRESOURCE(1), IMAGE_ICON, …)` for big (`SM_CXICON`) and small (`SM_CXSMICON`) sizes, `SendMessageW(hwnd, WM_SETICON, ICON_BIG/ICON_SMALL, …)`. Gets the HWND the same way `shape` does. No-op if loading fails or off Windows.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn run_command_has_background_flag() {
    let cmd = run_command(std::path::Path::new(r"C:\Apps\bloop\bloop.exe"));
    assert_eq!(cmd, r#""C:\Apps\bloop\bloop.exe" --background"#);
}
```

- [ ] **Step 2: Run** `cargo test --bin bloop run_command` — Expected: FAIL.
- [ ] **Step 3: Implement** `run_command` and use it in `set_run_value` (replaces the `format!("\"{}\"", exe.display())` at ~line 571). `autostart()` keeps checking only that the value exists.
- [ ] **Step 4: Implement** the event handle in `Instance`/`instance_named`, `signal_running_copy`, `open_requests`, `apply_app_icon`.
- [ ] **Step 5: Update `main.rs`** — parse `let launch = bloop::launch::launch_kind(std::env::args());` before `single_instance()`; when it returns `None`, call `platform::signal_running_copy()` if `bloop::launch::signals_running_copy(launch)`, then return. (The app is still single-window here; `launch` is passed to `Island` in Task 3.)
- [ ] **Step 6: Installer** — RunKey `ValueData: """{app}\bloop.exe"" --background"`; add `Parameters: "--background"` to the `Check: Relaunch` `[Run]` entry; leave the postinstall "Launch bloop" entry without parameters.
- [ ] **Step 7: Run** `cargo test`, `cargo clippy --all-targets -- -D warnings` — Expected: all pass.
- [ ] **Step 8: Manual check** — `cargo build --release`; start `target\release\bloop.exe`, start it again: second process exits (window opening arrives in Task 3; for now confirm with `RUST_LOG=bloop=debug` a log line from the `open_requests` thread you add at `debug`).
- [ ] **Step 9: Commit** `feat: second-launch signal and background autostart`

### Task 3: Daemon with notch and app windows

**Files:**
- Modify: `src/main.rs` (switch to `iced::daemon`, move notch `window::Settings` out)
- Modify: `src/ui.rs` (`Island` fields, `new`, `subscription`, `update` handlers for Window/Cursor/Focus/Escape/close, `view`/`theme`/`title` dispatch)
- Create: `src/ui/app_window.rs`
- Test: `src/ui.rs` tests module

**Interfaces:**
- Consumes: `Launch`, `Startup`, `startup` (Task 1); `platform::open_requests`, `platform::apply_app_icon` (Task 2).
- Produces:
  - `Island::new(launch: Launch) -> (Self, Task<Message>)`; field `launch: Launch`; existing field `window` keeps meaning "notch window id"; new field `app_window: Option<window::Id>`.
  - `pub fn notch_settings() -> window::Settings` in `ui.rs` (the settings now in `main.rs`, unchanged values).
  - In `app_window.rs`: `pub(super) fn app_settings() -> window::Settings` (Global Constraints values; `exit_on_close_request: false`); `impl Island { pub(super) fn open_app_window(&mut self) -> Task<Message>; pub(super) fn open_notch(&mut self) -> Task<Message>; pub(super) fn app_view(&self) -> Element<'_, Message> }`.
  - `pub fn view(&self, id: window::Id)`, `pub fn theme(&self, id: window::Id) -> Theme`, `pub fn title(&self, id: window::Id) -> String` ("bloop" for both).
  - Messages: `OpenWindow` (from tray Settings, open requests, notch icon), `CloseRequested(window::Id)`, `WindowClosed(window::Id)`; `Cursor(window::Id, bool)`, `Focus(window::Id, bool)`, `Escape(window::Id)` replace the id-less variants.

Behaviour to pin:
- `new` no longer calls `window::oldest()`; on `Loaded`, `match startup(self.launch, self.config.onboarded)`: `NotchOnly` → `open_notch`; `Dashboard` → `open_notch` + `open_app_window`; `Onboarding` → for now same as today (`open_notch` + `start_setup` in the notch; Task 5 moves it).
- `open_notch`: no-op if `window` is `Some`; else `window::open(notch_settings())`, store id, map the open task to `Message::Window(Some(id))` so `place_window` runs.
- `open_app_window`: if open → `window::minimize(id, false)` + `window::gain_focus(id)`; else open, store id, `window::run(id, platform::apply_app_icon)`.
- `CloseRequested(id)`: app window → `window::close(id)`, `app_window = None`; notch → `Message::Quit` (today's Alt+F4 behaviour). `WindowClosed(id)` clears whichever id matches.
- `Cursor`/`Focus`/`Escape` with an id other than `self.window` are ignored. Build them from `iced::event::listen_with(|event, _, id| …)` (third arg is the window id); replace `keyboard::listen` for Escape the same way.
- `theme(id)` returns the same theme for both. The daemon `.style` keeps a transparent background; `app_view` paints its own full-size `container` with `t.background` so the decorated window is opaque.
- `app_view` for this task: a placeholder column with the title "bloop" (replaced in Task 4).
- Tests: `ready()` becomes `Island::new(Launch::Manual)` and sets `app.window = Some(window::Id::unique())`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn background_start_opens_only_the_notch() {
    let mut app = Island::new(Launch::Background).0;
    let _ = app.update(Message::Loaded(Ok((Config { onboarded: true, ..Config::default() }, vec![]))));
    assert!(app.window.is_some()); assert!(app.app_window.is_none());
}
#[test] fn manual_start_opens_notch_and_window() { /* same with Launch::Manual */
    assert!(app.window.is_some()); assert!(app.app_window.is_some());
}
#[test] fn open_window_twice_keeps_one_window() {
    let mut app = ready(); let _ = app.update(Message::OpenWindow);
    let first = app.app_window; let _ = app.update(Message::OpenWindow);
    assert_eq!(app.app_window, first);
}
#[test] fn closing_app_window_keeps_running() {
    let mut app = ready(); let _ = app.update(Message::OpenWindow);
    let id = app.app_window.unwrap(); let _ = app.update(Message::CloseRequested(id));
    assert!(app.app_window.is_none()); assert!(!app.quitting);
}
#[test] fn app_window_cursor_does_not_touch_the_notch() {
    let mut app = ready(); let _ = app.update(Message::OpenWindow);
    let id = app.app_window.unwrap(); let before = app.presence.shape();
    let _ = app.update(Message::Cursor(id, true));
    assert_eq!(app.presence.shape(), before);
}
#[test] fn escape_in_app_window_leaves_the_notch_alone() {
    let mut app = ready(); let notch = app.window.unwrap();
    let _ = app.update(Message::Cursor(notch, true)); let _ = app.update(Message::OpenClick);
    let _ = app.update(Message::OpenWindow); let id = app.app_window.unwrap();
    let _ = app.update(Message::Escape(id));
    assert!(app.is_open());
}
#[test] fn closing_the_notch_still_quits() {
    let mut app = ready(); let notch = app.window.unwrap();
    let _ = app.update(Message::CloseRequested(notch));
    assert!(app.quitting);
}
```
(Adjust `Cursor`/`Focus`/`Escape` call sites in existing tests to pass `app.window.unwrap()`.)

- [ ] **Step 2: Run** `cargo test --bin bloop` — Expected: compile failure on the new messages/fields.
- [ ] **Step 3: Implement** the interfaces above; `main.rs` uses `iced::daemon(move || ui::Island::new(launch), ui::Island::update, ui::Island::view).title(ui::Island::title).theme(ui::Island::theme)…` keeping the executor, default font, style and subscription. Add `Subscription::run(platform::open_requests).map(|_| Message::OpenWindow)`, `window::close_requests().map(Message::CloseRequested)`, `window::close_events().map(Message::WindowClosed)`. `platform::Event::Settings` maps to `Message::OpenWindow`. Check exact 0.14 signatures against https://docs.rs/iced/0.14.0/iced/fn.daemon.html and https://docs.rs/iced/0.14.0/iced/window/index.html before writing.
- [ ] **Step 4: Run** `cargo test`, clippy — Expected: pass.
- [ ] **Step 5: Manual check** — release build: manual launch shows notch + placeholder window with bloop's icon in title bar and taskbar; X closes only the window; launching again re-opens it; `bloop.exe --background` shows only the notch; hovering the window does not pop the pill.
- [ ] **Step 6: Commit** `feat: run bloop as a two-window daemon`

### Task 4: Dashboard and removing the hub from the notch

**Files:**
- Create: `src/ui/dashboard.rs`
- Modify: `src/ui/app_window.rs` (`app_view` renders sidebar + page)
- Modify: `src/ui.rs` (remove `hub` field, `OpenHub`, hub branch in `panel_content`/`Escape`/`Settings`; header settings icon sends `OpenWindow`)
- Delete: `src/ui/hub.rs` (page bodies move to `dashboard.rs`)
- Test: `src/ui.rs` tests (move hub tests, add new)

**Interfaces:**
- Consumes: `open_app_window`, `app_window` (Task 3).
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum AppPage { Home, Appearance, Model, Shortcut, About }` with `title()` ("Home", "Appearance", "Model", "Shortcut & startup", "About") and `icon() -> Option<Icon>` using only glyphs already in the subset: Home `Sparkles`, Appearance `Palette`, Model `Cpu`, Shortcut `SlidersHorizontal`, About `None` (word label only). No new font glyphs.
  - Field `page: AppPage` on `Island`; `Message::ShowPage(AppPage)` replaces `OpenHub(HubPage)`.
  - `impl Island { pub(super) fn dashboard_view(&self) -> Element<'_, Message> }` in `dashboard.rs`.

Page contents (from spec):
- **Home**: provider + model (`badge_text`), connection/readiness (`pill_status`/`ready_text` as today), global shortcut, version + `update_status()` with the install button; each row has a button sending `ShowPage(..)` to its page.
- **Appearance**: the Look, Motion and Notch hub page bodies as three titled sections, same messages.
- **Model**: `model_fields()` + key field + Save (`Message::SaveSettings`), unchanged validation.
- **Shortcut & startup**: shortcut field (moved from Model page) with `desktop_warning` shown, autostart toggle (`Message::SetAutostart`, existing handler; `autostart_error` inline).
- **About**: version, auto-update toggle + check now (`updates_section` content), `LICENSE` note, data folder path (`self.db_path` parent), "Run setup again" (`Message::ShowSetup`).

Behaviour to pin:
- Opening the window when it is closed resets `draft_config`, `key_draft`, `key_status` (as `Settings` does today) and sets `page = Home` (unless onboarding, Task 5); focusing an already-open window changes neither draft nor page.
- `Message::Settings` is removed; everything that sent it sends `OpenWindow`.
- Notch header keeps theme toggle and Hide; the settings icon (`Icon::SlidersHorizontal`, tooltip "Settings") sends `OpenWindow`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn notch_settings_icon_opens_dashboard_home() {
    let mut app = ready(); let _ = app.update(Message::OpenWindow);
    assert!(app.app_window.is_some()); assert_eq!(app.page, AppPage::Home);
}
#[test] fn reopening_keeps_unsaved_model_draft() {
    let mut app = ready(); let _ = app.update(Message::OpenWindow);
    let _ = app.update(Message::ShowPage(AppPage::Model));
    let _ = app.update(Message::Model("draft-model".into()));
    let _ = app.update(Message::OpenWindow);
    assert_eq!(app.draft_config.model, "draft-model"); assert_eq!(app.page, AppPage::Model);
}
#[test] fn appearance_changes_apply_live() {
    let mut app = ready(); let _ = app.update(Message::OpenWindow);
    let _ = app.update(Message::SetNotchWidth(NotchWidth::L));
    assert_eq!(app.config.notch_width, NotchWidth::L);
}
```
Port existing hub tests (e.g. the one at ~line 2236 using `OpenHub(HubPage::Look)`) to `ShowPage(AppPage::Appearance)`.

- [ ] **Step 2: Run** `cargo test --bin bloop` — Expected: FAIL.
- [ ] **Step 3: Implement** `dashboard.rs` (sidebar 200 px fixed, page area `scrollable`, content max width 640 px) and wire `app_view` to it; remove the hub from the notch and delete `hub.rs`.
- [ ] **Step 4: Run** `cargo test`, clippy — Expected: pass.
- [ ] **Step 5: Manual check** — every page renders in light and dark; changing accent/width updates the notch live; Model Save validates as before; autostart toggle writes `"…\bloop.exe" --background` (check with `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v bloop`).
- [ ] **Step 6: Commit** `feat: dashboard window replaces the notch hub`

### Task 5: Onboarding in the app window

**Files:**
- Modify: `src/ui/onboarding.rs` (layout for the window: centered column, max width 560 px, mascot on top)
- Modify: `src/ui.rs` (`start_setup`, `finish_onboarding`, `Loaded` routing, `panel_content` no longer shows onboarding, `CloseRequested` during setup)
- Modify: `src/ui/app_window.rs` (`app_view` shows onboarding when `self.onboarding.is_some()`)
- Test: `src/ui.rs` tests (update `first_launch*` tests ~2116-2175, ShowSetup tests ~2471-2510)

**Interfaces:**
- Consumes: `startup` (Task 1), `open_app_window`, `open_notch` (Task 3), `AppPage` (Task 4).
- Produces: no new public names; `start_setup` now opens the app window instead of presenting the notch.

Behaviour to pin:
- `Startup::Onboarding` opens only the app window, on step one.
- Finishing: existing adoption logic unchanged, then `open_notch`, peek "I'm up here!", `page = Home`, window stays open.
- Closing the app window while `onboarding.is_some()`: `onboarding = None`, `config.onboarded` stays `false`, `open_notch`.
- `ShowSetup` (About page): starts onboarding in the open window; the notch stays.
- `OpenWindow` during setup focuses the window and keeps the current step.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn first_launch_shows_onboarding_in_the_window() {
    let app = first_launch();
    assert!(app.onboarding.is_some()); assert!(app.app_window.is_some()); assert!(app.window.is_none());
}
#[test] fn finishing_opens_notch_and_home() {
    let mut app = first_launch(); let _ = app.update(Message::OnboardSkip);
    assert!(app.config.onboarded); assert!(app.window.is_some());
    assert!(app.app_window.is_some()); assert_eq!(app.page, AppPage::Home);
}
#[test] fn closing_mid_setup_opens_notch_and_keeps_setup_pending() {
    let mut app = first_launch(); let id = app.app_window.unwrap();
    let _ = app.update(Message::CloseRequested(id));
    assert!(!app.config.onboarded); assert!(app.onboarding.is_none()); assert!(app.window.is_some());
}
#[test] fn open_request_during_setup_keeps_the_step() {
    let mut app = first_launch(); let _ = app.update(Message::OnboardNext);
    let step = app.onboarding.as_ref().unwrap().step();
    let _ = app.update(Message::OpenWindow);
    assert_eq!(app.onboarding.as_ref().unwrap().step(), step);
}
#[test] fn background_first_launch_still_onboards() {
    let mut app = Island::new(Launch::Background).0;
    let _ = app.update(Message::Loaded(Ok((Config::default(), vec![]))));
    assert!(app.onboarding.is_some()); assert!(app.app_window.is_some());
}
```

- [ ] **Step 2: Run** `cargo test --bin bloop` — Expected: FAIL.
- [ ] **Step 3: Implement** the behaviour above; delete the now-unused onboarding branch from `panel_content`.
- [ ] **Step 4: Run** `cargo test`, clippy — Expected: pass.
- [ ] **Step 5: Manual check** — `$env:BLOOP_DATA_DIR` to an empty temp folder, launch: onboarding window only; finish → notch appears and peeks, window shows Home; repeat and close mid-setup → notch appears, next launch shows onboarding again.
- [ ] **Step 6: Commit** `feat: onboarding runs in the app window`

### Task 6: Docs and end-to-end verification

**Files:**
- Modify: `README.md` (Run section: window, Start menu, sign-in behaviour; Implemented list)
- Modify: `docs/architecture.md` (new dated "App window (2026-11-09)" paragraph: daemon, two windows, `Local\bloop-open`, `--background`)
- Modify: `docs/verification.md` (new dated section with results)

- [ ] **Step 1:** Update README and architecture to match what shipped.
- [ ] **Step 2: Run** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo build --release` — Expected: all pass.
- [ ] **Step 3: Manual matrix** (record pass/fail in `docs/verification.md`): fresh data dir → onboarding window; manual launch after setup → notch + Home; `--background` → notch only; second launch while running (window closed, open, minimized) → window shows and focuses; tray Settings and notch icon → window; installer build (`installer/build.ps1`) → Run value carries `--background`; update relaunch entry carries `--background`.
- [ ] **Step 4: Commit** `docs: app window`
