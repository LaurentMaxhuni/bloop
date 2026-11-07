use bloop::{
    config::{Config, Provider},
    model::ChatMessage,
    provider::{StreamEvent, chat, list_models},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn fixture(
    provider: Provider,
    status: &str,
    body: &str,
) -> (Config, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = Config {
        provider,
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        ..Config::default()
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        for part in response.as_bytes().chunks(7) {
            socket.write_all(part).await.unwrap();
            tokio::task::yield_now().await;
        }
        request
    });
    (config, server)
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    loop {
        let mut buf = [0; 1024];
        let n = socket.read(&mut buf).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
        assert!(bytes.len() < 65536);
        if let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..split]).to_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .map_or(0, |v| v.parse().unwrap());
            if bytes.len() >= split + 4 + length {
                return String::from_utf8(bytes).unwrap();
            }
        }
    }
}

fn messages() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".into(),
        content: "hello".into(),
    }]
}

#[tokio::test]
async fn ollama_streams_over_real_http_and_sends_expected_request() {
    let (config, server) = fixture(
        Provider::Ollama,
        "200 OK",
        "{\"message\":{\"content\":\"héllo\"},\"done\":false}\n{\"done\":true}\n",
    )
    .await;
    let mut received = Vec::new();
    chat(&config, &messages(), |event| {
        received.push(event);
        std::future::ready(true)
    })
    .await
    .unwrap();
    assert_eq!(
        received,
        [StreamEvent::Delta("héllo".into()), StreamEvent::Done]
    );
    let request = server.await.unwrap();
    assert!(request.starts_with("POST /api/chat HTTP/1.1"));
    let body: serde_json::Value =
        serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["messages"][0]["content"], "hello");
    assert_eq!(body["stream"], true);
}

#[tokio::test]
async fn openai_sse_streams_and_uses_base_path() {
    let (mut config, server) = fixture(
        Provider::OpenAi,
        "200 OK",
        "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: [DONE]\n\n",
    )
    .await;
    config.endpoint.push_str("/v1");
    let mut text = String::new();
    chat(&config, &messages(), |event| {
        if let StreamEvent::Delta(s) = event {
            text.push_str(&s);
        }
        std::future::ready(true)
    })
    .await
    .unwrap();
    assert_eq!(text, "hi");
    assert!(
        server
            .await
            .unwrap()
            .starts_with("POST /v1/chat/completions HTTP/1.1")
    );
}

#[tokio::test]
async fn http_errors_and_truncated_streams_fail_visibly() {
    for (status, body) in [
        ("401 Unauthorized", "secret remote body"),
        ("200 OK", "{\"message\":{\"content\":\"partial\"}}\n"),
    ] {
        let (config, server) = fixture(Provider::Ollama, status, body).await;
        let error = chat(&config, &messages(), |_| std::future::ready(true))
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("secret remote body"));
        assert!(!error.is_empty());
        let _ = server.await;
    }
}

#[tokio::test]
async fn cancelling_request_closes_streaming_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = Config {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        ..Config::default()
    };
    let (ready, received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        socket.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"message\":{\"content\":\"partial\"}}\n").await.unwrap();
        let _ = ready.send(());
        let mut buf = [0; 1];
        tokio::time::timeout(std::time::Duration::from_secs(2), socket.read(&mut buf))
            .await
            .unwrap()
            .unwrap()
    });
    let request =
        tokio::spawn(async move { chat(&config, &messages(), |_| std::future::ready(true)).await });
    received.await.unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    assert_eq!(server.await.unwrap(), 0);
}

#[tokio::test]
async fn ollama_lists_models() {
    let (config, server) = fixture(
        Provider::Ollama,
        "200 OK",
        r#"{"models":[{"name":"qwen3:4b"},{"name":"llama3.2"}]}"#,
    )
    .await;
    let models = list_models(&config).await.unwrap();
    assert_eq!(models, ["qwen3:4b", "llama3.2"]);
    assert!(server.await.unwrap().starts_with("GET /api/tags "));
}

#[tokio::test]
async fn openai_lists_models_under_base_path() {
    let (mut config, server) = fixture(
        Provider::OpenAi,
        "200 OK",
        r#"{"data":[{"id":"a"},{"id":"b"}]}"#,
    )
    .await;
    config.endpoint.push_str("/v1");
    let models = list_models(&config).await.unwrap();
    assert_eq!(models, ["a", "b"]);
    assert!(server.await.unwrap().starts_with("GET /v1/models "));
}

#[tokio::test]
async fn list_models_failures_are_errors() {
    let big = format!(r#"{{"models":[{{"name":"{}"}}]}}"#, "x".repeat(1_150_000));
    for (status, body, expect) in [
        ("500 Internal Server Error", "{}", "HTTP 500"),
        ("200 OK", r#"{"nope":1}"#, "unexpected model list"),
        ("200 OK", big.as_str(), "too large"),
    ] {
        let (config, server) = fixture(Provider::Ollama, status, body).await;
        let error = list_models(&config).await.unwrap_err().to_string();
        assert!(error.contains(expect), "{error}");
        let _ = server.await;
    }
}

#[tokio::test]
async fn empty_model_list_is_ok_and_empty() {
    let (config, server) = fixture(Provider::Ollama, "200 OK", r#"{"models":[]}"#).await;
    assert!(list_models(&config).await.unwrap().is_empty());
    let _ = server.await;
}
