//! Claude Code and Codex CLIs as chat-only providers: argument lists, the prompt sent on
//! stdin, and decoding their JSONL output into [`StreamEvent`]s.

use crate::{
    config::Provider,
    model::{ChatMessage, MAX_OUTPUT},
    provider::{Error, StreamEvent},
};
use serde_json::Value;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Claude,
    Codex,
}

impl Kind {
    pub fn of(provider: Provider) -> Option<Kind> {
        match provider {
            Provider::ClaudeCode => Some(Kind::Claude),
            Provider::Codex => Some(Kind::Codex),
            _ => None,
        }
    }

    /// Executable name looked up on PATH.
    pub fn program(self) -> &'static str {
        match self {
            Kind::Claude => "claude",
            Kind::Codex => "codex",
        }
    }

    /// Product name for messages.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Claude => "Claude Code",
            Kind::Codex => "Codex",
        }
    }
}

/// Chat-only command line. Claude gets every tool switched off; Codex has no such switch,
/// so it runs read-only without the user's config, plugins, apps or memories, and
/// [`LineDecoder`] refuses any tool event it still emits.
pub fn args(kind: Kind, model: &str, dir: &Path) -> Vec<String> {
    let mut args: Vec<String> = match kind {
        Kind::Claude => [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--tools",
            "",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--setting-sources",
            "",
        ]
        .map(String::from)
        .to_vec(),
        Kind::Codex => [
            "exec",
            "--json",
            "--ephemeral",
            "--skip-git-repo-check",
            "--ignore-user-config",
            "--disable",
            "plugins",
            "--disable",
            "apps",
            "--disable",
            "memories",
            "--sandbox",
            "read-only",
            "--color",
            "never",
            "-C",
        ]
        .map(String::from)
        .to_vec(),
    };
    if kind == Kind::Codex {
        args.push(dir.display().to_string());
    }
    let model = model.trim();
    if !model.is_empty() {
        args.push(
            if kind == Kind::Claude {
                "--model"
            } else {
                "-m"
            }
            .into(),
        );
        args.push(model.into());
    }
    if kind == Kind::Codex {
        // Read the prompt from stdin.
        args.push("-".into());
    }
    args
}

/// The conversation as one prompt. Turns are wrapped in tags so text inside a message
/// (say, a line starting "User:") can never pass for a turn boundary.
pub fn flatten(messages: &[ChatMessage]) -> String {
    let mut prompt = String::from(
        "Continue this conversation as the assistant. Reply to the last user message only.\n",
    );
    for message in messages {
        let role = if message.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        prompt.push_str(&format!("\n<{role}>\n{}\n</{role}>\n", message.content));
    }
    prompt
}

const MAX_LINE: usize = 256 * 1024;

fn refuse() -> Error {
    Error::Protocol("Tool execution is unavailable in this milestone.".into())
}

fn short(message: &str) -> String {
    let message = message.trim();
    match message.char_indices().nth(300) {
        Some((cut, _)) => format!("{}…", &message[..cut]),
        None => message.to_owned(),
    }
}

/// Splits CLI stdout into lines and turns each JSON line into stream events.
pub struct LineDecoder {
    kind: Kind,
    pending: Vec<u8>,
    size: usize,
    done: bool,
    /// Codex item ids that already streamed deltas, so their completed text isn't repeated.
    streamed: Vec<String>,
}

impl LineDecoder {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            pending: Vec::new(),
            size: 0,
            done: false,
            streamed: Vec::new(),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<StreamEvent>, Error> {
        let mut events = Vec::new();
        for byte in bytes {
            if self.done {
                break;
            }
            if *byte == b'\n' {
                let mut line = std::mem::take(&mut self.pending);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if !line.iter().all(u8::is_ascii_whitespace) {
                    self.line(&line, &mut events)?;
                }
            } else {
                if self.pending.len() >= MAX_LINE {
                    return Err(Error::Protocol("CLI output line is too large.".into()));
                }
                self.pending.push(*byte);
            }
        }
        Ok(events)
    }

    pub fn finish(&mut self) -> Result<Vec<StreamEvent>, Error> {
        let events = self.push(b"\n")?;
        if !self.done {
            return Err(Error::Protocol(
                "The CLI stopped before finishing its reply.".into(),
            ));
        }
        Ok(events)
    }

    fn delta(&mut self, text: &str, events: &mut Vec<StreamEvent>) -> Result<(), Error> {
        if text.is_empty() {
            return Ok(());
        }
        self.size += text.len();
        if self.size > MAX_OUTPUT {
            return Err(Error::Protocol(
                "Response exceeded 128 KiB. Ask for a shorter answer.".into(),
            ));
        }
        events.push(StreamEvent::Delta(text.to_owned()));
        Ok(())
    }

    fn complete(&mut self, events: &mut Vec<StreamEvent>) -> Result<(), Error> {
        if self.size == 0 {
            return Err(Error::Protocol(
                "Model returned an empty response. Try another model or prompt.".into(),
            ));
        }
        self.done = true;
        events.push(StreamEvent::Done);
        Ok(())
    }

    fn line(&mut self, line: &[u8], events: &mut Vec<StreamEvent>) -> Result<(), Error> {
        // Non-JSON lines (banners, warnings) are not part of the protocol; skip them.
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            return Ok(());
        };
        match self.kind {
            Kind::Claude => self.claude(&value, events),
            Kind::Codex => self.codex(&value, events),
        }
    }

    fn claude(&mut self, value: &Value, events: &mut Vec<StreamEvent>) -> Result<(), Error> {
        let str_at = |pointer: &str| value.pointer(pointer).and_then(Value::as_str);
        match str_at("/type") {
            Some("stream_event") => match str_at("/event/type") {
                Some("content_block_start") => {
                    let block = str_at("/event/content_block/type").unwrap_or("text");
                    if !matches!(block, "text" | "thinking" | "redacted_thinking") {
                        return Err(refuse());
                    }
                }
                // The answer is complete here; Claude Code may spend a few more seconds on a
                // turn summary before its final `result` line, so don't wait for that.
                Some("message_delta") => match str_at("/event/delta/stop_reason") {
                    Some("end_turn" | "stop_sequence") => self.complete(events)?,
                    Some("max_tokens") => {
                        return Err(Error::Protocol(
                            "Model stopped before completing its reply. Ask for a shorter answer."
                                .into(),
                        ));
                    }
                    Some(_) => return Err(refuse()),
                    None => {}
                },
                Some("content_block_delta")
                    if str_at("/event/delta/type") == Some("text_delta") =>
                {
                    let text = str_at("/event/delta/text").unwrap_or_default().to_owned();
                    self.delta(&text, events)?;
                }
                _ => {}
            },
            Some("result") => {
                let result = str_at("/result").unwrap_or_default().to_owned();
                if value.get("is_error").and_then(Value::as_bool) == Some(true) {
                    let message = if result.is_empty() {
                        "Claude Code reported an error.".to_owned()
                    } else {
                        short(&result)
                    };
                    return Err(Error::Protocol(message));
                }
                // Without partial messages the text only arrives here.
                if self.size == 0 {
                    self.delta(&result, events)?;
                }
                self.complete(events)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn codex(&mut self, value: &Value, events: &mut Vec<StreamEvent>) -> Result<(), Error> {
        let str_at = |pointer: &str| value.pointer(pointer).and_then(Value::as_str);
        // Two observed shapes: `{"type":"item.completed","item":…}` and
        // `{"method":"item/completed","params":{"item":…}}`.
        let (event, item) = match (str_at("/type"), str_at("/method")) {
            (Some(kind), _) => (kind.replace('.', "/"), value.get("item")),
            (None, Some(method)) => (method.to_owned(), value.pointer("/params/item")),
            _ => return Ok(()),
        };
        let item_type = item
            .and_then(|i| i.get("type"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let tool = matches!(
            item_type,
            "command_execution"
                | "commandExecution"
                | "file_change"
                | "fileChange"
                | "mcp_tool_call"
                | "mcpToolCall"
                | "web_search"
                | "webSearch"
        );
        match event.as_str() {
            "item/started" | "item/updated" | "item/completed" if tool => return Err(refuse()),
            "item/agentMessage/delta" => {
                let id = str_at("/params/itemId").unwrap_or_default().to_owned();
                let text = str_at("/params/delta").unwrap_or_default().to_owned();
                if !self.streamed.contains(&id) {
                    self.streamed.push(id);
                }
                self.delta(&text, events)?;
            }
            "item/completed"
                if matches!(
                    item_type,
                    "agent_message" | "assistant_message" | "agentMessage"
                ) =>
            {
                let item = item.unwrap_or(&Value::Null);
                let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
                if !self.streamed.iter().any(|s| s == id) {
                    let text = item
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    self.delta(&text, events)?;
                }
            }
            "turn/completed" => self.complete(events)?,
            "turn/failed" | "error" => {
                let message = str_at("/error/message")
                    .or_else(|| str_at("/message"))
                    .or_else(|| str_at("/params/error/message"))
                    .unwrap_or("Codex reported an error.");
                return Err(Error::Protocol(short(message)));
            }
            // `item.type == "error"` items are warnings (e.g. malformed agent roles); ignore.
            _ => {}
        }
        Ok(())
    }
}

/// How long a CLI may stay silent, and run in total, before bloop gives up.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub idle: Duration,
    pub total: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(60),
            total: Duration::from_secs(180),
        }
    }
}

/// First `<name>.exe`, `.cmd` or `.bat` in the directories of `path_var`.
/// PowerShell shims (`.ps1`) are skipped: they need PowerShell to run.
pub fn find_in(name: &str, path_var: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_var).find_map(|dir| {
        ["exe", "cmd", "bat"]
            .iter()
            .map(|ext| dir.join(format!("{name}.{ext}")))
            .find(|candidate| candidate.is_file())
    })
}

pub fn find(kind: Kind) -> Option<PathBuf> {
    find_in(kind.program(), &std::env::var_os("PATH")?)
}

/// Empty folder the CLIs run in, so none of the user's projects are in reach.
fn work_dir() -> Result<PathBuf, Error> {
    let data = crate::storage::data_path().map_err(Error::Protocol)?;
    let dir = data
        .parent()
        .map(|p| p.join("cli"))
        .ok_or_else(|| Error::Protocol("Could not locate bloop's data folder.".into()))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| Error::Protocol(format!("Could not create {}: {e}", dir.display())))?;
    Ok(dir)
}

/// Runs the installed CLI for one reply.
pub async fn chat<F: Future<Output = bool> + Send>(
    kind: Kind,
    model: &str,
    messages: &[ChatMessage],
    emit: impl FnMut(StreamEvent) -> F + Send,
) -> Result<(), Error> {
    let Some(program) = find(kind) else {
        return Err(Error::Protocol(format!(
            "{} isn't installed. Install it, or pick another provider.",
            kind.name()
        )));
    };
    let dir = work_dir()?;
    let args = args(kind, model, &dir);
    let env: [(&str, &OsStr); 0] = [];
    stream_in(
        kind,
        &program,
        &args,
        &flatten(messages),
        &env,
        Limits::default(),
        Some(&dir),
        emit,
    )
    .await
}

/// Spawns `program`, writes `prompt` to its stdin and decodes its stdout. Extra `env`
/// variables are for tests. Dropping the returned future kills the whole process tree.
pub async fn stream_with<F: Future<Output = bool> + Send, E: AsRef<OsStr>>(
    kind: Kind,
    program: &Path,
    args: &[String],
    prompt: &str,
    env: &[(&str, E)],
    limits: Limits,
    emit: impl FnMut(StreamEvent) -> F + Send,
) -> Result<(), Error> {
    stream_in(kind, program, args, prompt, env, limits, None, emit).await
}

#[allow(clippy::too_many_arguments)]
async fn stream_in<F: Future<Output = bool> + Send, E: AsRef<OsStr>>(
    kind: Kind,
    program: &Path,
    args: &[String],
    prompt: &str,
    env: &[(&str, E)],
    limits: Limits,
    dir: Option<&Path>,
    mut emit: impl FnMut(StreamEvent) -> F + Send,
) -> Result<(), Error> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    // Run the program directly, `.cmd` shims included: the standard library escapes
    // batch-file arguments safely, which a hand-built `cmd /C` line would not.
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    for (key, value) in env {
        command.env(key, value);
    }
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command
        .spawn()
        .map_err(|e| Error::Protocol(format!("Could not start {}: {e}", kind.name())))?;
    // Owns the process tree: closing it (on drop, including cancel) kills every process in it.
    let _job = job::Job::adopt(&child);

    let mut stdin = child.stdin.take().expect("stdin piped");
    let prompt = prompt.to_owned();
    let writer = tokio::spawn(async move {
        let _ = stdin.write_all(prompt.as_bytes()).await;
        let _ = stdin.shutdown().await;
    });
    let mut stderr = child.stderr.take().expect("stderr piped");
    let errors = tokio::spawn(async move {
        // Keep only the tail; CLIs can be chatty on stderr.
        let mut tail = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(n) = stderr.read(&mut buf).await {
            if n == 0 {
                break;
            }
            tail.extend_from_slice(&buf[..n]);
            if tail.len() > 16 * 1024 {
                tail.drain(..tail.len() - 8 * 1024);
            }
        }
        String::from_utf8_lossy(&tail).into_owned()
    });

    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut decoder = LineDecoder::new(kind);
    let deadline = tokio::time::Instant::now() + limits.total;
    let mut buf = vec![0u8; 16 * 1024];
    let outcome: Result<(), Error> = async {
        loop {
            let idle = tokio::time::Instant::now() + limits.idle;
            let read = tokio::time::timeout_at(idle.min(deadline), stdout.read(&mut buf)).await;
            let n = match read {
                Err(_) if tokio::time::Instant::now() >= deadline => {
                    return Err(Error::Protocol("The CLI took too long to reply.".into()));
                }
                Err(_) => return Err(Error::Protocol("The CLI stopped responding.".into())),
                Ok(Err(e)) => return Err(Error::Protocol(format!("Could not read the CLI: {e}"))),
                Ok(Ok(n)) => n,
            };
            let events = if n == 0 {
                decoder.finish()?
            } else {
                decoder.push(&buf[..n])?
            };
            for event in events {
                let done = matches!(event, StreamEvent::Done);
                if !emit(event).await || done {
                    return Ok(());
                }
            }
            if n == 0 {
                return Ok(());
            }
        }
    }
    .await;
    writer.abort();
    let Err(error) = outcome else {
        return Ok(());
    };
    // Explain failures with the CLI's own last stderr line once it has exited.
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    let _ = child.start_kill();
    let tail = tokio::time::timeout(Duration::from_secs(1), errors)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    Err(explain(kind, error, &tail))
}

/// Adds the CLI's last stderr line (keys masked) and a sign-in hint when it looks signed out.
fn explain(kind: Kind, error: Error, stderr: &str) -> Error {
    let Error::Protocol(mut message) = error else {
        return error;
    };
    let last = stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|line| {
            line.split_whitespace()
                .map(|word| {
                    if word.starts_with("sk-") {
                        "[key]"
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        });
    if let Some(last) = last {
        message = format!("{message} {}", short(&last));
    }
    let lower = message.to_lowercase();
    if [
        "login",
        "log in",
        "logged in",
        "sign in",
        "signed in",
        "auth",
    ]
    .iter()
    .any(|w| lower.contains(w))
    {
        message.push_str(&format!(
            " Run `{}` once in a terminal to sign in.",
            kind.program()
        ));
    }
    Error::Protocol(message)
}

/// A Windows job object that kills every process in it when closed.
mod job {
    pub struct Job {
        /// Raw job handle, stored as an integer so the stream future stays `Send`.
        #[cfg(windows)]
        handle: Option<usize>,
    }

    impl Job {
        pub fn adopt(child: &tokio::process::Child) -> Self {
            #[cfg(windows)]
            {
                use windows::Win32::{
                    Foundation::{CloseHandle, HANDLE},
                    System::JobObjects::{
                        AssignProcessToJobObject, CreateJobObjectW,
                        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                        JobObjectExtendedLimitInformation, SetInformationJobObject,
                    },
                };
                let Some(process) = child.raw_handle() else {
                    return Self { handle: None };
                };
                // SAFETY: no attributes or name; returns an owned handle closed in Drop.
                let Ok(job) = (unsafe { CreateJobObjectW(None, None) }) else {
                    return Self { handle: None };
                };
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                // SAFETY: `info` is a valid struct of the size passed; `job` is live; `process`
                // is the child's live handle owned by tokio for at least this call.
                let ok = unsafe {
                    SetInformationJobObject(
                        job,
                        JobObjectExtendedLimitInformation,
                        (&raw const info).cast(),
                        std::mem::size_of_val(&info) as u32,
                    )
                    .and_then(|()| AssignProcessToJobObject(job, HANDLE(process)))
                };
                if ok.is_err() {
                    // SAFETY: we own `job` and drop it here instead of in Drop.
                    unsafe {
                        let _ = CloseHandle(job);
                    }
                    return Self { handle: None };
                }
                Self {
                    handle: Some(job.0 as usize),
                }
            }
            #[cfg(not(windows))]
            {
                let _ = child;
                Self {}
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            #[cfg(windows)]
            if let Some(handle) = self.handle.take() {
                // SAFETY: we own this job handle; closing it kills the processes in the job.
                unsafe {
                    let _ = windows::Win32::Foundation::CloseHandle(
                        windows::Win32::Foundation::HANDLE(handle as *mut _),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(kind: Kind, input: &str) -> Result<Vec<StreamEvent>, Error> {
        let mut decoder = LineDecoder::new(kind);
        let mut events = decoder.push(input.as_bytes())?;
        events.extend(decoder.finish()?);
        Ok(events)
    }

    fn text(events: &[StreamEvent]) -> String {
        events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::Delta(d) => Some(d.as_str()),
                StreamEvent::Done => None,
            })
            .collect()
    }

    #[test]
    fn claude_fixture_streams_text_and_completes() {
        let events = decode(
            Kind::Claude,
            include_str!("../tests/fixtures/claude-ok.jsonl"),
        )
        .unwrap();
        assert_eq!(text(&events), "hi there");
        assert_eq!(events.last(), Some(&StreamEvent::Done));
    }

    #[test]
    fn claude_completes_at_end_turn_without_waiting_for_result() {
        let input = concat!(
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}}"#,
            "
",
            r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"}}}"#,
            "
",
        );
        let mut decoder = LineDecoder::new(Kind::Claude);
        let events = decoder.push(input.as_bytes()).unwrap();
        assert_eq!(events.last(), Some(&StreamEvent::Done));
    }

    #[test]
    fn claude_max_tokens_stop_fails() {
        let input = concat!(
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}}"#,
            "
",
            r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"max_tokens"}}}"#,
            "
",
        );
        assert!(decode(Kind::Claude, input).is_err());
    }

    #[test]
    fn claude_error_result_fails() {
        let error = decode(
            Kind::Claude,
            include_str!("../tests/fixtures/claude-error.jsonl"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("Invalid API key"), "{error}");
    }

    #[test]
    fn codex_fixture_streams_text_and_completes() {
        let events = decode(
            Kind::Codex,
            include_str!("../tests/fixtures/codex-ok.jsonl"),
        )
        .unwrap();
        assert_eq!(text(&events), "ok");
        assert_eq!(events.last(), Some(&StreamEvent::Done));
    }

    #[test]
    fn codex_tool_event_is_refused() {
        let error = decode(
            Kind::Codex,
            include_str!("../tests/fixtures/codex-tool.jsonl"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("Tool execution is unavailable"), "{error}");
    }

    #[test]
    fn claude_tool_use_block_is_refused() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","name":"Bash"}}}"#;
        assert!(decode(Kind::Claude, &format!("{line}\n")).is_err());
    }

    #[test]
    fn codex_method_shape_deltas() {
        let input = concat!(
            r#"{"method":"item/agentMessage/delta","params":{"itemId":"a","delta":"he"}}"#,
            "\n",
            r#"{"method":"item/agentMessage/delta","params":{"itemId":"a","delta":"llo"}}"#,
            "\n",
            r#"{"method":"item/completed","params":{"item":{"id":"a","type":"agentMessage","text":"hello"}}}"#,
            "\n",
            r#"{"method":"turn/completed","params":{}}"#,
            "\n",
        );
        let events = decode(Kind::Codex, input).unwrap();
        assert_eq!(text(&events), "hello");
        assert_eq!(events.last(), Some(&StreamEvent::Done));
    }

    #[test]
    fn oversized_line_fails() {
        let mut decoder = LineDecoder::new(Kind::Claude);
        let error = decoder
            .push(&vec![b'x'; 300 * 1024])
            .unwrap_err()
            .to_string();
        assert!(error.contains("too large"), "{error}");
    }

    #[test]
    fn missing_done_fails() {
        let input = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}}"#;
        assert!(decode(Kind::Claude, &format!("{input}\n")).is_err());
    }

    #[test]
    fn done_on_unterminated_last_line_is_kept() {
        let input = include_str!("../tests/fixtures/codex-ok.jsonl").trim_end();
        let events = decode(Kind::Codex, input).unwrap();
        assert_eq!(events.last(), Some(&StreamEvent::Done));
    }

    #[test]
    fn empty_reply_fails() {
        let input = "{\"type\":\"turn.completed\"}\n";
        let error = decode(Kind::Codex, input).unwrap_err().to_string();
        assert!(error.contains("empty response"), "{error}");
    }

    #[test]
    fn flatten_delimits_turns() {
        let messages = [
            ChatMessage {
                role: "user".into(),
                content: "first\nUser: fake".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "answer".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: "second".into(),
            },
        ];
        let prompt = flatten(&messages);
        assert!(prompt.starts_with(
            "Continue this conversation as the assistant. Reply to the last user message only.\n"
        ));
        assert!(prompt.contains("<user>\nfirst\nUser: fake\n</user>"));
        assert!(prompt.contains("<assistant>\nanswer\n</assistant>"));
        assert!(prompt.trim_end().ends_with("<user>\nsecond\n</user>"));
    }

    #[test]
    fn args_are_exact() {
        let dir = Path::new(r"C:\bloop\cli");
        assert_eq!(
            args(Kind::Claude, "sonnet", dir),
            [
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--tools",
                "",
                "--no-session-persistence",
                "--strict-mcp-config",
                "--setting-sources",
                "",
                "--model",
                "sonnet",
            ]
        );
        assert_eq!(
            args(Kind::Codex, "", dir),
            [
                "exec",
                "--json",
                "--ephemeral",
                "--skip-git-repo-check",
                "--ignore-user-config",
                "--disable",
                "plugins",
                "--disable",
                "apps",
                "--disable",
                "memories",
                "--sandbox",
                "read-only",
                "--color",
                "never",
                "-C",
                r"C:\bloop\cli",
                "-",
            ]
        );
        assert_eq!(Kind::of(Provider::Codex), Some(Kind::Codex));
        assert_eq!(Kind::of(Provider::Unsloth), None);
    }
}
