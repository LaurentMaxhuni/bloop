# bloop

A free, open-source AI chat notch for Windows and Linux. bloop rests as a thin sliver at the top of
your screen, pops open when you want it, and chats with a model you choose: Ollama,
llama.cpp, Unsloth Studio, any OpenAI-compatible server, your ChatGPT plan, or the Claude Code
and Codex CLIs.
Native Rust (Iced), with no embedded browser and no bundled model.

**Website:** <https://laurentmaxhuni.github.io/bloop/> ·
**Download:** [latest release](https://github.com/LaurentMaxhuni/bloop/releases/latest) ·
**Licence:** [MIT](LICENSE)

bloop is chat-only. It can't run commands, read files or control your computer.

## Install

Download `bloop-setup-<version>.exe` from the latest release and run it. It installs for your
user only (no admin prompt) into `%LOCALAPPDATA%\Programs\bloop`, adds a Start Menu entry, and
can start bloop when you sign in. The installer isn't code-signed yet (an application to
SignPath Foundation is pending; see the [code signing policy](https://laurentmaxhuni.github.io/bloop/code-signing.html)),
so Windows SmartScreen says "unknown publisher": choose **More info → Run anyway**. Uninstall
from Windows Settings › Apps; it asks before deleting your chats and settings.

Requirements: Windows 10 or 11 and a DirectX 12 graphics driver, plus a model to talk to (see
[Models](#models)). bloop doesn't download or start a model for you.

### Linux

Download `bloop-<version>-x86_64.AppImage` from the latest release, make it executable and run
it:

```bash
chmod +x bloop-*-x86_64.AppImage && ./bloop-*-x86_64.AppImage
```

Keep it somewhere you can write to (for example `~/Applications`) so it can update itself;
from a read-only folder bloop links to the release page instead. On first run it adds itself
to your app menu, and **Start on login** in Settings adds an autostart entry. The AppImage
isn't code-signed: check it against the `.sha256` file next to it.

Requirements: x86_64, glibc 2.35 or newer (Ubuntu 22.04, Fedora 36, Debian 12 or later), a
Vulkan or OpenGL graphics driver, and a keyring (GNOME Keyring, KWallet or KeePassXC) to save
API keys and the ChatGPT sign-in. Without a keyring, set `BLOOP_API_KEY` instead.

Desktop notes:

- **GNOME, KDE Plasma, Xfce, Cinnamon, MATE** work as they are. bloop draws through X11 (XWayland
  on Wayland desktops), so with fractional scaling the notch can look slightly soft.
- **GNOME** shows the tray icon only with the AppIndicator extension (Ubuntu includes it).
- **Global shortcuts on Wayland** go through your desktop's shortcut service (KDE Plasma 6,
  GNOME 48+, Hyprland), which may ask you to confirm the key. Without one (Sway, older GNOME),
  bind `bloop --toggle` and `bloop --ask` to keys in your keyboard settings.
- **Hyprland, Sway and i3** tile windows, so the notch needs one config line. bloop shows it on
  Home with a Copy button; here it is too:

  ```text
  # Hyprland (~/.config/hypr/hyprland.conf)
  windowrulev2 = float, pin, noborder, noshadow, title:^(bloop notch)$
  # Sway (~/.config/sway/config) and i3 (~/.config/i3/config)
  for_window [title="^bloop notch$"] floating enable, sticky enable, border none
  ```

## Use

- **Chat context.** Beside the model under the prompt, choose **Full chat** (up to 12
  recent complete turns within 32 KiB), **Last 3 turns**, or **Only this message**.
  The choice saves immediately and applies to the next send or retry; the system message
  and custom instructions always stay.
- **Custom instructions.** On the Model page, tell Bloop about yourself or how to reply,
  then Save. Up to 2048 bytes are sent with every message, across all model profiles.
- **Sliver → pill → panel.** At rest bloop is a 120 × 5 px sliver at the top centre of the
  screen. Rest the mouse on it to pop out the pill, then click the pill or press
  **Ctrl+Alt+B** (configurable) to open the chat. Click anywhere else to close it. Escape stops
  a reply or collapses the notch. bloop hides while a fullscreen app, presentation or the lock
  screen is up, and peeks out for a few seconds when it has something to say.
- **Replies stream in** as they're written, formatted as Markdown: syntax-highlighted code
  blocks with a **Copy** button, and LaTeX math (`$…$`, `\(…\)`, `$$…$$`, `\[…\]`) drawn as
  formulas: fractions, roots, sums and integrals with limits, matrices. Links open in your
  browser (web and mail links only).
- **Copy, Retry, Edit.** Every reply has **Copy**; the last one also has **Retry** (ask
  again) and **Edit** (fix your message and resend; Escape cancels). Both replace the last
  exchange rather than adding to it.
- **Earlier prompts.** With the prompt empty, **Up** brings back this chat's last message
  and earlier ones; **Down** steps forward and clears it again.
- **Ask about your clipboard.** Copy some text (an error message, a paragraph), press
  **Ctrl+Alt+Shift+A** (configurable, or off) and type your question. The notch shows the
  attached text and where it will be sent before you send it. **Paste clipboard** under the
  prompt does the same.
- **Chats.** **New chat** (or **Ctrl+N** in the notch) starts over and keeps the old one. **All chats** opens the Chats
  page in the app window: search, open in the notch, rename, export to Markdown, or delete.
- **Model profiles.** Save extra providers or models as named profiles on the Model page
  (for example a local model and a cloud one) and switch from the menu under the prompt.
  Each profile row has a star (new chats start on the starred one), a pencil to edit it and
  a bin to delete it; the built-in **Main** profile can be edited but not deleted.
  The label there says **Local** or **Cloud**. A chat stays where its first message went:
  bloop won't send a local chat's history to a cloud model, so switching profile in a chat
  that has messages starts a new chat. **Test connection** on the Model page checks the
  endpoint, key and model without sending a chat.
- **Bloop replies in person.** A little Bloop sits beside each reply: it thinks while the model
  works, mouths along as the text types out, then grins, cracks up at a joke, or gets grumpy
  when a reply fails or you stop it. Older replies doze off. Click Bloop to make it
  giggle. **Settings → Appearance → Bloop** dresses it up: a hat (top hat, beanie, cap,
  crown, party or wizard), shades, glasses or a monocle, a bow tie, scarf, robe or cape, and
  a body colour. **Surprise me** picks a random outfit. With reduced motion the text
  appears at once.
- **App window.** Setup and settings live in a normal 900 × 620 window with Home, Appearance,
  Model, Shortcut & startup and About pages. First launch walks you through setup; after that,
  the notch's settings icon or the tray's **Settings** bring it back. Closing it leaves bloop
  running; quit from the tray. The notch opens on the monitor you're working on when you use
  the shortcut. On **Shortcut & startup**, type a shortcut or click **Record** and press it
  (Ctrl, Alt or Super plus a key); Enter or Esc saves it.
- **Updates.** bloop checks GitHub Releases 30 s after start and every 6 h (switchable); a
  failed check is retried after 5 min, then less often. It checks the download's SHA-256 and
  offers "Update ready — click to restart". Setup's log is `updates\install.log` in the data
  folder.

## Models

| Provider | Endpoint | Model |
| --- | --- | --- |
| Ollama | `http://127.0.0.1:11434` | Installed model name; default `qwen3:4b` |
| llama.cpp server | `http://127.0.0.1:8080/v1` | Server model name or alias |
| Unsloth Studio | `http://127.0.0.1:8888` | A model you've loaded |
| OpenAI-compatible service (LM Studio, vLLM, …) | Your base URL ending in `/v1` | A chat model it serves |
| Claude Code CLI | — (uses the installed `claude`) | Optional, e.g. `sonnet`; blank = CLI default |
| Codex CLI | — (uses the installed `codex`) | Optional; blank = Codex's default model |
| ChatGPT | OpenAI Responses API | Automatic or a model available to your ChatGPT account |

- **OpenAI-compatible providers** use streaming `chat/completions` (Ollama uses its own NDJSON
  API). Remote endpoints must be HTTPS; plain HTTP is allowed only on loopback. API keys come
  from `BLOOP_API_KEY` or Windows Credential Manager (`bloop/api-key/<origin>`, one per
  server, so a key is only sent to the server it was saved for), never the database.
- **CLI providers** use the account you're already signed in with (run `claude` or `codex` once
  in a terminal to sign in). Each message starts the CLI in an empty folder in chat-only mode:
  Claude Code gets no tools, MCP servers or settings; Codex runs as its app server with every
  tool feature off, your MCP servers disabled and a read-only sandbox. Any tool use stops the
  reply. Both stream text as it's written.
- **ChatGPT** uses your eligible ChatGPT plan. In **Settings → Model**, select **ChatGPT**,
  choose **Continue with ChatGPT**, and approve plan usage in your browser. Pick a model or
  leave it on **Automatic**, then choose **Save changes**. Bloop supports one connected ChatGPT
  account at a time. [OpenAI Docs](https://developers.openai.com/siwc/token-sharing-open-source)

With a local model bloop works fully offline. ChatGPT, Claude Code and Codex need the internet.

## Privacy

- Settings and your chats (the last 50 turns of each) live in
  `%LOCALAPPDATA%\bloop\bloop.sqlite3` (plain text; protect it with your Windows account).
  `BLOOP_DATA_DIR` moves it, e.g. for testing. Deleting a chat removes it from that file.
- No analytics or telemetry. If you select ChatGPT, Bloop sends your prompt and recent chat
  context to OpenAI and protects the account session with Windows DPAPI. Other chat content
  goes to the model you configured. You can turn off the GitHub update check in Settings.
- Prompts are limited to 8 KiB and replies to 128 KiB. Full chat keeps up to 12 recent complete
  turns within 32 KiB; the context selector can reduce this to three turns or no history.
  Interrupted replies stay visible but aren't sent back to the model.

## Build from source

Needs Windows 10/11, Rust stable (1.92+), and the Visual Studio C++ Build Tools with the
Windows SDK.

```powershell
cargo build --release
.\target\release\bloop.exe
```

Checks to run before a change lands:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`cargo test` covers the HTTP transport over real loopback connections, CLI process handling
with a fake CLI (`examples/fake_cli.rs`: streaming, deadlines, tool refusal, killing the whole
process tree), the Codex app-server handshake, Markdown/math rendering and UI state. Checks that
run the real CLIs spend a little quota and are opt-in:

```powershell
cargo test --test cli -- --ignored
```

To try streaming without a model, run the labelled fixture server against scratch data:

```powershell
$env:BLOOP_DATA_DIR = Join-Path $PWD 'local-test-data'
cargo run --example stream_fixture
# In another terminal, set the same BLOOP_DATA_DIR, then launch bloop (Ctrl+Alt+Shift+B).
```

Only one bloop runs at a time, so quit the installed copy before launching a dev build.

## Release

- **Installer:** install Inno Setup 6 (`winget install JRSoftware.InnoSetup`), then run
  `powershell -ExecutionPolicy Bypass -File installer\build.ps1`. It writes
  `dist\bloop-setup-<version>.exe` (version from `Cargo.toml`); `installer\make-art.ps1` redraws
  the icon and wizard art.
- **Update dry run:** `cargo run --example update_dry_run` builds fixture installers from
  `installer/bloop.iss /DFixture` (a separate app that never touches an installed bloop), then
  checks, downloads, verifies, installs and relaunches against a local fake GitHub. It needs
  Inno Setup 6.
- **Update:** bump `version` in `Cargo.toml` and merge it to `main`. Then, on an up-to-date
  `main` with the GitHub CLI signed in, run
  `powershell -ExecutionPolicy Bypass -File installer\release.ps1 -Notes "What changed"`. It
  pushes the tag `v<version>`, and `.github/workflows/release.yml` builds `bloop.exe` and the
  installer on GitHub Actions, signs them once SignPath is set up, writes the SHA-256 and
  publishes the release. `-Local` builds and publishes from your PC instead (always unsigned).
  Signing setup: [docs/code-signing.md](docs/code-signing.md).
- **CI:** the same workflow checks (fmt, clippy, tests) on Windows and on Linux (Ubuntu 22.04),
  and builds an unsigned installer and AppImage for pull requests that touch code or the
  installer; download them from the run's artifacts. On tags its `publish` job attaches both to
  one release. `-Local` publishes only the Windows installer.
- **Website:** the landing page is `site/`, published to GitHub Pages by
  `.github/workflows/pages.yml` on pushes to `main`. Preview it with
  `python -m http.server -d site`.

## Status and limits

- Windows and Linux (x86_64); macOS isn't planned. On Windows and X11 desktops the notch moves
  to the active monitor when opened with the shortcut; on Wayland desktops it stays on its
  monitor. Hovering works on the monitor it's on.
- Linux: on Wayland desktops a shortcut can open the notch without giving the prompt keyboard
  focus (the desktop decides); click the notch to type. Fullscreen hiding sees only X11 apps
  there. Per-desktop results: [verification notes](docs/verification.md#linux-port-2026-11-13).
- Memory use is about 100 MiB working set (GPU renderer); the provisional 50 MB target isn't met.
  See [verification notes](docs/verification.md).
- Screen-reader support is incomplete.
- Math covers common LaTeX; unknown commands show as written and big brackets aren't stretched
  to fit.
- No tools yet. A typed tool registry with code-owned permissions, confirmation UI and visible
  logs is the next milestone.

Design decisions and trade-offs: [docs/architecture.md](docs/architecture.md). Agent and
contributor rules: [AGENTS.md](AGENTS.md).
