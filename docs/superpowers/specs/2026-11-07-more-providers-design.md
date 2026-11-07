# More providers: Unsloth Studio, Claude Code, Codex — design

Date: 2026-11-07 · Status: awaiting review

## Goal

People can point bloop at what they already have installed:

- **Unsloth Studio** running a local model, with its `sk-unsloth-…` key.
- **Claude Code** (`claude`) or **Codex** (`codex`) CLIs, using the account each CLI is
  already signed in to — no API key pasted into bloop.

Success: pick "Claude Code" in Settings (or in setup), ask a question from the notch, and the
reply streams in from the user's own Claude subscription; same for Codex; and Unsloth Studio
works by choosing it and pasting the key once.

## Decisions (from brainstorming)

- **CLIs are chat only.** No tools, file edits, commands or web access. bloop's
  "no tool execution in this milestone" rule stands.
- **Keys go to Windows Credential Manager**, never to bloop's SQLite database or logs.
  `BLOOP_API_KEY`, when set, still wins.
- **One CLI process per message.** bloop sends the whole conversation each turn (as it already
  does for HTTP providers) and kills the process on cancel. No CLI sessions are kept.

Out of scope: agent/tool modes, Claude/Codex image input, choosing a working folder,
Linux/macOS credential stores, Unsloth's Anthropic-compatible `/v1/messages` endpoint,
auto-starting Unsloth Studio.

## Providers

`config::Provider` becomes `Ollama | OpenAi | Unsloth | ClaudeCode | Codex`
(serde names unchanged for the existing two; old configs load as before).

| Provider | Transport | Default endpoint | Model field |
|---|---|---|---|
| Ollama | HTTP NDJSON (existing) | `http://127.0.0.1:11434` | required |
| OpenAI-compatible | HTTP SSE (existing) | `http://127.0.0.1:8080/v1` | required |
| Unsloth Studio | HTTP SSE (OpenAI path, existing decoder) | `http://127.0.0.1:8888/v1` | required |
| Claude Code | `claude` subprocess | — (hidden) | optional (blank = CLI default) |
| Codex | `codex` subprocess | — (hidden) | optional (blank = CLI default) |

`Config::validate` skips endpoint validation for the CLI providers and allows an empty model
for them only (still ≤ 200 bytes, and limited to `[A-Za-z0-9._:/-]` so it can't smuggle flags).

## Keys

- New `src/secrets.rs` (library, Windows-only body): `api_key() -> Option<String>` returns
  `BLOOP_API_KEY` if set, else the Credential Manager generic credential `bloop/api-key`;
  `set_api_key(Option<&str>) -> Result<(), String>` writes or deletes it (`CredWriteW`,
  `CRED_PERSIST_LOCAL_MACHINE`, `CredDeleteW`). Max 4 KiB.
- HTTP providers (OpenAI-compatible, Unsloth, and Ollama if a key is present) send it as a
  bearer token, replacing today's direct `BLOOP_API_KEY` read.
- Settings and setup show an **API key** password field for OpenAI-compatible and Unsloth.
  It shows "Saved in Windows Credential Manager" when a key exists, never the key itself;
  Save writes it, a "Remove key" button deletes it. When `BLOOP_API_KEY` is set the field is
  replaced by "Using BLOOP_API_KEY from the environment".

## Unsloth Studio

- A preset over the existing OpenAI-compatible transport (`chat/completions`, `models`).
- Setup detection tries `http://127.0.0.1:8888/v1` then `:8000/v1` with the saved key; the
  first that lists models wins and becomes the endpoint.
- HTTP 401 from Unsloth → "Unsloth Studio rejected the key. Copy the key it printed when the
  model loaded into Settings."

## Claude Code and Codex

New `src/cli.rs` (library):

- `find(kind) -> Option<PathBuf>`: looks up `claude` / `codex` on `PATH`, accepting
  `.exe`, `.cmd` and `.ps1` shims (npm installs `.cmd`); `.cmd` is run via `cmd /C` with
  arguments passed as separate argv entries, never a joined string.
- `chat(kind, model, messages, emit) -> Result<(), provider::Error>` — same shape as
  `provider::chat`; `provider::chat` dispatches to it for the CLI providers.
- Working directory: `%LOCALAPPDATA%\bloop\cli` (created empty) so nothing in the user's
  projects is in reach. `CREATE_NO_WINDOW` so no console flashes.
- Prompt: the conversation flattened to text (`User:` / `Assistant:` turns, last user turn
  last), written to the child's **stdin** (never argv: no length limit, no quoting issues),
  then stdin is closed. Capped by the existing `MAX_PROMPT`/context limits.
- **Claude Code:** `claude -p --output-format stream-json --verbose
  --include-partial-messages --tools "" --no-session-persistence --strict-mcp-config
  --setting-sources "" [--model M]`. Text comes from `stream_event` content-block text
  deltas; `result` with `is_error: false` ends the reply; `is_error: true` → its message.
- **Codex:** `codex exec --json --ephemeral --skip-git-repo-check --sandbox read-only
  --color never -C <cli dir> [-m M] -`. Text from agent-message items (deltas when present,
  else the completed item); `turn.completed` ends; `turn.failed` / `error` → message. Both
  observed event shapes (`type`-based and `method`-based) are accepted.
- **Chat-only enforcement:** any tool / command / file-change / web-search / MCP event from
  either CLI aborts the reply with "Tool execution is unavailable in this milestone." and
  kills the process. (Claude has tools disabled up front; Codex has no switch for that, so
  this is its guard.)
- Decoding reuses the provider limits: max line 256 KiB, max reply 128 KiB, empty reply is an
  error. Deadlines: no output for 60 s or 180 s total → kill and fail.
- Cancel: the Iced task abort drops the child handle, which kills the process tree
  (`kill_on_drop`, plus a job object so `cmd /C` children die too).
- Stderr is read but only its last line (≤ 300 chars, after stripping anything that looks
  like a key) is shown on failure.

## Setup and Settings

- Provider picker lists all five. Endpoint field hidden for CLI providers; model field shows
  "Leave blank for the CLI's default".
- Setup's model step, in addition to Ollama, notices Unsloth (8888/8000) and installed CLIs
  and lists what it found as choices: "Ollama — qwen3:4b", "Unsloth Studio",
  "Claude Code", "Codex". Picking one fills provider/endpoint/model. Not-found behaviour is
  unchanged.
- CLI not installed when chosen → "Claude Code isn't installed. Install it, or pick another
  provider." CLI signed out → the CLI's own error line, plus "Run `claude` (or `codex`) once
  in a terminal to sign in."

## Architecture fit

- `docs/architecture.md` gets a line: CLI providers are subprocesses in chat-only mode;
  bloop never enables their tools; Win32 calls (credentials, job objects) stay in
  `secrets.rs` / `cli.rs` behind `cfg(windows)`.
- New `windows` features: `Win32_Security_Credentials`, `Win32_System_JobObjects`.
  `tokio` gains `process`.

## Testing

- Unit: config round-trip for new providers and old configs; validation (empty model OK only
  for CLIs, model charset); conversation flattening; Claude and Codex stream decoders on
  recorded fixtures (text deltas, completion, error result, tool event → refusal, oversized
  line, empty reply, both Codex shapes); `find` against a temp `PATH` with `.exe`/`.cmd`.
- Integration: a fake CLI (`examples/fake_cli.rs`, built by the test) that replays a fixture
  on stdout — proves stdin delivery, streaming, cancel kills it, and the no-output deadline.
- Credential Manager round-trip on a test target name (`bloop-test/api-key`), Windows only.
- Unsloth: transport tests reuse the OpenAI fixtures with a bearer-token assertion.
- Manual: real Claude Code and Codex replies from the notch; cancel mid-reply leaves no
  `claude`/`codex` process running; Unsloth Studio with and without a key.
