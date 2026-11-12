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
                match self.provider.wire() {
                    Wire::Ollama if !line.is_empty() => self.frame(&line, &mut events)?,
                    Wire::OpenAi | Wire::Responses if line.is_empty() && !self.data.is_empty() => {
                        let data = std::mem::take(&mut self.data);
                        self.frame(&data, &mut events)?;
                    }
                    Wire::OpenAi | Wire::Responses if line.starts_with(b"data:") => {
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
        if self.provider.wire() == Wire::OpenAi && data == b"[DONE]" {
            self.done = true;
            events.push(StreamEvent::Done);
            return Ok(());
        }
        let value: Value = serde_json::from_slice(data)
            .map_err(|_| Error::Protocol("Provider returned an invalid JSON event.".into()))?;
        if self.provider.wire() == Wire::Responses {
            match value.get("type").and_then(Value::as_str) {
                Some("response.output_text.delta") => {
                    if let Some(delta) = value["delta"].as_str().filter(|delta| !delta.is_empty()) {
                        events.push(StreamEvent::Delta(delta.into()));
                    }
                }
                Some("response.completed") => {
                    self.done = true;
                    events.push(StreamEvent::Done);
                }
                Some("response.failed" | "response.incomplete" | "error") => {
                    return Err(chatgpt_error(None, &value, None));
                }
                _ => {}
            }
            return Ok(());
        }
        if value.get("error").is_some_and(|e| !e.is_null()) {
            // Do not reflect arbitrary remote error bodies or credentials into logs.
            return Err(Error::Protocol(
                "Provider reported an error. Check the model name and server logs.".into(),
            ));
        }
        let content = match self.provider.wire() {
            Wire::Cli | Wire::Responses => None,
            Wire::Ollama => {
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
            Wire::OpenAi => {
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
        if self.provider.wire() == Wire::Ollama
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
    if let Some(kind) = crate::cli::Kind::of(config.provider) {
        return crate::cli::chat(kind, &config.model, messages, emit).await;
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()?;
    let wire = config.provider.wire();
    let request = if wire == Wire::Responses {
        let token = crate::chatgpt::access_token()
            .await
            .map_err(Error::Protocol)?;
        let model = if config.model.trim().is_empty() {
            fetch_chatgpt_models(&token)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    Error::Protocol("No models are available for this ChatGPT account.".into())
                })?
                .slug
        } else {
            config.model.trim().to_owned()
        };
        client
            .post(format!("{}/responses", crate::chatgpt::RESOURCE))
            .bearer_auth(token)
            .json(&responses_body(&model, messages))
    } else {
        let base = validate_endpoint(&config.endpoint).map_err(Error::Protocol)?;
        let (path, body) = match wire {
            Wire::Ollama => (
                "api/chat",
                json!({"model": config.model.trim(), "messages": messages,
                "stream": true, "options": {"num_predict": 2048}}),
            ),
            Wire::OpenAi => (
                "chat/completions",
                json!({"model": config.model.trim(), "messages": messages,
                "stream": true, "max_tokens": 2048}),
            ),
            Wire::Cli | Wire::Responses => return Err(cli_on_http()),
        };
        let request = client
            .post(format!("{}/{path}", base.as_str().trim_end_matches('/')))
            .json(&body);
        match crate::secrets::api_key(&config.endpoint) {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    };
    let mut response = request.send().await?;
    if !response.status().is_success() {
        if wire == Wire::Responses {
            return Err(chatgpt_http_error(response).await);
        }
        return Err(http_error(
            config.provider,
            response.status().as_u16(),
            "Check endpoint, model, and authentication.",
        ));
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

fn responses_body(model: &str, messages: &[ChatMessage]) -> Value {
    let input: Vec<_> = messages
        .iter()
        .map(|message| {
            let role = if message.role == "system" {
                "developer"
            } else {
                message.role.as_str()
            };
            json!({"role": role, "content": message.content})
        })
        .collect();
    json!({"model": model, "input": input, "store": false, "stream": true})
}

/// Cap on a model-list response body.
const MAX_LIST: usize = 1024 * 1024;
const MAX_MODELS: usize = 500;

/// Names of the models the configured provider serves. Used by setup to auto-detect Ollama,
/// so it validates only the endpoint (the model may still be blank) and gives up after 3 s.
pub async fn list_models(config: &Config) -> Result<Vec<String>, Error> {
    if config.provider == Provider::ChatGpt {
        return Ok(chatgpt_models()
            .await?
            .into_iter()
            .map(|model| model.slug)
            .collect());
    }
    if config.provider.is_cli() {
        // CLIs pick their own default model; there is nothing to list.
        return Ok(Vec::new());
    }
    let base = validate_endpoint(&config.endpoint).map_err(Error::Protocol)?;
    let (path, list, field) = match config.provider.wire() {
        Wire::Ollama => ("api/tags", "/models", "name"),
        Wire::OpenAi => ("models", "/data", "id"),
        Wire::Cli | Wire::Responses => return Err(cli_on_http()),
    };
    let url = format!("{}/{path}", base.as_str().trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()?;
    let mut request = client.get(url);
    if let Some(key) = crate::secrets::api_key(&config.endpoint) {
        request = request.bearer_auth(key);
    }
    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(http_error(config.provider, response.status().as_u16(), ""));
    }
    let unexpected = || Error::Protocol("Provider returned an unexpected model list.".into());
    let value = read_json(response, "Model list").await?;
    let items = value
        .pointer(list)
        .and_then(Value::as_array)
        .ok_or_else(unexpected)?;
    let mut names: Vec<String> = Vec::new();
    for name in items.iter().filter_map(|item| item.get(field)?.as_str()) {
        let name = name.trim();
        if !name.is_empty() && !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
            if names.len() == MAX_MODELS {
                break;
            }
        }
    }
    Ok(names)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatGptModel {
    pub slug: String,
    pub display_name: String,
}

impl std::fmt::Display for ChatGptModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display_name)
    }
}

pub async fn chatgpt_models() -> Result<Vec<ChatGptModel>, Error> {
    let token = crate::chatgpt::access_token()
        .await
        .map_err(Error::Protocol)?;
    fetch_chatgpt_models(&token).await
}

async fn fetch_chatgpt_models(token: &str) -> Result<Vec<ChatGptModel>, Error> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()?;
    let response = client
        .get(format!("{}/models", crate::chatgpt::RESOURCE))
        .bearer_auth(token)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(chatgpt_http_error(response).await);
    }
    let value = read_json(response, "Model list").await?;
    let items = value["models"]
        .as_array()
        .ok_or_else(|| Error::Protocol("ChatGPT returned an unexpected model list.".into()))?;
    let mut models = Vec::new();
    for item in items.iter().filter(|item| item["visibility"] == "list") {
        let Some(slug) = item["slug"]
            .as_str()
            .filter(|slug| !slug.is_empty() && slug.len() <= 200)
        else {
            continue;
        };
        if models.iter().any(|model: &ChatGptModel| model.slug == slug) {
            continue;
        }
        models.push(ChatGptModel {
            slug: slug.into(),
            display_name: item["display_name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .unwrap_or(slug)
                .into(),
        });
        if models.len() == MAX_MODELS {
            break;
        }
    }
    Ok(models)
}

async fn read_json(mut response: reqwest::Response, description: &str) -> Result<Value, Error> {
    let too_large = || Error::Protocol(format!("{description} is too large."));
    if response
        .content_length()
        .is_some_and(|length| length > MAX_LIST as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > MAX_LIST {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map_err(|_| Error::Protocol("Provider returned an unexpected JSON response.".into()))
}

async fn chatgpt_http_error(response: reqwest::Response) -> Error {
    let status = response.status().as_u16();
    let request_id = response
        .headers()
        .get("x-request-id")
        .or_else(|| response.headers().get("openai-request-id"))
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            value.len() <= 100
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .map(str::to_owned);
    let body = read_json(response, "Provider response")
        .await
        .unwrap_or(Value::Null);
    chatgpt_error(Some(status), &body, request_id.as_deref())
}

fn chatgpt_error(status: Option<u16>, body: &Value, request_id: Option<&str>) -> Error {
    let code = body
        .pointer("/response/error/code")
        .or_else(|| body.pointer("/error/code"))
        .or_else(|| body.get("code"))
        .and_then(Value::as_str);
    let hint = match (status, code) {
        (Some(429), _) | (_, Some("subscription_sharing_usage_limit_exceeded")) => {
            "Usage limit reached. Choose Manage usage in Settings → Model."
        }
        (_, Some("subscription_sharing_user_not_eligible")) => {
            "ChatGPT plan usage is unavailable for this account or workspace."
        }
        (Some(401), _) | (_, Some("subscription_sharing_invalid_user")) => {
            "Reconnect your ChatGPT account in Settings → Model."
        }
        (Some(403), _) => "ChatGPT denied access for this account or workspace.",
        (Some(503), _) | (_, Some("subscription_sharing_usage_unavailable")) => {
            "ChatGPT plan usage is temporarily unavailable. Try again later."
        }
        _ => "ChatGPT could not complete the request.",
    };
    let http_status = status;
    let status = status
        .map(|status| format!(" (HTTP {status})"))
        .unwrap_or_default();
    let request = request_id
        .map(|id| format!(" Request ID: {id}."))
        .unwrap_or_default();
    let message = format!("{hint}{status}{request}");
    match http_status {
        Some(status) => Error::Http { status, message },
        None => Error::Protocol(message),
    }
}

/// A failed HTTP status, explained for the provider that returned it.
fn http_error(provider: Provider, status: u16, hint: &str) -> Error {
    if provider == Provider::Unsloth && status == 401 {
        return Error::Http {
            status,
            message: "Unsloth Studio rejected the key. Copy the key it printed when the model loaded into Settings.".into(),
        };
    }
    Error::Http {
        status,
        message: format!("Provider returned HTTP {status}. {hint}")
            .trim_end()
            .into(),
    }
}

/// CLI providers are routed before reaching the HTTP code (Task 5 wires them up).
fn cli_on_http() -> Error {
    Error::Protocol("This provider runs a local CLI, not an HTTP endpoint.".into())
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

    #[test]
    fn chatgpt_uses_stateless_responses_with_developer_context() {
        let body = responses_body(
            "chosen-model",
            &crate::model::context(&[], "Hello", "Be brief."),
        );
        assert_eq!(body["model"], "chosen-model");
        assert_eq!(body["input"][0]["role"], "developer");
        assert!(
            body["input"][0]["content"]
                .as_str()
                .unwrap()
                .ends_with("<instructions>\nBe brief.\n</instructions>")
        );
        assert_eq!(body["input"][1], json!({"role":"user", "content":"Hello"}));
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn chatgpt_stream_requires_a_completed_event() {
        let mut decoder = Decoder::new(Provider::ChatGpt);
        assert_eq!(
            decoder
                .push(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hi\"}\r\n\r\n")
                .unwrap(),
            [StreamEvent::Delta("Hi".into())]
        );
        assert!(decoder.finish().is_err());
        assert_eq!(
            decoder
                .push(b"data: {\"type\":\"response.completed\"}\n\n")
                .unwrap(),
            [StreamEvent::Done]
        );
        let mut decoder = Decoder::new(Provider::ChatGpt);
        let error = decoder.push(b"data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\"}}}\n\n").unwrap_err();
        assert!(error.to_string().contains("Manage usage"));
        assert!(decoder.finish().is_err());
    }
}
use crate::{
    config::{Config, Provider, Wire, validate_endpoint},
    model::{ChatMessage, MAX_OUTPUT},
};
use serde_json::{Value, json};
use std::time::Duration;

const MAX_FRAME: usize = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Protocol(String),
    #[error("{message}")]
    Http { status: u16, message: String },
    #[error("Provider connection failed: {0}")]
    Network(#[source] reqwest::Error),
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error.without_url())
    }
}
