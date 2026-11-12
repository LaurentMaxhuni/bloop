# Native island MVP

Decision (2026-11-06): Iced 0.14, DirectX 12 GPU rendering, one Cargo package.
Modules separate UI, provider transport, platform integration, and SQLite storage.
No browser runtime, inference process, or continuously running animation timer.
Split crates only when independent consumers justify public interfaces.

Iced exposes transparency, undecorated windows, logical DPI-aware sizes, window
levels, positioning, and subscriptions. The first software-rendered build used
34 MiB working set but lost unchanged content during transparent cursor redraws.
Softbuffer's GDI surface does not reliably support the required alpha composition.
The GPU build fixes that observed failure; selecting DirectX 12 alone reduces
unused backend loading. Slint offers declarative animation
and software rendering but adds a DSL and licensing choices. egui favors immediate
mode; native Win32 would require substantially more layout, text, and input code.

Sources verified before implementation:
- https://docs.rs/iced/0.14.0/iced/window/settings/struct.Settings.html
- https://docs.rs/iced/0.14.0/iced/window/enum.Position.html
- https://docs.rs/crate/iced/0.14.0/features
- https://slint.dev/get-started
- https://docs.typesafe.ai/llms.txt
- https://github.com/rust-windowing/softbuffer/issues/17
- https://docs.ollama.com/api/chat
- https://docs.ollama.com/api/streaming
- https://developers.openai.com/api/reference/resources/chat

Windows risks: transparent software surfaces, taskbar suppression, per-monitor DPI,
focus activation restrictions, hotkey collisions, and tray message-pump ownership.
Keep Win32 calls inside the platform module. A dedicated blocking Windows message
loop owns the tray and global shortcut; UI receives events without polling.

Provider boundary: Ollama NDJSON and OpenAI-compatible SSE (including llama.cpp and
Unsloth Studio). Bound frame/output/context sizes, distinguish failures from successful
completion, enforce connect/read/overall deadlines, and cancel by aborting the Iced task.
Remote endpoints are explicit configuration; plaintext HTTP restricted to loopback.
API keys come from BLOOP_API_KEY or Windows Credential Manager (`bloop/api-key`,
`secrets.rs`) and are never written to SQLite or logs.

ChatGPT plan usage uses a native OAuth client in `chatgpt.rs` with browser sign-in, a
temporary loopback callback, PKCE and signed ID token verification. The installation ID,
account registration and rotating session are stored together in a Windows DPAPI-protected
SQLite record. Model discovery and refresh happen during user actions. The ChatGPT provider
uses OpenAI's Responses API with `store: false`, streams into the existing chat transport,
and requires a completed event. The GUI receives account status and model names.
[OpenAI Docs](https://developers.openai.com/siwc/token-sharing-open-source)

CLI providers (2026-11-07): Claude Code and Codex run as one subprocess per message
(`cli.rs`) in chat-only mode. bloop never enables their tools: Claude gets `--tools ""`
and no MCP or settings; Codex runs read-only without the user's config, plugins, apps or
memories, and any tool event from either aborts the reply. They run in an empty
`<data dir>/cli` folder, read the prompt from stdin, and live in a Windows job object so
cancelling or quitting kills the whole process tree. Win32 calls for this stay in
`cli.rs` and `secrets.rs`.

Codex streaming (2026-11-11): `codex exec --json` only emits whole messages, so Codex now
runs as `codex app-server` (JSON-RPC over stdio), which streams `item/agentMessage/delta`.
The app server cannot skip the user's config, so bloop disables every tool feature on the
command line, reads the effective config over the protocol and disables each MCP server by
name in `thread/start`, uses a read-only ephemeral thread, picks Codex's default model via
`model/list` when none is set, drops `commentary`-phase messages, and still refuses any tool
item or server request. A CLI reply holds three pipe operations on tokio's blocking pool at
once, so the runtime allows `cli::BLOCKING_THREADS` (4) blocking threads.

Rich replies (2026-11-11): replies render with iced's `markdown` widget (`highlighter` feature:
syntect, pure Rust, about +2.9 MiB release size). `math.rs` rewrites inline TeX to Unicode
and display TeX (`$$…$$`, `\[…\]`) to ```` ```math ```` fences; `ui/rich.rs` lays those out
natively (fractions, scripts, roots, big operators, matrices) in Cambria Math, adds a Copy
button to code blocks, and opens only http(s)/mailto links. Each turn keeps a parsed
`Reply` that is extended incrementally while it streams.

Notch motion (2026-11-07): the OS window is created once at the open panel's size and is
never resized while animating; only the drawn notch changes size, driven by critically
damped springs (`motion.rs`) stepped on `window::frames()` (redraw-synced, no subscription
when settled). The window region is re-cut to the drawn outline whenever it moves a whole
pixel, so everything outside the notch stays click-through. Width, position, feel and reduce
motion come from `Config`; reduce motion follows Windows' "Animation effects" by default.
Header and sidebar icons come from a 4.5 KB subset of the Lucide icon font (ISC,
`assets/LUCIDE-LICENSE.txt`); word labels show if it fails to load.

Self-update (2026-11-07): `update.rs` asks the public GitHub Releases API for
`LaurentMaxhuni/bloop` 30 s after start and every 6 h (switchable in the app window), downloads
`bloop-setup-X.Y.Z.exe` into `<data dir>/updates` while hashing it, and keeps it only if its
SHA-256 matches the release's `.sha256` asset. Redirects may only go to GitHub's HTTPS hosts.
Installing saves state, starts the installer with `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART
/RELAUNCH=1` and quits; Inno waits for the `bloop-app` mutex and reopens bloop. The checksum
catches broken downloads, not a compromised GitHub account; the installer is not code-signed.
Releases are published with `installer/release.ps1` (GitHub CLI).

Jev considered for future intent/tool selection. Do not add a cloud dependency to
the local chat MVP. Future model proposals must pass a typed registry and code-owned
permissions; model confidence never grants permission. No shell/tool execution in
this milestone.

Performance targets are provisional. Windows overlay is the supported milestone;
Linux tray, compositor placement, and shortcuts require separate verification.
The first measured GPU baseline exceeds the provisional 50 MB target. A future
software compositor must preserve alpha correctly before replacing DirectX.

App window (2026-11-09): bloop is now an `iced::daemon` with two windows: the notch (always
present) and a decorated app window (900 × 620, min 720 × 520) created on demand and closed
for real, so it costs nothing while hidden. It replaces the in-notch hub and hosts onboarding.
Window ids route cursor, focus and drawing to the right window; closing the app window never
quits, Quit lives in the tray. `launch.rs` decides what to open: first run shows only the
window (finishing or closing it brings up the notch; unfinished setup resumes next launch);
a manual launch opens notch and window; `--background` (the HKCU Run value and the update
relaunch) opens the notch only. A second launch signals the named event `Local\bloop-open`
and exits; the running copy opens or focuses the window. `bloop-app` stays the installer mutex.

Release builds and signing (2026-11-11): releases are built by `.github/workflows/release.yml` on
GitHub's Windows runners from a pushed `v<version>` tag (`installer/release.ps1` creates it), not
on a personal PC, because SignPath Foundation (free certificates for open-source projects) signs
only automated builds of the public repository. The workflow signs `bloop.exe` before Inno Setup
packages it, then signs the installer; both steps are skipped until the SignPath secret and
variable exist, so unsigned releases keep working meanwhile. `build.rs` generates the version
resource from `Cargo.toml` for SignPath's product-name check. Alternatives: a purchased OV
certificate (needs a hardware token or cloud HSM, a yearly fee, and would keep local builds) and
Microsoft Artifact Signing (monthly fee; individual identities limited to a few countries).
Neither signs Inno's generated uninstaller without a local signing tool. Details:
`docs/code-signing.md`.

## Paced reveal and reply moods (2026-11-12)

Streaming worked end to end, but it didn't look like it. Measured against the real Codex CLI, a
short reply spent about 5 s thinking and then arrived in bursts over about 1.5 s, several
tokens per read, so the text seemed to appear all at once. The fix is in the UI, not the
providers: `Island::reveal` holds how many bytes of the newest reply are on screen. Each
mascot tick (33 ms) shows at least 3 more characters, or a sixth of the backlog when that is
larger. That is about 90 characters a second, and a large backlog drains within about a
second, so the reveal never lags far behind the model. The first characters show as soon as
they arrive. Stored history and model context always use the full text. With reduced motion
the reveal is off.

The reaction (happy, laughing or mad) waits until the reveal catches up. `Mood::for_reply`
picks it with a cheap word check: laughter ("haha", "lol", 😂) in the reply, or a joke
request in the prompt, means laughing, and a failed or stopped reply means mad. The chat
draws a small Bloop beside each reply. Only the newest one moves, and only on the ticks the
notch mascot already runs, so this adds no new timers. Older replies show Bloop asleep, as
does the newest after 45 s of quiet. The two new moods are Laughing (squeezed eyes, open
mouth, a bounce that fades) and Mad (slanted brows, frown, a brief tremble, an anger mark).

## Monitor placement (2026-11-12)

The notch uses physical HWND placement on its owning Iced event-loop thread. Shortcut
opening from the sliver targets the foreground application's monitor, ignoring our own
process, then the cursor, then primary. Other placements retain the current monitor.
The work-area origin stays physical; only canvas dimensions and edge inset are scaled.
Two synchronous SetWindowPos calls preserve activation and Z order: the first lets
winit 0.30.13 handle WM_DPICHANGED and its suggested rectangle, and the second restores
our exact requested rectangle after that handler returns. There is no timer or polling.
Initial window settings retain the existing primary_position logical-coordinate helper.
See Microsoft's SetWindowPos documentation:
https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setwindowpos

## Connection diagnostics (2026-11-12)

The library's diagnose::test_connection returns a Report and has a ten-second overall
budget. HTTP providers reuse the existing bounded model-list GET (three seconds), without
sending chat content. ChatGPT first reads account/plan status and lists models only when
plan usage is enabled; at most one existing token-refresh request precedes that GET.
The refresh remains cancellation-safe and may finish persisting rotated credentials after
the diagnostic deadline. No new endpoint or background request is introduced.
CLI discovery uses cli::find on a blocking worker and does not start any program or claim
authentication was checked. Provider errors retain HTTP status and URL-stripped reqwest
causes so diagnostics can distinguish refused connections, TLS failures and timeouts.
