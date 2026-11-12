# MVP verification — 2026-11-06

Windows x64, Rust 1.97.1, Iced 0.14, native MSVC toolchain.

Passed on final source:
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`: 19 passed (10 library, 5 UI state, 4 real HTTP transport tests).
- `cargo build --release`

An independent source review found nullable tool fields being rejected and saves
that could exceed reload limits. Both received failing regression tests and fixes;
the complete suite then passed. Shutdown, animation, stale-request handling, and
saved-chat clearing are also covered by state tests.

Real Windows UI checks: compact and expanded transparency, stable cursor redraw,
default and configured global shortcuts, dark/light theme, settings layout, token
streaming from a labeled local fixture, Stop, history/theme restoration after
restart, and quit through settings and Alt+F4. Fixture used isolated local data;
no real conversation was sent to a cloud service. No local model server was running,
so actual inference quality and model-specific compatibility remain unverified.
The tray message loop initialized without errors; tray menu selection itself has
not been manually exercised.

Performance observations (single process, external inference excluded):

| Build / state | Working set | Private bytes | Observation |
| --- | --- | --- | --- |
| Initial tiny-skia | ~34 MiB | ~11 MiB | Transparent partial redraw failed; rejected |
| All GPU backends, compact | 187.13 MiB | 200.58 MiB | 93.75 ms CPU over 8.05 s |
| DirectX-only final release, compact | 100.52 MiB | 107.16 MiB | 0 ms CPU delta over 8.07 s |

Final release executable: 14.28 MiB. Final sample used normal default settings,
20 process threads (including graphics-driver threads), and no backend override.
Tokio is explicitly limited to two async workers and two blocking workers.
These short samples are observations, not guarantees or a full profiling study.
Working-set and private-memory metrics differ; neither includes dedicated GPU VRAM.
The sub-50 MB target is not achieved. Startup latency, 60 FPS cadence, GPU energy
use, and mixed-DPI/multi-monitor hotplug behavior have not been benchmarked.

Known scope limits: Windows/DirectX 12 only; primary-monitor anchoring; incomplete
screen-reader integration; one stored conversation and one instance per data
directory; no OS tools or autonomous agent loop. Phase 3 remains separate work.

Sandboxed MSVC archive writes and network access were blocked. Cargo dependency
fetches, native builds/tests, and interactive launches were run with approved
environment access. No changes to global Rust, Git, or Windows configuration.

## Auto-hide notch (2026-11-06)

Checked on Windows 11 (100 % scale) with a debug build and a scratch `BLOOP_DATA_DIR`,
using `WindowFromPoint` to see which process owns each screen point:

| Check | Result |
| --- | --- |
| At rest only the sliver belongs to bloop; points 70 px right of it and 20 px below it belong to the window underneath | Pass |
| Resting the cursor on the sliver for 0.7 s shows the pill with the mascot | Pass |
| Moving away shrinks back to the sliver within 1.2 s | Pass |
| Clicking the pill opens the full panel | Pass |
| Activating another window closes the panel | Pass |
| Settings › Peek test (debug only) shows "Hi! This is a peek." and returns to the sliver by 5 s | Pass |
| A borderless fullscreen top-most window hides the sliver within the 2 s poll; it returns after the window closes | Pass |

Not checked: a real fullscreen game or video, the lock screen, mixed-DPI monitors,
and a reply finishing while the panel is closed (covered by unit tests only).

## Unsloth Studio, Claude Code and Codex providers (2026-11-07)

Automated: `cargo test` — 35 library, 44 app, 7 CLI-process and 10 HTTP-transport tests
pass (plus 3 opt-in real-CLI checks: `cargo test --test cli -- --ignored`, all passing); `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean.
The CLI-process tests run a fake CLI (`examples/fake_cli.rs`) and prove the prompt
arrives on stdin, replies stream, the idle deadline fires, a signed-out CLI gets a hint,
and cancelling kills the CLI and its child, directly and through a `.cmd` shim.
After the final review: a refused tool event kills the CLI at once, Codex items are
allowlisted, re-running setup keeps a working configuration, and turn markers carry a
random per-request nonce.

Checked by hand on Windows 11 with a debug build and a scratch `BLOOP_DATA_DIR`
(Claude Code 2.1.316, Codex 0.159.3; Ollama and Unsloth Studio not running):

| Check | Result |
| --- | --- |
| Setup's model step lists Claude Code and Codex and preselects one | Pass |
| Claude Code replies from the notch ("pong") with the user's own sign-in | Pass |
| Codex replies from the notch ("pong"); badge reads "Codex" | Pass |
| No bloop-started `claude`/`codex` process remains after a reply | Pass |
| Force-quitting bloop mid-reply kills `cmd` → `node` → `codex.exe` | Pass |
| Cancelling a real Codex reply kills its process tree (`cargo test --test cli -- --ignored`) | Pass |
| Codex starts no MCP servers with bloop's flags (user has Firecrawl, Neon, Supabase configured) | Pass |

Not checked: Unsloth Studio against a running server (transport tests cover its
OpenAI-compatible stream, bearer key and 401 message), saving a key from the Settings UI
on a real Unsloth, and Escape cancelling a reply while the prompt has focus (it doesn't;
that predates this work — the Stop button cancels).

## Installer (2026-11-07)

`installer\build.ps1` builds `dist\bloop-setup-0.2.0.exe` (6.5 MB, Inno Setup 6.7.3).
Checked on Windows 11 by upgrading an existing 0.1.0 install that had chats in it, with
`/VERYSILENT /TASKS=""`:

| Check | Result |
| --- | --- |
| Setup exits 0 with no admin prompt; files land in `%LOCALAPPDATA%\Programs\bloop` | Pass |
| `bloop.exe` replaced and shows the mascot icon | Pass |
| Uninstall entry "bloop 0.2.0" written (it was missing from the 0.1.0 install) | Pass |
| Start Menu shortcut present; no Run value when the task is not selected | Pass |
| Chats and settings (`%LOCALAPPDATA%\bloop\bloop.sqlite3`) untouched by the upgrade | Pass |
| The single-instance mutex (`Local\bloop-app`) refuses a second copy | Pass (dev build) |

Not checked: uninstalling (it would remove the user's real install), the "Also delete your
bloop chats and settings?" prompt, the wizard UI pages, start-with-Windows after signing
out and in, and SmartScreen on another PC. Bitdefender quarantined the first build and the
build scripts; add the project and `%USERPROFILE%\.rustup` / `.cargo` as exceptions.

## Smooth motion, icons and customization hub (2026-11-07)

Automated: `cargo test` — 46 library, 58 app, 7 CLI-process and 10 HTTP-transport tests pass;
`cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are clean. Springs are
tested for settling time per preset at 60 and 144 Hz, no overshoot, momentum on reversal,
and clamped stalls; the app tests cover reversal mid-animation, reduce motion, no frames at
rest, notch placement, hub navigation, live appearance changes, hover delay and old configs.

Checked by hand on Windows 11 with a debug build, a scratch `BLOOP_DATA_DIR` and
`BLOOP_FRAME_LOG`, on a 360 Hz monitor (RTX 4070 Ti SUPER):

| Check | Result |
| --- | --- |
| Frames follow the display: median interval 2.76 ms (360 Hz) | Pass |
| 9 of 10 animations had no frame over 2.5 × the median (longest 4–5.6 ms); the first open, which also builds setup, had 10 frames up to 11.2 ms | Pass |
| Clicks beside the sliver, beside the pill, beside and below the open panel reach the window underneath; inside the notch they reach bloop | Pass |
| Header shows sun/moon, sliders and chevron icons; "Customize" tooltip on hover | Pass |
| Hub grid shows four tiles with icons and summaries | Pass |
| Look: Teal and Light apply instantly (moon icon, teal toggle) | Pass |
| Notch: L widens the panel to 600 px and Right moves it to the right edge (24 px inset), live | Pass |

Not checked: Reduce motion following the Windows "Animation effects" toggle (changing it is a
system setting), Follow Windows across a real theme switch, 60 Hz monitors, mixed-DPI or
multi-monitor setups, and moving the notch between monitors while running.

## Auto-update (2026-11-07)

Automated: `cargo test` — 52 library, 72 app, 7 CLI-process, 10 HTTP-transport and 9 update
tests pass (plus opt-in network checks: `cargo test --test update -- --ignored`); clippy
`-D warnings` and `fmt --check` are clean. Update tests run a local fake GitHub: newer/none,
rate limit and HTML replies, no releases yet (404), checksum mismatch, size overrun, refused
off-GitHub redirects, and leftover `.part` files.

A throwaway probe installer showed that a silent setup started while the app's mutex is held
exits with code 1 (nothing installed), and that `InitializeSetup` runs before Inno's AppMutex
check, so bloop's installer now waits there (up to 15 s) during self-updates.

End to end on Windows 11 with real releases on `LaurentMaxhuni/bloop`:

| Check | Result |
| --- | --- |
| `installer\release.ps1` builds, writes the `.sha256` and publishes v0.3.0 and v0.3.1 | Pass |
| The public API lists both assets; the checksum file matches the updater's format | Pass |
| Silent upgrade 0.2.0 → 0.3.0 with `/RELAUNCH=1` installs and reopens bloop | Pass |
| A running 0.3.0 finds v0.3.1 33 s after start, downloads 6.9 MB and verifies it | Pass |
| It peeks "Update 0.3.1 ready — click to restart"; one click updates and 0.3.1 is running 3 s later | Pass |
| Chats (8 turns), provider, setup state, shortcuts and start-with-Windows choice kept across both updates | Pass |

Not checked: the 6-hour repeat, "Check now" and the auto-update switch by hand (unit tests
only), a download interrupted by quitting, an installer blocked by antivirus at launch, and
renaming the repository. The ready message wraps to two lines in the pill.

## App window (2026-11-09)

Automated, run on the `feat/app-window` branch: `cargo fmt --check` clean; `cargo clippy
--all-targets -- -D warnings` clean; `cargo test` passes with 58 library, 89 app, 7 CLI-process
(3 ignored), 10 HTTP-transport and 9 update (1 ignored) tests; `cargo build --release` succeeds
(one dead-code warning, `Message::PeekTest` never constructed, present in the release build only).

Verified by reading `installer/bloop.iss` and `src/platform.rs`/`src/launch.rs` (installer not
built, build.ps1 not run):

| Check | Result |
| --- | --- |
| Installer autostart Run value is `"{app}\bloop.exe" --background` | Pass (source) |
| Installer `/RELAUNCH=1` entry runs `bloop.exe --background` | Pass (source) |
| In-app autostart value is `"<exe>" --background`; `--background` parses to the notch-only launch | Pass (unit tests) |

Pending human check: a copy of bloop installed on this machine was running and holding the
single-instance lock, so the new build was not launched.

| Check | Result |
| --- | --- |
| Fresh `BLOOP_DATA_DIR` shows onboarding in the app window, no notch; finishing opens notch and Home | Pending human check |
| Closing the window mid-setup opens the notch; setup returns next launch | Pending human check |
| Manual launch after setup shows notch and Home | Pending human check |
| `--background` shows the notch only | Pending human check |
| Second launch with the window closed, open, and minimized shows and focuses it | Pending human check |
| Tray Settings and the notch settings icon open the window | Pending human check |
| Closing the window leaves bloop running; tray Quit exits | Pending human check |
| Installer built with `installer\build.ps1` writes the Run value with `--background` | Pending human check |
| Self-update relaunch starts bloop with `--background` end to end | Pending human check |

## Codex streaming, rich replies (2026-11-11)

Automated: `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean;
`cargo test` passes 69 library, 97 app, 8 CLI-process (3 opt-in ignored), 10 HTTP-transport and
9 update tests. New tests cover the Codex app-server handshake (MCP servers disabled by name,
default model from `model/list`, one `turn/start`), commentary filtering, refused server
requests, failed-turn messages, TeX parsing and Unicode conversion, `$5 and $10` and code staying
literal, incremental Markdown parsing while streaming, and a prompt reaching the CLI on a runtime
capped like the app's. That last test failed with the old cap of 2 blocking threads ("The CLI
stopped responding") and passes with 4.

Real CLIs (`cargo test --test cli -- --ignored`, Claude Code 2.1.310, Codex 0.153.4): all three
pass. Codex replies now arrive in many pieces; a direct app-server probe showed the first delta
about 4.5 s after start, where `codex exec --json` delivered the whole reply at about 13 s.

Checked by hand on Windows 11 with a debug build and a scratch `BLOOP_DATA_DIR`:

| Check | Result |
| --- | --- |
| Seeded reply: heading, bold, inline code, list, link, inline math as Unicode | Pass |
| Display math: integral with limits, sum with limits above/below, stacked fraction, 2×2 matrix | Pass |
| Rust code block: language label, Copy button, syntax colours | Pass |
| "It costs $5 and $10" stays plain text | Pass |
| Live Claude Code reply streams into the notch with display math and a Python block mid-stream | Pass (after the blocking-thread fix; before it, Claude reported no input on stdin) |

Not checked: a Codex reply inside the running app (the attempt was abandoned when simulated
keystrokes left the notch), clicking Copy and links, the light theme, non-Windows fonts.
Release executable: 18.1 MiB (15.2 MiB for the installed 0.3.1), mostly syntect's syntaxes.

Landing page: both JSON-LD blocks parse, the 9 FAQ answers match their `FAQPage` entries, the
description is 144 characters, and the page has no horizontal scroll at 375 px.

## Sign in with ChatGPT (2026-11-11, written by Codex, reviewed here)

Automated: on the combined tree, `cargo fmt --check` and `cargo clippy --all-targets -- -D
warnings` are clean and `cargo test` passes (75 library tests, including callback parsing with
state and client checks, a loopback listener that ignores unrelated requests, ID-token
issuer/audience/nonce/expiry binding, and a DPAPI round trip of a 5000-byte session).

Reviewed: PKCE (S256), random state and nonce, a loopback-only callback that checks the Host
header and rejects duplicate parameters, RS256 ID-token signature checks against the issuer's
JWKS, revocation only on the issuer's origin, no redirects or proxies, bounded response bodies,
DPAPI-encrypted session storage, and Responses requests with `store: false`.

Not checked: a real sign-in, token refresh or ChatGPT reply. Those need a person to approve the
sign-in in a browser with an eligible ChatGPT plan.

## Paced reveal and reply moods (2026-11-12)

Measured: a one-off probe with the real Codex CLI (`cli::chat` on a runtime capped at
`BLOCKING_THREADS`) logged a timestamp for each delta. The first token came at 5.1 s and the
last at 6.5 s, often several per read, which confirmed that streaming worked but was too
quick to see. The probe was removed afterwards.

Automated: `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean, and
`cargo test` passes. New UI state tests: a burst types out over several ticks and Bloop laughs
only after it has caught up (joke prompt), reduced motion shows text at once, a failed reply
keeps its partial text and makes Bloop mad, a stopped reply stays mad while older replies
sleep, everything sleeps after the nap delay, and the laugh check ignores "punctuation" and
"lollipop".

On screen: a dev build on scratch data (`BLOOP_DATA_DIR`) with a seeded chat drew the sleeping
Bloop on an older reply and the laughing Bloop ("is cracking up") on a joke reply. Both read
clearly at 22 px.

Not checked on screen: the live run (thinking, then typing, then happy) and the Mad face. The
run to see them ended early: the chat panel had closed before a scripted click, so the click
landed on another window. Unit tests cover these states, but no one has looked at them yet.

## Monitor placement (2026-11-12)

Automated geometry covers left/centre/right, negative desktop coordinates, a nonzero
work-area top, and 100%, 125%, 150%, and 200% scaling. Reviewed the pinned winit 0.30.13
WM_DPICHANGED handler: its suggested-rectangle SetWindowPos completes inside the handler.
The placement callback reapplies the intended physical rectangle after the first move.
Required gates: cargo fmt, cargo clippy --all-targets -- -D warnings, cargo test.
No bloop executable was launched. Actual monitor selection, focus ordering and cross-DPI
movement remain unverified on screen; test both directions between differently scaled
monitors, with the foreground window on each monitor and with a bloop window foreground.
All three gates passed. cargo test required the normal Windows session: the sandbox could not access Credential Manager for two existing secrets tests. Real-CLI and live-GitHub tests remain ignored.

## Independent clipboard shortcut (2026-11-12)

Unit coverage checks distinct Toggle/Ask routing, disabling Ask without affecting Toggle,
invalid syntax, duplicate bindings in either direction, idempotent registration and
preserving the old binding after registration failure. Both shortcuts reuse validate_shortcut.
No desktop hotkeys were registered in these tests; Windows registration and actual key
presses remain unverified. UI integration is intentionally just an empty Event::Ask arm.
Passed: cargo fmt, cargo clippy --all-targets -- -D warnings, and cargo test (normal Windows session; existing live tests ignored).

## Connection diagnostics (2026-11-12)

Loopback tests cover Ollama/OpenAI/Unsloth model-list success, missing and empty models,
401/403, 404, refused port, a silent server timeout, a TLS certificate alert, malformed
JSON, and invalid URLs. Requests are body-free GETs to api/tags or v1/models, with no chat.
Model suggestions stop at five. Unit checks cover CLI path/missing reports and ChatGPT's
signed-out/plan-disabled gates. Live ChatGPT authentication/model access and real CLI
sign-in were not exercised. CLI hints follow the vendors' installation documentation:
https://support.claude.com/en/articles/14554922-claude-code-user-faq
https://github.com/openai/codex/blob/main/README.md
Passed: cargo fmt, cargo clippy --all-targets -- -D warnings, and cargo test (normal Windows session). All eight loopback diagnostics passed; the silent-server check returned in about three seconds.

## Markdown export helpers (2026-11-12)

Unit tests cover transcript order, preserved Markdown/Unicode, interrupted and empty replies,
empty titles, invalid filename characters, case-insensitive device names (including those
with extensions and superscript digits), extension normalization and UTF-16 length bounds.
The Windows Save-dialog API compiles with its added windows crate feature. Its buffers,
cancellation/error branch and worker-thread ownership were reviewed against Microsoft's
API documentation. The dialog was not opened; save/cancel/overwrite interactions and
non-Windows compilation remain unverified. No bloop executable was launched.
Passed: cargo fmt, cargo clippy --all-targets -- -D warnings, and cargo test (normal Windows session): 220 passed, four existing live-network/CLI tests ignored.

## v1.1–v1.3: reply actions, profiles, clipboard ask, chats (2026-11-12)

Passed on the branch `feat/v1.1-v1.3`: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test` (245 passed, 4 live CLI tests
ignored as usual).

Covered by tests: Retry and Edit & resend replace the last turn and are ignored while
streaming; New chat keeps the old chat listed; switching profile in a chat with history
starts a new chat; a chat's local history isn't sent after the default changes to a cloud
endpoint (Send and Retry); a deleted profile blocks sending; chats imported from the old
single history are never sent; clipboard text is fenced, capped, sent once, refused when
the whole prompt is too long and dropped on chat change or a late read; keys are stored
per endpoint origin and a typed key is dropped when the server or profile changes; the
storage queue keeps the outgoing chat's save ahead of a chat load, a delete removes that
chat's queued saves and runs after the save in flight, and Quit waits for queued jobs;
storage migration, listing, search, rename and delete against real SQLite files; relative
dates. Codex reviewed the branch and found seven problems (all fixed, with tests above); a
second Codex review of the fixes stopped at its usage limit before reporting.

Not verified: none of the new UI was looked at or clicked in a running bloop, because the
installed copy was running and a dev build would only signal it. Not exercised: the Save
dialog, the second global hotkey on the desktop, Test connection against real servers,
moving across monitors with different scaling, and the migration on a real 0.4 database.

## Icon buttons and default profile (2026-11-12)

Passed: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
(249 passed, 4 ignored as usual), built in a separate target directory because a running
debug bloop held `target/debug`.

Covered by tests: every `Icon` has a glyph in the subset font (and a left-out one
doesn't); the star makes a profile the default, New chat starts on it, Save leaves it
alone, deleting it returns the star to Main, and a missing id can't be starred; a profile
is deleted only after Delete is confirmed, Keep cancels; deleting another profile leaves
the one being edited open; `default_profile` round-trips and a dangling one loads as 0.

Not verified: the new icons, tooltips and profile rows weren't looked at in a running
bloop, because a debug copy was running and a second one would only signal it.

## Self-update end to end (2026-11-12)

Automated: `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` are clean.
`cargo test` passes: 93 library, 125 app, 8 CLI-process (3 ignored), 8 diagnose,
10 HTTP-transport and 9 update tests (2 ignored). New tests cover the retry schedule (5 → 10 → 20 min,
reset on success, never more than 6 h), migration leaving another copy's Run value alone, and
the installer's arguments and log path.

Real GitHub (`cargo test --test update -- --ignored`): `releases/latest` reports v0.4.0 with
`bloop-setup-0.4.0.exe` (8,332,809 bytes) and `bloop-setup-0.4.0.exe.sha256` (88 bytes). The
tag `v0.4.0` parses, and the asset names match `installer_name`. A build that thinks it is
0.0.1 finds 0.4.0, downloads it through GitHub's redirect to `release-assets.githubusercontent.com`,
verifies the SHA-256, and `ready()` accepts it.

Dry run (`cargo run --example update_dry_run`, Inno Setup 6.4, Windows 11 26300): fixture
installers 0.0.1 and 9.9.9 built from `installer/bloop.iss /DFixture`. Result:

| Check | Result |
| --- | --- |
| 0.0.1 installs silently with autostart and the desktop icon off | Pass |
| Old copy holds the fixture mutex; the update check against a local fake GitHub finds 9.9.9 | Pass |
| `download_from` streams 4.6 MB, checks size and SHA-256; `ready()` accepts it | Pass |
| Setup started with `update::installer_command` (the app's exact arguments) while the mutex is held | Pass |
| Setup waits: the old copy quits after 3.6 s, setup exits 0 at 4.9 s, and the relaunch comes 1.6 s after the mutex is released | Pass |
| Installed version becomes 9.9.9; the app is relaunched with `--background` | Pass |
| `install.log` is written next to the installer and records success | Pass |
| Unmodified (non-fixture) script still compiles with ISCC | Pass |
| Afterwards no fixture folder, uninstall entry, Start-menu shortcut or Run value is left | Pass |

On this machine, the HKCU Run value was `"C:\github-repos\bloop\target\debug\bloop.exe"
--background`, written by a dev build. That is the cause recorded in `docs/architecture.md`.
The installed copy is 0.4.0, the latest release, and `%LOCALAPPDATA%\bloop\updates` never
existed, so the installed copy never downloaded an update.

Not checked: the fixed app itself updating on the real desktop. A dev bloop was running and
holding the real mutex, and no release newer than 0.4.0 exists. Also not checked: the
`DeinitializeSetup` reopen after a failed install (no forced mid-install failure was staged),
the `CREATE_BREAKAWAY_FROM_JOB` path inside a kill-on-close job, the 6-hour repeat in real
time, and a check after sleep or hibernation.

## Chat queue fixes (2026-11-12)

Export of a chat that isn't open now queues behind any save of it, so the file has its newest
turns. Opening a chat ignores a late read once a reply, new chat, another open or a delete
came in between. Deleting a chat drops queued renames, reads and exports of it but keeps the
settings part of queued saves. A pasted API key is cleared when another server is picked or
a new profile starts. An update waiting on a save also waits for a reply in progress, then
installs after the save that follows it (`a_deferred_update_survives_a_reply`; its check was
corrected to look for the save in flight rather than in the queue). `cargo fmt --check`,
clippy and `cargo test` pass (93 library, 132 app, 8 CLI, 8 diagnose, 10 transport, 9 update).
Not checked in a running bloop.

## Prompt recall with Up and Down (2026-11-12)

`cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test --bin bloop`
(123 passed) are clean. `up_and_down_recall_this_chats_prompts` checks that Up on an empty
prompt brings back the newest prompt and then older ones, stops at the oldest, Down walks back
and clears, and Up does nothing on a typed draft. Recall is off while a reply streams, while
editing, and with clipboard text attached; it only reacts to unmodified arrows the prompt
didn't handle.

Not verified: pressing the keys in a running bloop (another copy held the single-instance
mutex), and the caret landing at the end of the recalled text.

Ctrl+N in the notch starts a new chat (124 tests pass). `ctrl_n_starts_a_new_chat_only_in_the_open_notch`
checks it does nothing while the notch is closed or for another window. Not pressed in a
running bloop.

## Custom instructions (2026-11-12)

Passed: `cargo fmt`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
and `cargo test` (260 passed, four existing live CLI/network tests ignored). The suite
was rerun after boxing save-job config snapshots. The initial sandbox run could not
access Credential Manager; the complete suite passed with normal Windows access.

Covered: old config defaults, trimmed multiline round-trip, UTF-8 byte limits,
unchanged empty system text, appended instructions and exact context-budget boundary;
Ollama/OpenAI HTTP payloads via loopback fixtures, ChatGPT developer payload and CLI
flattening; editor loading, edits, Save/clear, oversized draft rejection, shared
instructions across profiles and draft reset on reopening.

Not checked: appearance or interaction in a running app, or instruction-following by
live models. No bloop app was launched; the installed copy was left running.

## Notch context, icon buttons and dropdowns (2026-11-12)

Passed: `cargo fmt`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
and `cargo test` (266 passed, four existing live CLI/network tests ignored). Builds used
the worktree's default `target/debug`; `CARGO_TARGET_DIR` was not set. The initial sandbox
run failed `secrets::tests::blank_key_removes` and `secrets::tests::credential_round_trip`
because Credential Manager had no logon session; both passed with normal Windows access.

Covered: all three context modes select the newest complete pairs in chronological
order (12, 3, or none), skip interrupted turns, keep the system message/instructions and
current prompt, and respect the 32 KiB budget. Old config defaults and all mode round-trips
pass. UI state tests check immediate application and queued saves without saving pending
Model edits, and key paste trimming, empty clipboard, provider/endpoint/profile mismatches
and environment-key handling. The existing subset-font test verifies all reused icons.

Reviewed Iced widget 0.14.2 source: pick-list padding flows to the overlay menu, while
menu style has no item-padding field. Reviewed all NotchWidth panel sizes: S is 480 px
(424 px inside panel padding), M 540 px and L 600 px. The model control is bounded and
clipped to 156 px; wrapping the footer keeps its controls within the available width,
including word fallbacks. Settings/setup dropdowns share the larger item padding.

UI not viewed in a running app. No bloop executable was launched. Actual clipboard
access, tooltip placement, copied-state appearance and dropdown/footer rendering at
different Windows scales remain unverified. The mascot implementation and Appearance
mascot toggle were left untouched. No dependencies or font glyphs were added.

## Mascot outfits (2026-11-12)

`cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass (101 library,
128 app, and the CLI, diagnose, transport and update suites). New tests: a `Look`
round-trips, old configs load the default and a partial one fills in the rest; **Surprise
me** never repeats the current outfit (500 seeds); picking a look applies and saves at once;
clicking Bloop makes it laugh.

Checked by eye only in an SVG mock that copies the drawing's coordinates (idle face, all hats
but party and wizard, all eyewear, all outfits): everything sits on the body and inside the
canvas. Not checked: the real Iced drawing in a running bloop (another copy held the
single-instance mutex), the party and wizard hats, other moods with accessories (a hop with a
tall hat, sleepy z and mad mark beside a hat), and the Appearance page's layout at 900 px.


## 0.5.0 release build (2026-11-12)

`cargo build --release` and the full checks pass on the merged branch (104 library, 140 app,
8 CLI, 8 diagnose, 11 transport, 9 update). The release exe was started against a scratch
`BLOOP_DATA_DIR` with outfits seeded into its settings, and screenshots were taken:

| Check | Result |
| --- | --- |
| Setup screen's 96 px Bloop in wizard hat, round glasses, robe, lilac coat | Pass: hat sits on the head inside the canvas, glasses frame the eyes, robe follows the body |
| Party hat, shades, bow tie, mint coat | Pass |
| Top hat, monocle, cape, classic coat | Pass: cape hangs behind the body |
| App window Home page reports bloop 0.5.0 | Pass |

Not checked: the Appearance page's Bloop section, the Model page (custom instructions, Paste
key, bigger dropdowns) and the open notch (context selector, header and greeting Bloop,
prompt recall, Ctrl+N). Synthetic mouse clicks and hover didn't reach the window, and
AGENTS.md says not to drive the desktop, so these need a person.

## Profile ids are never reused (2026-11-12)

Review finding on PR #7: deleting the highest-numbered profile let the next new profile take
its id, so chats that kept the old id would quietly continue on the new profile's model.
`Config.last_profile_id` now records the highest id ever handed out (raised on create and on
delete, and never below a saved profile's id on load), and `next_profile_id` counts from it.
`a_deleted_profiles_id_is_never_reused` covers delete → create → reload. Full checks pass
(104 library, 141 app, integration suites). Limitation: a config saved by 0.5.0 or older
after deleting its top profile has no record of that id; the first delete with this build
starts recording.
