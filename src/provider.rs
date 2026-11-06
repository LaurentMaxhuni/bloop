#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Delta(String),
    Done,
}

struct Decoder {
    provider: Provider,
    pending: Vec<u8>,
    data: Vec<u8>,
    done: bool,
}
impl Decoder {
    fn new(provider: Provider) -> Self {
        Self {
            provider,
            pending: Vec::new(),
            data: Vec::new(),
            done: false,
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Result<Vec<StreamEvent>, Error> {
        let mut events = Vec::new();
        // Process bytes incrementally so a large HTTP chunk cannot bypass frame limits.
        for byte in bytes {
            if self.done {
                break;
            }
            if *byte == b'\n' {
                let mut line = std::mem::take(&mut self.pending);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                match self.provider {
                    Provider::Ollama if !line.is_empty() => self.frame(&line, &mut events)?,
                    Provider::OpenAi if line.is_empty() && !self.data.is_empty() => {
                        let data = std::mem::take(&mut self.data);
                        self.frame(&data, &mut events)?;
                    }
                    Provider::OpenAi if line.starts_with(b"data:") => {
                        let data = line[5..].strip_prefix(b" ").unwrap_or(&line[5..]);
                        if self.data.len() + data.len() + 1 > MAX_FRAME {
                            return Err(Error::Protocol("Provider event is too large.".into()));
                        }
                        if !self.data.is_empty() {
                            self.data.push(b'\n');
                        }
                        self.data.extend_from_slice(data);
                    }
                    _ => {}
                }
            } else {
                if self.pending.len() >= MAX_FRAME {
                    return Err(Error::Protocol("Provider line is too large.".into()));
                }
                self.pending.push(*byte);
            }
        }
        Ok(events)
    }

    fn frame(&mut self, data: &[u8], events: &mut Vec<StreamEvent>) -> Result<(), Error> {
        if self.provider == Provider::OpenAi && data == b"[DONE]" {
            self.done = true;
            events.push(StreamEvent::Done);
            return Ok(());
        }
        let value: Value = serde_json::from_slice(data)
            .map_err(|_| Error::Protocol("Provider returned an invalid JSON event.".into()))?;
        if value.get("error").is_some_and(|e| !e.is_null()) {
            // Do not reflect arbitrary remote error bodies or credentials into logs.
            return Err(Error::Protocol(
                "Provider reported an error. Check the model name and server logs.".into(),
            ));
        }
        let content = match self.provider {
            Provider::Ollama => {
                if value
                    .get("done_reason")
                    .and_then(Value::as_str)
                    .is_some_and(|r| r != "stop")
                {
                    return Err(Error::Protocol(
                        "Model stopped before completing its reply. Ask for a shorter answer."
                            .into(),
                    ));
                }
                if value
                    .pointer("/message/tool_calls")
                    .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty()))
                {
                    return Err(Error::Protocol(
                        "Tool execution is unavailable in this milestone.".into(),
                    ));
                }
                value.pointer("/message/content").and_then(Value::as_str)
            }
            Provider::OpenAi => {
                let reason = value
                    .pointer("/choices/0/finish_reason")
                    .and_then(Value::as_str);
                if value
                    .pointer("/choices/0/delta/tool_calls")
                    .is_some_and(|v| !v.is_null() && v.as_array().is_none_or(|a| !a.is_empty()))
                    || value
                        .pointer("/choices/0/delta/function_call")
                        .is_some_and(|v| !v.is_null())
                    || reason.is_some_and(|r| r != "stop")
                {
                    return Err(Error::Protocol(
                        "Response interrupted or requested unsupported tools.".into(),
                    ));
                }
                value
                    .pointer("/choices/0/delta/content")
                    .and_then(Value::as_str)
            }
        };
        if let Some(content) = content.filter(|s| !s.is_empty()) {
            events.push(StreamEvent::Delta(content.into()));
        }
        if self.provider == Provider::Ollama
            && value.get("done").and_then(Value::as_bool) == Some(true)
        {
            self.done = true;
            events.push(StreamEvent::Done);
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<Vec<StreamEvent>, Error> {
        let events = self.push(b"\n\n")?;
        if !self.done {
            return Err(Error::Protocol(
                "Connection closed before the response finished.".into(),
            ));
        }
        Ok(events)
    }
}

pub async fn chat<F: Future<Output = bool> + Send>(
    config: &Config,
    messages: &[ChatMessage],
    mut emit: impl FnMut(StreamEvent) -> F + Send,
) -> Result<(), Error> {
    config.validate().map_err(Error::Protocol)?;
    let base = validate_endpoint(&config.endpoint).map_err(Error::Protocol)?;
    let path = match config.provider {
        Provider::Ollama => "api/chat",
        Provider::OpenAi => "chat/completions",
    };
    let url = format!("{}/{path}", base.as_str().trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()?;
    let body = match config.provider {
        Provider::Ollama => {
            json!({"model": config.model.trim(), "messages": messages, "stream": true,
            "options": {"num_predict": 2048}})
        }
        Provider::OpenAi => {
            json!({"model": config.model.trim(), "messages": messages, "stream": true, "max_tokens": 2048})
        }
    };
    let mut request = client.post(url).json(&body);
    if let Ok(key) = std::env::var("BLOOP_API_KEY") {
        request = request.bearer_auth(key);
    }
    let mut response = request.send().await?;
    if !response.status().is_success() {
        return Err(Error::Protocol(format!(
            "Provider returned HTTP {}. Check endpoint, model, and authentication.",
            response.status().as_u16()
        )));
    }
    let mut decoder = Decoder::new(config.provider);
    let mut output_size = 0_usize;
    loop {
        let events = match response.chunk().await? {
            Some(chunk) => decoder.push(&chunk)?,
            None => decoder.finish()?,
        };
        for event in events {
            if let StreamEvent::Delta(ref delta) = event {
                output_size += delta.len();
                if output_size > MAX_OUTPUT {
                    return Err(Error::Protocol(
                        "Response exceeded 128 KiB. Ask for a shorter answer.".into(),
                    ));
                }
            }
            let done = matches!(event, StreamEvent::Done);
            if done && output_size == 0 {
                return Err(Error::Protocol(
                    "Model returned an empty response. Try another model or prompt.".into(),
                ));
            }
            if !emit(event).await || done {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_frames_and_unfinished_streams() {
        let mut decoder = Decoder::new(Provider::Ollama);
        assert!(decoder.push(&vec![b'x'; MAX_FRAME + 1]).is_err());
        let mut decoder = Decoder::new(Provider::Ollama);
        decoder
            .push(b"{\"message\":{\"content\":\"partial\"}}\n")
            .unwrap();
        assert!(decoder.finish().is_err());
    }

    #[test]
    fn refuses_tool_calls_and_token_limit_completions() {
        let mut decoder = Decoder::new(Provider::OpenAi);
        assert!(
            decoder
                .push(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{}]}}]}\n\n")
                .is_err()
        );
        let mut decoder = Decoder::new(Provider::Ollama);
        assert!(
            decoder
                .push(b"{\"done\":true,\"done_reason\":\"length\"}\n")
                .is_err()
        );
    }

    #[test]
    fn nullable_tool_fields_do_not_block_text_streams() {
        let mut decoder = Decoder::new(Provider::OpenAi);
        let events = decoder.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\",\"tool_calls\":null,\"function_call\":null}}]}\n\n").unwrap();
        assert_eq!(events, [StreamEvent::Delta("hello".into())]);
    }

    #[test]
    fn fragmented_ndjson_preserves_utf8() {
        let wire = "{\"message\":{\"content\":\"héllo\"},\"done\":false}\n{\"done\":true}\n";
        let mut decoder = Decoder::new(crate::config::Provider::Ollama);
        let mut text = String::new();
        let mut done = false;
        for byte in wire.as_bytes() {
            for event in decoder.push(&[*byte]).unwrap() {
                match event {
                    StreamEvent::Delta(s) => text.push_str(&s),
                    StreamEvent::Done => done = true,
                }
            }
        }
        assert_eq!(text, "héllo");
        assert!(done);
    }

    #[test]
    fn sse_comments_crlf_and_done() {
        let mut decoder = Decoder::new(crate::config::Provider::OpenAi);
        let events = decoder.push(b": heartbeat\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r\n\r\ndata: [DONE]\r\n\r\n").unwrap();
        assert_eq!(
            events,
            vec![StreamEvent::Delta("hi".into()), StreamEvent::Done]
        );
    }

    #[test]
    fn provider_error_is_not_success() {
        let mut decoder = Decoder::new(crate::config::Provider::Ollama);
        assert!(decoder.push(b"{\"error\":\"model missing\"}\n").is_err());
    }
}
use crate::{
    config::{Config, Provider, validate_endpoint},
    model::{ChatMessage, MAX_OUTPUT},
};
use serde_json::{Value, json};
use std::time::Duration;

const MAX_FRAME: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Protocol(String),
    #[error("Provider connection failed: {0}")]
    Network(String),
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error.without_url().to_string())
    }
}
