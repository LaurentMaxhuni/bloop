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

## Chats, model profiles and clipboard ask (2026-11-12)

- **Chats replace the single history.** A `chats` table holds one row per conversation
  (title, profile, destination, last change, turns as JSON). On first load the old
  `state.history` becomes one chat in a single transaction and is emptied, so the
  migration runs once. The old schema never recorded where that history was sent (the
  settings may have changed since), so the imported chat is marked `unknown` and bloop
  won't send it anywhere; it can be read, exported or deleted. A chat without turns is
  never written, so "New chat" costs nothing. Chat ids are creation times in
  milliseconds, assigned in the UI, so a save never needs a round trip to learn its id.
- **One storage queue.** Saves (settings plus the chat in the notch), renames, deletes and
  loads of whole chats run one at a time, in order, each with a snapshot taken when it was
  queued (`ui/jobs.rs`). A newer save replaces queued saves of the same chat; a delete
  drops that chat's queued saves and renames and runs after the one in flight. So
  switching chats can't lose the outgoing chat's last reply, an old save can't bring a
  deleted chat back, and opening a chat reads its newest turns. Quit and Restart to update
  wait for the queue to empty. Search reads outside the queue; it may miss a reply still
  being saved. (Found by Codex reviewing the first version, which tracked one save.)
- **A chat keeps its destination.** Each chat records where its first message went:
  `local` (a loopback endpoint) or `cloud:<provider or origin>`. bloop refuses to send a
  chat's history anywhere else, including Retry, which would resend an old prompt. Changing
  the default model or a profile's endpoint therefore can't move local context to a cloud
  service; the user starts a new chat instead. Switching profile in the notch does exactly
  that when the chat already has turns.
- **Profiles** are extra named provider/endpoint/model sets in the config (at most 8); the
  existing provider, endpoint and model stay the default profile, so older configs load
  unchanged. Sending swaps the chat's profile into a copy of the config, so the provider
  code didn't change.
- **Default profile (2026-11-12).** `default_profile` names the profile new chats start
  on (0, the built-in one, unless the user stars another). The built-in profile is shown
  as "Main" rather than "Default", because the default can now be any profile. The star
  applies and saves at once, like Appearance, and never touches unsaved Model page edits;
  Save keeps the stored value. Deleting the starred profile hands the star back to Main,
  and a saved file naming a missing profile loads with Main as the default. New chat now
  starts on the default instead of the current chat's profile, so the star means one
  thing everywhere. Deleting a profile asks first, as deleting a chat does.
- **Icons over long labels (2026-11-12).** Frequent actions (send, stop, copy, retry,
  edit, new chat, chat list, profile and chat row actions, page links on Home) are Lucide
  icons with a tooltip; they show their word until the icon font loads, as the header
  already did. Confirmations (Keep/Delete), Save, sign-in and setup keep text, where a
  wrong guess costs more. The font subset grew to 27 glyphs (10 KB); a test reads the
  font's cmap so a code point missing from the subset fails the build instead of drawing
  a box.
- **API keys follow the endpoint.** Saved keys are keyed by endpoint origin in Credential
  Manager (`bloop/api-key/<origin>`), so a profile for one server never sends the key
  saved for another. The old single key moves to the endpoint configured at first start.
  `BLOOP_API_KEY` still applies to every endpoint, as before. The single key an older
  bloop saved isn't moved automatically, because the endpoint may have changed since it
  was saved; the Model page offers to tie it to the endpoint shown, and it's never sent
  until the user does. A key typed into the field is dropped when the provider, profile or
  server changes, so it can't be saved for a server it wasn't pasted for.
- **Clipboard ask** is a second global hotkey (default Ctrl+Alt+Shift+A, empty turns it
  off). It reads clipboard text only when pressed, attaches up to 6 KiB below the
  question in a code fence longer than any backtick run inside it, and shows the text and
  the destination before anything is sent. The attachment belongs to the chat it was made
  in: New chat, opening another chat or a late clipboard read never carries it over. No image capture yet: many supported models
  can't read images, so a screen-region feature should go through local OCR first.
- **Markdown export** writes a chat through the standard Windows Save dialog; nothing is
  written without the user picking a file.

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

## Markdown export helpers (2026-11-12)

export::markdown preserves turn text and marks interrupted replies before their content,
so an unfinished code fence cannot hide the marker. Suggested names remove Windows-invalid
characters and device names, trim dots/whitespace, and cap the complete name at 120 UTF-16
code units while preserving Unicode scalar boundaries. The library does no file I/O.
platform::save_file_dialog uses GetSaveFileNameW with call-local UTF-16 buffers, no owner
HWND or callbacks, a Markdown filter/default extension, overwrite confirmation and
OFN_NOCHANGEDIR. It runs on the caller's blocking worker and returns a path or cancellation;
the caller writes the file. The native call has its own modal message loop, so it does not
block Iced. No COM initialization or application window handle crosses threads.
API reference: https://learn.microsoft.com/windows/win32/api/commdlg/nf-commdlg-getsavefilenamew

## Custom instructions (2026-11-12)

Custom instructions are one global setting, edited on the Model page and saved with
the existing settings flow. Outer whitespace is trimmed; the text is capped at 2048
UTF-8 bytes to keep this per-message overhead small. Old configs default to empty.
The context builder appends a labelled `<instructions>` block after the unchanged
fixed system message, explicitly describing it as preferences, not permission to use
tools. Keeping it in the system message gives every provider the same context:
ChatGPT maps it to developer context, and CLIs retain their existing flattening of
the system message into a user-labelled turn. The entire system message, including
the block and its delimiters, counts against the 32 KiB context budget; old whole
pairs are dropped first, and the current prompt is always retained.
Save jobs box their config snapshot to keep the queue's enum small as settings grow.

## Mascot outfits (2026-11-12)

Bloop's hat, eyewear, outfit and body colour are a `Look` saved with the settings
(`config.rs`, so the library still has no GUI code; unknown or missing parts load as the
default). `mascot.rs` draws them on the same 24-unit grid as the body. A hat would poke out
of the canvas, so each hat adds headroom (4–8 units) above the grid and Bloop is drawn a
little smaller in the same square; the antenna is hidden under any hat. A tinted coat uses a
fixed dark face so it reads in both themes, and accessory colours are fixed for the same
reason. The outfit is drawn under the face so a laughing mouth stays visible, the cape before
the body so it hangs behind. To make Bloop more present, the open notch's header shows it at
36 px (was 30), an empty chat greets you with a 72 px Bloop, and clicking it makes it laugh.
Appearance changes apply and save at once, like the rest of that page.

