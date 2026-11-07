# More Providers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** bloop can talk to Unsloth Studio (with a key kept in Windows Credential Manager) and to the user's installed Claude Code and Codex CLIs in chat-only mode.

**Architecture:** `Provider` grows three variants. Unsloth reuses the OpenAI-compatible HTTP transport through a `Wire` mapping. A new library module `cli` spawns one CLI process per message, feeds the flattened conversation on stdin, decodes its JSONL stream into the existing `StreamEvent`s, and refuses any tool event. A new library module `secrets` stores the API key in Credential Manager.

**Tech Stack:** Rust 2024, tokio (`process`), `windows` 0.62 (`Win32_Security_Credentials`, `Win32_System_JobObjects`), Iced 0.14.

**Spec:** `docs/superpowers/specs/2026-11-07-more-providers-design.md`

## Global Constraints

- CLIs are chat only: no tools, file edits, commands, web or MCP. Any such event aborts with "Tool execution is unavailable in this milestone." and kills the process.
- Keys never go to SQLite or logs; Credential Manager target `bloop/api-key`; `BLOOP_API_KEY` wins when set; max key 4 KiB.
- Defaults: Unsloth `http://127.0.0.1:8888/v1` (detection also tries `:8000/v1`).
- CLI working dir `%LOCALAPPDATA%\bloop\cli`; prompt via stdin only; `CREATE_NO_WINDOW`.
- Claude args: `-p --output-format stream-json --verbose --include-partial-messages --tools "" --no-session-persistence --strict-mcp-config --setting-sources "" [--model M]`.
- Codex args: `exec --json --ephemeral --skip-git-repo-check --sandbox read-only --color never -C <cli dir> [-m M] -`.
- CLI limits: line 256 KiB, reply 128 KiB (`MAX_OUTPUT`), empty reply is an error, 60 s without output or 180 s total → kill and fail.
- CLI model: may be empty; ≤ 200 bytes; only `[A-Za-z0-9._:/-]`.
- Copy: "Unsloth Studio rejected the key. Copy the key it printed when the model loaded into Settings."; "Claude Code isn't installed. Install it, or pick another provider." (same for Codex); sign-in hint "Run `claude` once in a terminal to sign in." / "Run `codex` once…"; key status "Saved in Windows Credential Manager" / "Using BLOOP_API_KEY from the environment"; model placeholder "Leave blank for the CLI's default".
- Win32 calls only in `secrets.rs`, `cli.rs`, `platform.rs`, each `unsafe` with `// SAFETY:`.

## Review Focus

1. A `.cmd` shim (npm Codex) cancelled mid-reply → no `node`/`codex` process left running. Test in Task 5.
2. The CLI prints a huge single line or never ends a line → bounded error, no memory growth. Test in Task 4.
3. Model field holding `--dangerous-flag` or a space → rejected by validation, never reaches argv. Test in Task 1.
4. A conversation containing `User:` text inside a message → flattening stays unambiguous (each turn delimited). Test in Task 4.
5. Credential save with an empty / whitespace key → treated as "remove", not saving an empty secret. Test in Task 2.

---

### Task 1: Provider variants and validation

**Files:** Modify `src/config.rs`, `src/provider.rs` (match arms only), `src/ui.rs` (provider pick list + default endpoint).

**Interfaces — Produces:**
- `enum Provider { Ollama, OpenAi, Unsloth, ClaudeCode, Codex }` (serde names as variant names; Display: "Ollama", "OpenAI / llama.cpp", "Unsloth Studio", "Claude Code", "Codex").
- `Provider::ALL: [Provider; 5]`, `fn is_cli(self) -> bool`, `fn default_endpoint(self) -> &'static str` ("" for CLIs).
- `enum Wire { Ollama, OpenAi, Cli }` and `fn wire(self) -> Wire` (Unsloth → OpenAi).
- `Config::validate`: CLIs skip endpoint check; empty model allowed only for CLIs; CLI model charset check with error "Model may contain only letters, digits and . _ : / -".

- [ ] Step 1: tests in `config.rs`: `old_provider_names_still_load` (`"OpenAi"` → OpenAi); `cli_model_may_be_blank_but_not_flags` (ClaudeCode + "" ok; + "--x" err; + "a b" err; Ollama + "" err); `unsloth_uses_openai_wire`.
- [ ] Step 2: `cargo test --lib config` → compile fail.
- [ ] Step 3: implement; provider.rs `Decoder`/`chat`/`list_models` match on `provider.wire()` (Cli arm in `chat`/`list_models` returns `Error::Protocol("internal: CLI provider on HTTP path")` until Task 5); ui Provider message uses `default_endpoint()`, pick list uses `Provider::ALL`.
- [ ] Step 4: `cargo test` PASS, clippy clean.
- [ ] Step 5: commit `feat: Unsloth, Claude Code and Codex provider choices`.

### Task 2: API key in Credential Manager

**Files:** Create `src/secrets.rs`; modify `src/lib.rs`, `Cargo.toml` (`Win32_Security_Credentials`), `src/provider.rs` (both bearer sites).

**Interfaces — Produces:**
- `secrets::api_key() -> Option<String>`; `secrets::set_api_key(key: Option<&str>) -> Result<(), String>`; `secrets::key_source() -> KeySource` with `enum KeySource { Environment, Saved, None }`.
- Private `read(target: &str)`, `write(target: &str, key: Option<&str>)` so tests use `bloop-test/api-key`.

- [ ] Step 1 (Windows tests): `credential_round_trip` (write "sk-unsloth-abc" → read same; write None → read None; second delete Ok); `blank_key_removes` (write Some("  ") → read None); `oversized_key_rejected` (4097 bytes → Err).
- [ ] Step 2: run → compile fail.
- [ ] Step 3: implement with `CredWriteW` (`CRED_TYPE_GENERIC`, `CRED_PERSIST_LOCAL_MACHINE`, UTF-8 blob), `CredReadW` + `CredFree`, `CredDeleteW` (`ERROR_NOT_FOUND` → Ok). Provider bearer uses `secrets::api_key()`.
- [ ] Step 4: `cargo test` PASS.
- [ ] Step 5: commit `feat: keep the API key in Windows Credential Manager`.

### Task 3: Unsloth over HTTP

**Files:** Modify `src/provider.rs`, `tests/transport.rs`.

**Interfaces — Produces:** HTTP 401 with `config.provider == Unsloth` → `Error::Protocol(<Unsloth copy>)` in both `chat` and `list_models`.

- [ ] Step 1: transport tests: `unsloth_streams_like_openai_with_bearer` (set `BLOOP_API_KEY=sk-unsloth-t` for the test via a serial guard; request has `authorization: bearer sk-unsloth-t`, `POST /v1/chat/completions`); `unsloth_401_explains_the_key` (error contains "Unsloth Studio rejected the key").
- [ ] Step 2: fail. Step 3: implement. Step 4: `cargo test --test transport` PASS.
- [ ] Step 5: commit `feat: Unsloth Studio provider`.

### Task 4: CLI decoding (pure)

**Files:** Create `src/cli.rs` (pure part), `tests/fixtures/claude-ok.jsonl`, `claude-error.jsonl`, `codex-ok.jsonl`, `codex-tool.jsonl`; modify `src/lib.rs`.

**Interfaces — Produces:**
- `enum Kind { Claude, Codex }`; `Kind::of(Provider) -> Option<Kind>`; `Kind::program(self) -> &'static str` ("claude"/"codex").
- `fn args(kind: Kind, model: &str, dir: &Path) -> Vec<String>` (exact Global Constraints lists).
- `fn flatten(messages: &[ChatMessage]) -> String`: each turn as `<role>\n<content>\n</role>` blocks (`<user>`, `<assistant>`), preceded by one line "Continue this conversation as the assistant. Reply to the last user message only." Ensures #4.
- `struct LineDecoder { kind, pending: Vec<u8>, size: usize }` with `push(&[u8]) -> Result<Vec<StreamEvent>, Error>` and `finish() -> Result<Vec<StreamEvent>, Error>`; per-line parse:
  - Claude: `stream_event` + `content_block_delta`/`text_delta` → Delta; `stream_event` + `content_block_start` with `content_block.type` ≠ `text`/`thinking` → tool refusal; `result` with `is_error:false` → Done, `is_error:true` → `Error::Protocol(result text, ≤300 chars)`; other types ignored (including `assistant`, to avoid duplicate text).
  - Codex: `item.started`/`item.completed` whose `item.type` ∈ {command_execution, file_change, mcp_tool_call, web_search} → refusal; `item.completed` with `agent_message`/`assistant_message` → Delta(text) unless deltas already arrived for that item id; method-shape `item/agentMessage/delta` → Delta(`params.delta`); `turn.completed` / `turn/completed` → Done; `turn.failed`/`error` → Protocol(message).
  - Line > 256 KiB → Protocol("CLI output line is too large."); reply > MAX_OUTPUT → Protocol("Response exceeded 128 KiB. Ask for a shorter answer."); `finish()` without Done → Protocol("The CLI stopped before finishing its reply."); Done with no text → Protocol("Model returned an empty response. Try another model or prompt.").

- [ ] Step 1: record real fixtures once: `echo hi | claude -p <args> > tests/fixtures/claude-ok.jsonl` and `echo "say hi" | codex <args> > tests/fixtures/codex-ok.jsonl` (in an empty temp dir); hand-write `claude-error.jsonl` (one `result` with `is_error:true`) and `codex-tool.jsonl` (one `item.started` command_execution). Strip any session ids/emails.
- [ ] Step 2: unit tests: `claude_fixture_streams_text_and_completes`, `claude_error_result_fails`, `codex_fixture_streams_text_and_completes`, `codex_tool_event_is_refused`, `codex_method_shape_deltas` (inline JSONL), `oversized_line_fails` (300 KiB no newline), `missing_done_fails`, `flatten_delimits_turns` (content containing "User:" stays inside its `<user>` block; last block is the final user message), `args_are_exact` (Claude with model "sonnet"; Codex without model).
- [ ] Step 3: run → fail. Step 4: implement. Step 5: `cargo test --lib cli` PASS.
- [ ] Step 6: commit `feat: decode Claude Code and Codex output`.

### Task 5: CLI processes

**Files:** Modify `src/cli.rs`, `src/provider.rs` (dispatch), `Cargo.toml` (`tokio` `process`, `windows` `Win32_System_JobObjects`); create `examples/fake_cli.rs`, `tests/cli.rs`.

**Interfaces — Produces:**
- `fn find(kind: Kind) -> Option<PathBuf>` (PATH × PATHEXT-style `.exe`, `.cmd`, `.bat`; `.ps1` ignored) and `fn find_in(name, path_var: &OsStr) -> Option<PathBuf>` for tests.
- `async fn stream<F>(kind: Kind, program: &Path, args: &[String], prompt: &str, emit) -> Result<(), Error>` — spawns (`.cmd`/`.bat` via `cmd /C <program> <args…>`), `CREATE_NO_WINDOW`, stdin piped → write prompt → close; stdout through `LineDecoder`; deadlines 60 s idle / 180 s total; child in a Job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, job handle closed on drop (kills tree on cancel/error); stderr collected (last line ≤ 300 chars, `sk-…`-looking tokens replaced by `[key]`) appended to non-protocol failures.
- `async fn chat<F>(kind, model, messages, emit)` = find (missing → install copy) + create cli dir + args + flatten + stream. Non-zero exit with stderr mentioning "login"/"auth"/"sign in" → add the sign-in hint.
- `provider::chat` and `provider::list_models` route `Wire::Cli` to `cli::chat` / `Ok(vec![])`.

- [ ] Step 1: `examples/fake_cli.rs`: echoes stdin to `$FAKE_STDIN`, replays `$FAKE_FIXTURE` line by line (`$FAKE_DELAY_MS` between lines), then `$FAKE_HANG=1` sleeps forever; spawns a grandchild sleeper when `$FAKE_CHILD=1` writing its pid to `$FAKE_CHILD_PID`.
- [ ] Step 2: `tests/cli.rs` (build example via `escargot`-free approach: `env!("CARGO_BIN_EXE_…")` isn't available for examples, so tests run `cargo build --example fake_cli` once through `std::sync::Once` and use `target/debug/examples/fake_cli.exe`): `stdin_receives_prompt_and_reply_streams`; `cancel_kills_process_tree` (drop the future mid-stream with FAKE_CHILD=1, then assert both pids gone within 2 s); `cmd_shim_runs_and_is_killed` (write a temp `fake.cmd` that calls the exe; same assertions); `idle_deadline_fails` (FAKE_HANG with a test-only shortened deadline via `stream_with_limits`); `find_in_prefers_exe_then_cmd`.
- [ ] Step 3: run → fail. Step 4: implement. Step 5: `cargo test` PASS, clippy clean.
- [ ] Step 6: commit `feat: run Claude Code and Codex in chat-only mode` (real-CLI checks happen in the app in Task 6).

### Task 6: Settings and setup

**Files:** Modify `src/ui.rs`, `src/ui/onboarding.rs`, `src/onboarding.rs`.

**Interfaces:**
- Consumes: Tasks 1, 2, 5 (`Provider::ALL`, `is_cli`, `default_endpoint`, `secrets::*`, `cli::find`).
- Produces: `Detection::Found` becomes `Found(Vec<Choice>)` with `struct Choice { provider: Provider, endpoint: String, model: String, label: String }` (labels: "Ollama — <model>", "Unsloth Studio — <model>", "Claude Code", "Codex"); new messages `ApiKey(String)`, `SaveKey`, `RemoveKey`, `PickChoice(usize)`; field `key_draft: String`, `key_status: KeySource`.

Behaviour: detection runs Ollama, Unsloth 8888 then 8000, and `cli::find` for both CLIs concurrently (each 3 s cap), collecting every hit; the previously selected provider, if found, is preselected, else the first. Settings: endpoint hidden for CLIs; model placeholder per Global Constraints for CLIs; key field (`text_input(..).secure(true)`) for OpenAi/Unsloth with Save/Remove and the two status copies; old "Cloud credentials come from…" line replaced by "Keys are stored in Windows Credential Manager, never in bloop's files. Remote endpoints require HTTPS.". `SaveSettings` with a non-empty `key_draft` calls `set_api_key` (error → status line), then clears the draft.

- [ ] Step 1: ui tests: `detection_lists_every_found_provider` (feed `Detected(Ok(vec![Choice…×3]))`, picking index 2 sets provider/endpoint/model); `cli_provider_hides_endpoint_and_allows_blank_model` (select ClaudeCode, model "", SaveSettings → config saved, no status error); `onboarding_tests_still_pass` (existing ones adapted to `Choice`).
- [ ] Step 2: fail. Step 3: implement. Step 4: `cargo test`, clippy clean.
- [ ] Step 5: manual on the dev build with a fresh `BLOOP_DATA_DIR`: setup lists Claude Code and Codex; pick Claude Code → reply streams; Cancel mid-reply → `tasklist` shows no claude/node left; same for Codex; Unsloth (if a model is loaded) with key saved via Settings.
- [ ] Step 6: commit `feat: pick Unsloth, Claude Code or Codex in setup and Settings`.

### Task 7: Docs and verification

- [ ] `docs/architecture.md`: CLI providers line (spec "Architecture fit").
- [ ] `docs/verification.md`: automated results + the Task 6 manual checklist results.
- [ ] `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` clean.
- [ ] Commit `docs: providers architecture and verification`.
