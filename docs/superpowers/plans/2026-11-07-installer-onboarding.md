# Installer and Onboarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** bloop installs from one per-user `bloop-setup-<version>.exe` and greets first launches with a five-step setup inside the notch.

**Architecture:** Pure onboarding state lives in the library (`bloop::onboarding`); its Iced view lives in a child module of `ui` (`src/ui/onboarding.rs`) so it can read `Island`'s private fields. Win32 work (single-instance mutex, HKCU Run value) stays in `src/platform.rs`. Inno Setup packages the release exe; a PowerShell script draws the art and icon reproducibly.

**Tech Stack:** Rust 2024 / Iced 0.14 / reqwest 0.13 / `windows` 0.62, `embed-resource` (build-dependency), Inno Setup 6, PowerShell + System.Drawing.

**Spec:** `docs/superpowers/specs/2026-11-06-installer-onboarding-design.md`

## Global Constraints

- Installer is per-user: `PrivilegesRequired=lowest`, install dir `{localappdata}\Programs\bloop`.
- Output: `dist/bloop-setup-<version>.exe`, version read from `Cargo.toml`; `dist/` git-ignored.
- Single-instance mutex name: `Local\bloop-app` (Inno `AppMutex=bloop-app`).
- Autostart truth: value `bloop` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, data = quoted exe path.
- Uninstall prompt copy: "Also delete your bloop chats and settings?" default No; Yes deletes `%LOCALAPPDATA%\bloop`.
- Model detection timeout: 3 s; never blocks finishing.
- Onboarding copy is fixed by the spec: "Hi, I'm bloop.", "Rest your mouse on the sliver", "Click to chat", "Ctrl+Alt+B from anywhere", "Hover the top of your screen anytime.", final peek "I'm up here!".
- Win32 calls only in `src/platform.rs`; every `unsafe` block carries a `// SAFETY:` comment (clippy denies otherwise).
- Deviation from spec: wizard art ships as BMP (Inno's wizard image format), icon as ICO.

## Review Focus

1. A second bloop launched while one runs (e.g. installer "Launch bloop" while autostart copy is up) → the second exits silently; no two notches. Test in Task 2.
2. Ollama answers `/api/tags` with zero models → treated as "not found" path with fields shown, not an empty dropdown. Test in Task 3.
3. A huge or malformed model list body → bounded error, no hang or OOM. Test in Task 3.
4. User finishes onboarding with an invalid endpoint typed in step 2 → finishing still succeeds, previous valid config is kept, `onboarded = true`. Test in Task 4.
5. Detection result arriving after the user already left step 2 / finished → ignored, no panic, doesn't overwrite a model the user picked. Test in Task 4.

---

### Task 1: `onboarded` config field and pure onboarding state

**Files:**
- Modify: `src/config.rs` (field + default + test)
- Create: `src/onboarding.rs`
- Modify: `src/lib.rs` (`pub mod onboarding;`)

**Interfaces:**
- Produces: `Config.onboarded: bool` (default `false`).
- Produces: `bloop::onboarding::{Step, Onboarding, Detection}`:
  - `enum Step { Welcome, Model, HowItWorks, MakeItYours, AllSet }` (Copy, Eq, Debug)
  - `Onboarding::new() -> Self` (step Welcome); `step(&self) -> Step`; `index(&self) -> usize` (0..5); `next(&mut self)` (saturates at AllSet); `back(&mut self)` (saturates at Welcome); `is_first(&self) -> bool`; `is_last(&self) -> bool`; `const COUNT: usize = 5`.
  - `enum Detection { Checking, Found(Vec<String>), NotFound }` (Clone, Debug, PartialEq).

- [ ] **Step 1: Failing tests**
  - `config::tests::old_config_without_onboarded_loads_as_false`: `serde_json::from_str::<Config>(r#"{"provider":"Ollama","endpoint":"http://127.0.0.1:11434","model":"m","shortcut":"Ctrl+Alt+B","light":true}"#)` → `onboarded == false`, `light == true`; round-trip of `Config { onboarded: true, ..Default::default() }` keeps `true`.
  - `onboarding::tests::navigation_saturates`: new → Welcome, `is_first`; `back()` stays Welcome; four `next()` → AllSet, `is_last`, `index()==4`; fifth `next()` stays AllSet; `back()` → MakeItYours.
- [ ] **Step 2:** `cargo test --lib` → fails to compile (missing field/module).
- [ ] **Step 3:** Implement field and module.
- [ ] **Step 4:** `cargo test --lib` → PASS.
- [ ] **Step 5:** Commit `feat: onboarding state and onboarded config flag`.

### Task 2: Single instance and start-with-Windows (platform)

**Files:**
- Modify: `Cargo.toml` (add `"Win32_System_Registry"` to `windows` features)
- Modify: `src/platform.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Produces: `platform::single_instance() -> Option<Instance>` — `Instance` holds the mutex HANDLE and closes it on Drop; `None` when `CreateMutexW(Local\bloop-app)` reports `ERROR_ALREADY_EXISTS`. Non-Windows: always `Some`.
- Produces: `platform::autostart() -> bool`, `platform::set_autostart(enable: bool) -> Result<(), String>`; both delegate to private `run_value(key: &str) -> Option<String>` / `set_run_value(key: &str, enable: bool) -> Result<(), String>` with `key = RUN_KEY = r"Software\Microsoft\Windows\CurrentVersion\Run"`. Value name `bloop`, data `format!("\"{}\"", std::env::current_exe()?.display())`. Deleting a missing value is `Ok`. Non-Windows: `false` / `Err("Start with Windows is only available on Windows.")`.
- `main.rs`: `let Some(_instance) = platform::single_instance() else { return; };` first line of `main`.

- [ ] **Step 1: Failing tests (Windows-only, `#[cfg(windows)]`)**
  - `second_instance_is_refused`: first `single_instance()` is `Some`, second while first alive is `None`; after dropping first, a new one is `Some`. (Use a distinct test mutex name via a private `instance_named(name)` helper so a running bloop doesn't break the test.)
  - `run_value_round_trip`: with key `Software\bloop-test\Run`: `set_run_value(k, true)` → `run_value(k)` is `Some(s)` where `s` starts and ends with `"` and contains `current_exe` file name; `set_run_value(k, false)` → `None`; second disable is `Ok`. Clean up the test key at the end (`RegDeleteTreeW`).
- [ ] **Step 2:** `cargo test --bin bloop platform` → compile failure.
- [ ] **Step 3:** Implement with `CreateMutexW` / `GetLastError`, `RegCreateKeyExW` + `RegSetValueExW(REG_SZ)`, `RegDeleteKeyValueW`, `RegGetValueW(RRF_RT_REG_SZ)`.
- [ ] **Step 4:** Tests PASS; `cargo clippy --all-targets` clean.
- [ ] **Step 5:** Commit `feat: single instance and start-with-Windows`.

### Task 3: `provider::list_models`

**Files:**
- Modify: `src/provider.rs`
- Modify: `tests/transport.rs` (`read_request` must treat a missing `content-length` as 0, since GET has no body)

**Interfaces:**
- Produces: `pub async fn list_models(config: &Config) -> Result<Vec<String>, Error>`.
  - Validates endpoint with `validate_endpoint` (not full `config.validate()` — the model may be blank during setup).
  - Ollama: `GET {base}/api/tags` → `models[].name`. OpenAI: `GET {base}/models` → `data[].id`.
  - Client: connect 3 s, overall `timeout(3 s)`, no redirects, no proxy, bearer from `BLOOP_API_KEY` like `chat`.
  - Body cap `MAX_LIST = 1024 * 1024` bytes read via `chunk()`; over → `Error::Protocol("Model list is too large.")`. Non-2xx → `Error::Protocol("Provider returned HTTP {code}.")`. Bad JSON / wrong shape → `Error::Protocol("Provider returned an unexpected model list.")`.
  - Names: trimmed, empty skipped, deduplicated, original order, at most 500.

- [ ] **Step 1: Failing tests in `tests/transport.rs`**
  - `ollama_lists_models`: body `{"models":[{"name":"qwen3:4b"},{"name":"llama3.2"}]}` → `["qwen3:4b","llama3.2"]`; request starts with `GET /api/tags`.
  - `openai_lists_models_under_base_path`: endpoint `…/v1`, body `{"data":[{"id":"a"},{"id":"b"}]}` → `["a","b"]`; request starts `GET /v1/models`.
  - `list_models_failures_are_errors`: `500` status → Err; body `{"nope":1}` → Err; body of 1.1 MiB → Err.
  - `empty_model_list_is_ok_and_empty`: `{"models":[]}` → `Ok(vec![])` (UI maps empty to NotFound).
- [ ] **Step 2:** `cargo test --test transport` → compile failure.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test --test transport` → PASS (existing chat tests still pass).
- [ ] **Step 5:** Commit `feat: list models from Ollama and OpenAI-compatible providers`.

### Task 4: Onboarding view and flow

**Files:**
- Create: `src/ui/onboarding.rs` (`impl Island { fn onboarding_view(&self) -> Element<'_, Message> }` plus small step helpers)
- Modify: `src/ui.rs` (`mod onboarding;`, fields, messages, update arms, header/body switch, settings additions, tests)

**Interfaces:**
- Consumes: Task 1 types, `platform::autostart/set_autostart` (Task 2), `provider::list_models` (Task 3).
- New `Island` fields: `onboarding: Option<Onboarding>`, `detection: Detection`, `autostart: bool`, `autostart_error: String`.
- New `Message`s: `OnboardNext`, `OnboardBack`, `OnboardSkip`, `Detect`, `Detected(Result<Vec<String>, String>)`, `PickModel(String)`, `SetLight(bool)`, `SetAutostart(bool)`, `ShowSetup`.

Behaviour:
- `Loaded(Ok)` with `!config.onboarded` (and `Loaded(Err)` too — storage failure means setup shows each launch): set `onboarding = Some(new())`, `autostart = platform::autostart()`, open the panel (`present(Event::Shortcut)`), and start detection.
- `Detect`: `detection = Checking`, `Task::perform(list_models(&draft_config) mapped to String error, Detected)`. `Detected(Ok(v))` with non-empty `v` → `Found(v)`, and if `draft_config.model` is not in `v`, set it to `v[0]`; empty or Err → `NotFound`. Ignore `Detected` when `onboarding` is `None`.
- Step views: dots (filled for current), Back (hidden on first), Next/“Finish” (on last), “Skip setup” ghost button.
  1. Welcome: mascot at 96 px in `Mood::Happy`, "Hi, I'm bloop.", one line "A tiny assistant that lives at the top of your screen."
  2. Model: `Checking` → "Looking for Ollama…"; `Found` → `pick_list` of names → `PickModel`; `NotFound` → "Start Ollama, or use an OpenAI-compatible endpoint" + Retry (`Detect`) + the existing provider/endpoint/model fields (reuse the field helpers from `settings_view`; extract them to a shared fn).
  3. How it works: three `canvas(Notch{..})` miniatures at SLIVER/COMPACT/EXPANDED scaled ×0.25 with the three captions; shortcut `text_input` bound to `Message::Shortcut`.
  4. Make it yours: two preview cards (mini notch drawn with `Tokens::new(false)` / `Tokens::new(true)`) pressing `SetLight(bool)`; “Start with Windows” `toggler` → `SetAutostart`; `autostart_error` shown under it in destructive colour.
  5. All set: "Hover the top of your screen anytime."
- `SetLight(b)`: sets `config.light` and `draft_config.light` (live). `SetAutostart(b)`: call `set_autostart`; on Ok set `autostart = b` and clear error; on Err set `autostart_error`, leave `autostart`.
- `OnboardNext` on last step and `OnboardSkip` both call `finish_onboarding()`: if `draft_config` validates (`validate()` and `platform::validate_shortcut`), adopt it (and re-register shortcut via controller); else keep `config` (its light flag stays as chosen). Set `config.onboarded = true`, `draft_config = config.clone()`, `onboarding = None`, then `Task::batch([persist(), present(Event::Dismiss), present(Event::Peek("I'm up here!".into()))])`.
- While `onboarding.is_some()` the header hides Light/Settings buttons and the body is `onboarding_view()`. Escape only hides the panel; onboarding stays for the next open.
- Settings gains a “Start with Windows” toggler (same `SetAutostart`) and a “Show setup again” ghost button → `ShowSetup`: `settings = false`, `config.onboarded = false`, `onboarding = Some(new())`, `autostart = platform::autostart()`, persist, start `Detect`.

- [ ] **Step 1: Failing tests in `ui.rs` tests** (use `Loaded(Ok((Config::default(), vec![])))` on `ready()`):
  - `first_launch_shows_onboarding`: after Loaded, `app.onboarding.is_some()` and `app.is_open()`.
  - `onboarded_config_skips_setup`: Loaded with `onboarded: true` → `onboarding.is_none()`.
  - `finishing_sets_onboarded_and_peeks`: Loaded, 4× `OnboardNext`, then `OnboardNext` → `onboarding.is_none()`, `config.onboarded`, `presence.state() == State::Peek`.
  - `skip_finishes_with_current_settings`: Loaded, `OnboardSkip` → same assertions; `config.model == "qwen3:4b"`.
  - `invalid_draft_keeps_previous_config_on_finish`: Loaded, `Endpoint("http://example.com")`, `OnboardSkip` → `config.endpoint == "http://127.0.0.1:11434"`, `config.onboarded`.
  - `detection_picks_installed_model`: Loaded, `Detected(Ok(vec!["llama3.2".into()]))` → `detection == Found(..)`, `draft_config.model == "llama3.2"`; `Detected(Ok(vec![]))` → `NotFound`.
  - `late_detection_after_finish_is_ignored`: Loaded, `OnboardSkip`, `Detected(Ok(vec!["x".into()]))` → `config.model` unchanged, `onboarding.is_none()`.
  - `show_setup_again_reopens_step_one`: onboarded app, `ShowSetup` → `onboarding.unwrap().step() == Step::Welcome`, `!config.onboarded`.
- [ ] **Step 2:** `cargo test --bin bloop ui` → fails.
- [ ] **Step 3:** Implement update arms, then the view.
- [ ] **Step 4:** `cargo test` and `cargo clippy --all-targets` clean; run `cargo run` with `BLOOP_DATA_DIR` pointed at a fresh temp dir and click through all five steps.
- [ ] **Step 5:** Commit `feat: first-launch onboarding in the notch`.

### Task 5: App icon and installer art

**Files:**
- Create: `installer/make-art.ps1`, outputs committed: `installer/art/bloop.ico` (16/32/48/256 px), `installer/art/wizard.bmp` (328×628), `installer/art/wizard-small.bmp` (110×110)
- Create: `installer/bloop.rc` (`1 ICON "art/bloop.ico"`), `build.rs`
- Modify: `Cargo.toml` (`[target.'cfg(windows)'.build-dependencies] embed-resource = "3"`)

**Interfaces:**
- Produces files the installer script (Task 6) references by those exact paths.
- `build.rs`: on Windows target, `embed_resource::compile("installer/bloop.rc", embed_resource::NONE).manifest_optional().unwrap()`; `cargo:rerun-if-changed` for the rc and ico.

Art: black-and-white, matching the in-app mascot (black rounded blob, two white oval eyes, small smile) — ICO on transparent background; wizard image black field with the mascot large and centered low, "bloop" in white; small image the mascot on white. Script is deterministic (no randomness, fixed sizes) and writes ICO by packing PNG frames manually (System.Drawing's `Icon.Save` is lossy).

- [ ] **Step 1:** Write script; run `powershell -ExecutionPolicy Bypass -File installer/make-art.ps1`; open the outputs and look at them.
- [ ] **Step 2:** `cargo build --release`; check `(Get-Item target/release/bloop.exe)` shows the icon in Explorer (or extract with `[System.Drawing.Icon]::ExtractAssociatedIcon`) — non-default icon.
- [ ] **Step 3:** Commit `feat: mascot app icon and installer art`.

### Task 6: Inno Setup script and build script

**Files:**
- Create: `installer/bloop.iss`, `installer/build.ps1`
- Modify: `.gitignore` (`/dist/`), `README.md` (install + build-installer section)

**Interfaces:**
- `build.ps1`: reads `version = "x.y.z"` from `Cargo.toml` `[package]`, runs `cargo build --release`, locates `ISCC.exe` (PATH, `${env:ProgramFiles(x86)}\Inno Setup 6`, `$env:LOCALAPPDATA\Programs\Inno Setup 6`; error with the winget hint if absent), runs `iscc /DAppVersion=<v> installer\bloop.iss`, exits non-zero on any failure.
- `bloop.iss` essentials: `AppId` fixed GUID; `AppName=bloop`; `DefaultDirName={localappdata}\Programs\bloop`; `PrivilegesRequired=lowest`; `DisableProgramGroupPage=yes`; `OutputDir=..\dist`; `OutputBaseFilename=bloop-setup-{#AppVersion}`; `WizardStyle=modern`; `WizardImageFile`/`WizardSmallImageFile`/`SetupIconFile` from `art\`; `UninstallDisplayIcon={app}\bloop.exe`; `AppMutex=bloop-app`; `CloseApplications=yes`.
  - `[Tasks]`: `desktopicon` (unchecked), `autostart` "Start bloop when I sign in" (checked).
  - `[Icons]`: `{userprograms}\bloop`, `{userdesktop}\bloop` (task desktopicon).
  - `[Registry]`: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, `ValueName: bloop`, `ValueData: """{app}\bloop.exe"""`, `Flags: uninsdeletevalue`, `Tasks: autostart`. Plus `[UninstallRun]` none; uninstall deletes the Run value even if the app set it: `[Code]` `CurUninstallStepChanged(usUninstall)` → `RegDeleteValue(HKCU, RUN_KEY, 'bloop')`.
  - `[Run]`: `{app}\bloop.exe`, "Launch bloop", `Flags: nowait postinstall skipifsilent`.
  - `[Code]` `usPostUninstall`: `MsgBox('Also delete your bloop chats and settings?', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES` → `DelTree(ExpandConstant('{localappdata}\bloop'), True, True, True)`; skipped when `UninstallSilent`.

- [ ] **Step 1:** Install Inno Setup for the developer: `winget install --id JRSoftware.InnoSetup -e`.
- [ ] **Step 2:** Write both scripts; run `powershell -ExecutionPolicy Bypass -File installer/build.ps1` → `dist/bloop-setup-0.1.0.exe` exists.
- [ ] **Step 3:** Silent smoke test: `dist\bloop-setup-0.1.0.exe /VERYSILENT /TASKS=autostart` → `%LOCALAPPDATA%\Programs\bloop\bloop.exe` exists, Run value equals quoted path, Start Menu shortcut exists; then `"%LOCALAPPDATA%\Programs\bloop\unins000.exe" /VERYSILENT` → files, shortcut and Run value gone, `%LOCALAPPDATA%\bloop` untouched.
- [ ] **Step 4:** Commit `feat: per-user Inno Setup installer`.

### Task 7: Verification and docs

- [ ] `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` all clean.
- [ ] Update `docs/verification.md` with the automated results and a manual checklist (from the spec's Testing section): full wizard install, onboarding with Ollama running and stopped, sign out/in for autostart, uninstall answering Yes and No.
- [ ] Commit `docs: verify installer and onboarding`.
