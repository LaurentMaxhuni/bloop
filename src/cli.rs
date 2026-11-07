//! Claude Code and Codex CLIs as chat-only providers: argument lists, the prompt sent on
//! stdin, and decoding their JSONL output into [`StreamEvent`]s.

use crate::{
    config::Provider,
    model::{ChatMessage, MAX_OUTPUT},
    provider::{Error, StreamEvent},
};
use serde_json::Value;
use std::path::Path;

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
