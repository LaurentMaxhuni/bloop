# Bloop: Engineering Instructions

Bloop is a lightweight, local-first AI desktop companion that lives in a small notch at the top of the screen.

## Technology
- Rust for the application core.
- Iced 0.14 for the native GUI (DirectX 12 via wgpu). Slint was evaluated and rejected; see `docs/architecture.md`.
- SQLite for persistent data when needed.
- Local AI through OpenAI-compatible HTTP APIs (plus Ollama's API); the Claude Code and Codex CLIs in chat-only mode; ChatGPT plans through Sign in with ChatGPT and the Responses API.
- Windows first, Linux support later.

## Hard constraints
- No Electron, React, Node.js, Chromium, or embedded webviews.
- No telemetry or background network requests (the only exception is the user-switchable update check to GitHub Releases). ChatGPT sign-in, token refresh and model lists happen only on user actions.
- Avoid unnecessary dependencies and subprocesses.
- Prefer event-driven operations over polling.
- The interface must remain responsive during AI inference.
- Keep the agent engine separate from the GUI.
- Never execute destructive system actions without explicit approval.

## Development rules
- Work in small, independently testable milestones.
- Inspect existing code before modifying it.
- Explain significant architectural decisions (record them in `docs/architecture.md`).
- Run cargo fmt, cargo clippy, and cargo test.
- Do not claim features are working without verification.
- Do not add unrelated features or dependencies.
- Report files changed, tests run, and limitations.

## Design
- Small, pill-shaped notch at the top center.
- Two expressive animated eyes.
- Smooth expansion and collapse.
- Dark background, restrained accent colors.
- Minimal CPU usage while idle.

## Map
One Cargo package: a library (`src/lib.rs`, no GUI code) and the `bloop` binary (`src/main.rs`).

| Path | What lives there |
| --- | --- |
| `src/provider.rs` | HTTP providers: Ollama NDJSON and OpenAI-compatible SSE, deadlines, size bounds; dispatches CLI providers to `cli.rs` |
| `src/chatgpt.rs` | Sign in with ChatGPT: OAuth with PKCE, loopback callback, ID-token verification, DPAPI-protected session, token refresh |
| `src/cli.rs` | Claude Code (`claude -p --output-format stream-json`) and Codex (`codex app-server`, JSON-RPC) as chat-only providers; tool refusal; Windows job object that kills the process tree |
| `src/math.rs` | Finds TeX in replies, inline TeX to Unicode, display TeX to ```` ```math ```` fences, TeX parser |
| `src/model.rs`, `src/storage.rs`, `src/config.rs` | Turns and context window, SQLite state, settings and validation |
| `src/secrets.rs`, `src/update.rs`, `src/launch.rs`, `src/onboarding.rs` | Credential Manager and DPAPI, self-update, launch modes, provider detection |
| `src/ui.rs`, `src/ui/*.rs` | Iced daemon: notch and app window; `ui/rich.rs` renders replies (Markdown, code blocks, laid-out math) |
| `src/platform.rs` | All other Win32: tray, hotkey, window region, single instance, autostart, opening links |
| `src/presence.rs`, `src/motion.rs`, `src/mascot.rs` | Notch state machine, springs, the mascot |
| `examples/fake_cli.rs`, `examples/stream_fixture.rs` | Test doubles: a fake CLI and a labelled streaming server |
| `site/` | Landing page (static HTML/CSS/JS, GitHub Pages) |
| `installer/` | Inno Setup installer and release scripts (`release.ps1` pushes the tag; CI builds) |
| `.github/workflows/release.yml` | CI checks and installer build on PRs; tagged releases built, signed (SignPath, once set up) and published |

## Commands
```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --test cli -- --ignored   # runs the real Claude Code and Codex CLIs; spends a little quota
```

## Things that have bitten us
- **Tokio blocking pool.** On Windows, tokio performs child-pipe I/O on blocking threads, and a CLI reply keeps three open (stdin, stdout, stderr). The app's runtime caps blocking threads at `cli::BLOCKING_THREADS`; lowering it starves stdin and the CLI never receives its prompt. `#[tokio::test]` has a large pool and hides this, so `prompt_arrives_on_the_apps_runtime` in `tests/cli.rs` uses a capped runtime.
- **Codex can't skip the user's config in app-server mode.** Isolation comes from `--disable` flags, per-thread `mcp_servers.<name>.enabled = false` (names read via `config/read`), a read-only sandbox and the decoder's allowlist. Any new Codex item type is treated as a tool and refused until added to the allowlist.
- **Single instance.** A global mutex lets one bloop run. Launching a dev build while the user's installed copy runs just signals that copy. Ask before closing it, and use a scratch `BLOOP_DATA_DIR`.
- **Don't drive the desktop with simulated keystrokes.** The notch can lose focus and keystrokes land in whatever window is in front (once that was the user's chat). Prefer state tests, fixtures and screenshots.
- **Unsafe code** needs a `// SAFETY:` comment (`clippy::undocumented_unsafe_blocks` is denied). Keep Win32 calls in `platform.rs`, `cli.rs` and `secrets.rs`.
- **The model is never allowed tools.** CLI tool events, Codex server requests and unknown item types end the reply and kill the process. Keep it that way until the planned permissioned tool registry exists.

## Docs to keep current
- `README.md`: for people using and building bloop.
- `docs/architecture.md`: decisions and why, dated.
- `docs/verification.md`: what was checked, how, and what wasn't.
- `docs/code-signing.md` and `site/code-signing.html`: how releases are signed and the public signing and privacy policy. If bloop gains a new network connection, add it to that page's privacy section.
- `site/index.html` and `site/llms.txt`: public facts (providers, platforms, privacy). Keep the FAQ text and its `FAQPage` JSON-LD identical, and update `softwareVersion` and the sitemap's `lastmod` on release.
