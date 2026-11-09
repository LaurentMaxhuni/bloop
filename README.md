# bloop

Landing page: <https://laurentmaxhuni.github.io/bloop/> (source in `site/`, published by
`.github/workflows/pages.yml`; preview locally with `python -m http.server -d site`).

A native, local-first AI chat notch for Windows. Rust + Iced, without a browser
runtime or bundled inference engine. First runnable milestone, not yet an autonomous
desktop agent.

## Run

Requires Windows 10/11, a DirectX 12 graphics driver, Rust stable, and the Visual
Studio C++ Build Tools with Windows SDK. Verified with Rust 1.97.1 on Windows.

```powershell
cargo build --release
.\target\release\bloop.exe
```

At rest bloop is a thin sliver at the top center of the screen. Rest the mouse on
it to pop out the pill (it shrinks back when you move away), click the pill or press
**Ctrl+Alt+B** to open the panel, and click anywhere else to close it. bloop also
peeks out on its own for a few seconds when it has something to say, and hides
completely while a fullscreen app, presentation, or the lock screen is up. Escape
stops an active reply or collapses the notch.

bloop also has a normal app window (900 × 620, resizable) for setup and settings. The
first time you run it, the window walks you through setup and the notch appears when
you finish; if you close it midway, setup returns next launch. After that, starting
bloop from the Start menu or the exe opens the notch and the window. Pages: Home,
Appearance, Model, Shortcut & startup, and About, where you choose the provider, base
URL, model, and global shortcut (a conflicting shortcut produces a visible warning;
choose another). The notch's settings icon and the tray's **Settings** also open the
window. Closing the window leaves bloop running; use the tray's **Quit**. Starting
bloop again while it is running just brings the window forward. Login autostart and
self-updates start bloop with `--background`, which opens the notch only.

Start your model server separately. No model is downloaded or started by bloop.

| Provider | Endpoint base | Model |
| --- | --- | --- |
| Ollama | `http://127.0.0.1:11434` | Installed model name; default `qwen3:4b` |
| llama.cpp server | `http://127.0.0.1:8080/v1` | Server model name/alias |
| OpenAI-compatible service | Your HTTPS base ending in `/v1` | Supported chat model |

For authenticated endpoints, set `BLOOP_API_KEY` in the environment before launching.
Keys are never stored in the database. The client supports the streaming
`chat/completions` contract; Responses-only models and provider-specific parameters
are outside this milestone.

## Implemented

- Auto-hiding notch: a 120 × 5 px sliver at rest, a pill on hover (150 ms) or
  peek (4 s), the full panel on click/shortcut; hidden during fullscreen apps.
- App window: a decorated 900 × 620 dashboard (Home, Appearance, Model, Shortcut &
  startup, About) that also hosts first-run setup. It opens on manual launches,
  second launches, the notch settings icon and tray Settings; closing it never quits.
- Always-on-top notch flush with the top of the primary work area, clipped to its
  real silhouette (concave shoulders, rounded bottom) with a native window region.
- Black-and-white shadcn/ui "neutral" styling in light and dark themes.
- Bloop, a canvas-drawn mascot that blinks, follows the pointer, thinks while waiting,
  talks while replying, cheers when done, winces on errors, and naps when left alone.
- Interruptible 220 ms expand/collapse animation. At rest the only timer is a
  blink every ~3.7 s; 30 fps frames run only while the mascot is moving.
- Native Windows tray, configurable global shortcut, taskbar suppression, logical
  DPI-aware sizing.
- Streaming Ollama NDJSON and OpenAI-compatible SSE, Stop/Escape cancellation,
  connection/read/overall deadlines, bounded output, and visible failures.
- SQLite settings and the last 50 conversation turns. Interrupted replies remain
  visible but are omitted from future model context. Clear chat asks for confirmation.
- Serialized background saves. Quit waits for the latest save; failures keep the
  app open and expose an explicit quit-without-saving action.

Settings/history live at `%LOCALAPPDATA%\bloop\bloop.sqlite3`. `BLOOP_DATA_DIR` overrides
the data directory for isolated testing. Conversation history is local plaintext;
protect it through your Windows account and disk security. Run one bloop instance
per data directory.

Only explicitly configured endpoints receive chat content. Remote connections
require HTTPS; plaintext HTTP is restricted to loopback. Redirects and system
proxies are disabled. Inputs are limited to 8 KiB, output to 128 KiB; context retains
up to 12 complete recent pairs within a 32 KiB history/prompt budget.

## Validation

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Transport tests use real loopback HTTP connections, including fragmented UTF-8,
SSE, failures, truncated streams, and cancellation disconnects. UI tests cover stale
chunks, interruptible animation, clear confirmation, and failed-save shutdown.

For deterministic manual streaming verification without a model:

```powershell
$env:BLOOP_DATA_DIR = Join-Path $PWD 'local-test-data'
cargo run --example stream_fixture
# In another terminal, set the same BLOOP_DATA_DIR, then launch bloop.
```

The fixture binds `127.0.0.1:11634`, labels its responses as fixtures, and requires
an isolated data directory. Its shortcut is Ctrl+Alt+Shift+B. It never performs
inference. Stop it after testing.

## Limits and next milestone

The software-renderer prototype used about 34 MiB but failed transparent cursor
redraws on Windows. The working GPU renderer uses more memory; see
[verification notes](docs/verification.md). The provisional sub-50 MB target is
**not achieved**. Frame cadence, startup latency, and multiple mixed-DPI monitors
have not been benchmarked. The notch currently stays on the primary monitor.

Windows UI smoke tests cover rendering, both themes, shortcut expansion, streaming,
Stop, saved history restoration, settings, and quit. Live model output remains
unverified because no inference server was running. Linux integration is planned;
screen-reader UI Automation support is also incomplete in this Iced milestone.

Phase 3 remains: typed tool registry, code-owned permissions and confirmation UI,
visible tool logs, bounded cancellable execution loop, and basic OS tools. There is
currently **no shell, file, application, clipboard, or window-control access** for
the model. The system prompt describes that limitation.

Architecture and tradeoffs: [decision](docs/architecture.md).
Implementation checklist: [plan](docs/superpowers/plans/2026-11-06-native-island.md).

Licensed under [MIT](LICENSE).

## Installing

Download `bloop-setup-<version>.exe` and run it. It installs for your user only (no admin
prompt) into `%LOCALAPPDATA%\Programs\bloop`, adds a Start Menu entry, and can start
bloop when you sign in. The installer isn't code-signed yet, so Windows SmartScreen
shows "unknown publisher": choose **More info → Run anyway**. Uninstall from Windows
Settings › Apps; it asks before deleting your chats and settings.

## Building the installer

Needs Inno Setup 6 (`winget install JRSoftware.InnoSetup`), then:

    powershell -ExecutionPolicy Bypass -File installer\build.ps1

This builds the release exe and writes `dist\bloop-setup-<version>.exe` (version from
`Cargo.toml`). `installer\make-art.ps1` redraws the icon and wizard art.

## Publishing an update

Bump `version` in `Cargo.toml`, commit, then (with the GitHub CLI signed in once via
`gh auth login`):

    powershell -ExecutionPolicy Bypass -File installer\release.ps1 -Notes "What changed"

This builds the installer, writes its SHA-256 next to it and creates the GitHub release
`v<version>` with both files. Running copies of bloop find it within six hours (or straight
away with the app window's **About › Check now**), download it, check the hash and offer
"Update ready — click to restart".
