# Installer and onboarding — design

Date: 2026-11-06 · Status: awaiting review · Sub-project 2 of 3 (notch ✓ → installer/onboarding → widgets)

## Goal

Anyone can install bloop from a single `setup.exe`, and the first launch walks them
through a short, good-looking setup. After install and setup, bloop is a sliver at the
top of the screen and connected to a model.

Success looks like:

- Double-clicking `bloop-setup-<version>.exe` installs bloop without an admin prompt,
  adds it to the Start Menu, optionally starts it with Windows, and launches it.
- The first launch opens the onboarding inside the notch panel; finishing it never
  shows it again (unless asked for from Settings).
- Uninstalling removes the app, its Start Menu entry and its start-with-Windows entry,
  and asks before deleting chat history and settings.

Out of scope: code signing (Windows SmartScreen will say "unknown publisher" on other
people's PCs until a certificate is bought), auto-update, MSIX, reading other apps'
notifications (needs signing; decided with the widgets), multi-language.

## Order decision

Notifications need package identity, which needs a trusted code-signing certificate.
To avoid blocking on that, this sub-project ships a classic installer, and the widgets
that don't need identity (timers, music, weather) follow. Notifications come last, once
signing is decided.

## Installer

- **Tool:** Inno Setup 6 (`iscc`), installed for the developer with
  `winget install JRSoftware.InnoSetup`. Not needed by people installing bloop.
- **Script:** `installer/bloop.iss`; build with `installer/build.ps1`, which runs
  `cargo build --release` then `iscc`, producing `dist/bloop-setup-<version>.exe`
  (version read from `Cargo.toml`). `dist/` is git-ignored.
- **Install:** per-user (`PrivilegesRequired=lowest`) into
  `%LOCALAPPDATA%\Programs\bloop`; Start Menu shortcut; desktop shortcut offered but
  unchecked; "Start bloop when I sign in" task, checked by default; "Launch bloop" on
  the finish page, checked.
- **Running copy:** bloop creates a named mutex `Local\bloop-app` at startup; the
  installer's `AppMutex` uses it to ask the user to close bloop before install/uninstall.
- **Uninstall:** removes files, shortcuts and the Run entry; then asks
  "Also delete your bloop chats and settings?" (default No) and deletes
  `%LOCALAPPDATA%\bloop` only on Yes.
- **Look:** Inno's modern wizard style with black-and-white art: a large wizard image
  and a small header image featuring the mascot, plus the app icon. Art is generated
  by `installer/make-art.ps1` (System.Drawing, reproducible, committed as PNG/ICO).
- **App icon:** the same mascot icon is embedded in `bloop.exe` (via a `build.rs` with
  the `embed-resource` crate) so Explorer, the Start Menu and Task Manager show it.

## Start with Windows

One source of truth: the value `bloop` under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, holding the quoted exe path.

- The installer task writes it; the uninstaller deletes it.
- `platform::autostart() -> bool` and `platform::set_autostart(bool) -> Result<(), String>`
  read/write it from the app. Onboarding and a new Settings toggle use them.

## Onboarding

Shown in the Open panel on launch when `config.onboarded` is false (new config field,
default false, saved like other settings). Five steps, one screen each, with step dots,
Back / Next, and "Skip setup" (which finishes with current settings):

1. **Welcome** — the mascot large and happy; "Hi, I'm bloop." and one line on what it is.
2. **Your model** — auto-detects Ollama at `http://127.0.0.1:11434` by listing its models
   (`GET /api/tags`). Found: pick a model from a dropdown (current default preselected if
   installed). Not found: a short note ("Start Ollama, or use an OpenAI-compatible
   endpoint") with Retry and the existing provider/endpoint/model fields. Next stays
   enabled; nothing here blocks finishing.
3. **How it works** — three small drawings (sliver → pill → panel) with captions:
   "Rest your mouse on the sliver", "Click to chat", "Ctrl+Alt+B from anywhere"; the
   shortcut field is editable here.
4. **Make it yours** — Light / Dark choice with live previews; "Start with Windows"
   toggle reflecting `platform::autostart()`.
5. **All set** — "Hover the top of your screen anytime." Finish saves settings with
   `onboarded = true`, closes the panel to the sliver and peeks "I'm up here!".

Settings gains "Start with Windows" and "Show setup again" (sets `onboarded = false`
and opens step 1).

## Architecture

- `src/onboarding.rs` (new, pure): `Step` enum, `Onboarding { step }` with `next()`,
  `back()`, `is_last()`, and the model-detection result type. No Iced/Win32.
- `provider::list_models(config: &Config) -> Result<Vec<String>, ProviderError>`:
  Ollama `GET /api/tags` → `models[].name`; OpenAI-compatible `GET {base}/models` →
  `data[].id`. Same endpoint validation, timeouts and size limits as `chat`.
- `src/ui.rs`: onboarding view and messages; first-launch check after `Loaded`.
- `src/platform.rs`: autostart read/write and the single-instance mutex.
- `src/mascot.rs`: reuse the mascot; small sliver/pill/panel illustrations for step 3.

## Error handling

- Model detection failure or timeout (3 s) → "not found" state; never blocks.
- Autostart write failure → inline error text under the toggle; setting not changed.
- Config save failure during Finish → existing storage warning; onboarding still closes
  and will show again next launch (because `onboarded` was not saved).

## Testing

- Unit: onboarding step navigation; `list_models` against the existing local fixture
  server for Ollama and OpenAI shapes plus an error case; config round-trip of
  `onboarded` (old configs without the field load as `false`); autostart read/write
  against a test registry key (not the real Run key); first-launch shows onboarding,
  finishing sets `onboarded` and collapses.
- Manual on Windows: build the installer; install; walk through onboarding with Ollama
  running and stopped; reboot or sign out/in to confirm start-with-Windows; uninstall
  with both answers to the data question.
