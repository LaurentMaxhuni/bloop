use bloop::{
    config::{Config, Provider},
    diagnose::test_connection,
};
use std::time::{Duration, Instant};
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
        endpoint: format!(
            "http://{}{}",
            listener.local_addr().unwrap(),
            if provider == Provider::Ollama {
                ""
            } else {
                "/v1"
            }
        ),
        model: "chosen".into(),
        ..Config::default()
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            let count = socket.read(&mut bytes).await.unwrap();
            assert!(count > 0 && request.len() < 16_384);
            request.extend_from_slice(&bytes[..count]);
        }
        socket.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });
    (config, task)
}

#[tokio::test]
async fn success_lists_models_with_one_get_and_no_chat_content() {
    for provider in [Provider::Ollama, Provider::OpenAi, Provider::Unsloth] {
        let body = if provider == Provider::Ollama {
            r#"{"models":[{"name":"chosen:latest"}]}"#
        } else {
            r#"{"data":[{"id":"chosen"}]}"#
        };
        let (config, task) = fixture(provider, "200 OK", body).await;
        let report = test_connection(&config).await;
        assert!(report.ok, "{report:?}");
        assert!(report.fix.is_none());
        let request = task.await.unwrap();
        assert!(request.starts_with(if provider == Provider::Ollama {
            "GET /api/tags HTTP/1.1\r\n"
        } else {
            "GET /v1/models HTTP/1.1\r\n"
        }));
        assert_eq!(request.split_once("\r\n\r\n").unwrap().1, "");
        assert!(!request.to_lowercase().contains("content-length:"));
    }
}

#[tokio::test]
async fn missing_model_names_at_most_five_available_models() {
    let (config, task) = fixture(Provider::OpenAi, "200 OK", r#"{"data":[{"id":"one"},{"id":"two"},{"id":"three"},{"id":"four"},{"id":"five"},{"id":"six"}]}"#).await;
    let report = test_connection(&config).await;
    assert!(!report.ok);
    assert!(report.summary.contains("chosen"));
    assert!(report.summary.contains("one, two, three, four, five"));
    assert!(!report.summary.contains("six"));
    assert!(report.fix.unwrap().contains("Settings"));
    task.await.unwrap();
}

#[tokio::test]
async fn authentication_and_wrong_path_have_specific_fixes() {
    for (provider, status) in [
        (Provider::OpenAi, "401 Unauthorized"),
        (Provider::Unsloth, "401 Unauthorized"),
        (Provider::Ollama, "403 Forbidden"),
        (Provider::OpenAi, "404 Not Found"),
    ] {
        let (config, task) =
            fixture(provider, status, "private error body must not be reflected").await;
        let report = test_connection(&config).await;
        assert!(!report.ok);
        assert!(!report.summary.contains("private error"));
        let fix = report.fix.unwrap();
        if status.starts_with("404") {
            assert!(fix.contains("/v1") && fix.contains("base URL"));
        } else {
            assert!(fix.contains("Settings → Model → API key") && fix.contains("BLOOP_API_KEY"));
        }
        task.await.unwrap();
    }
}

#[tokio::test]
async fn refused_port_suggests_starting_ollama() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let config = Config {
        provider: Provider::Ollama,
        endpoint: format!("http://{address}"),
        ..Config::default()
    };
    let report = test_connection(&config).await;
    assert!(!report.ok);
    assert!(report.summary.contains("Connection refused"), "{report:?}");
    assert!(report.fix.unwrap().contains("Start Ollama"));
}

#[tokio::test]
async fn silent_server_times_out_within_budget() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = Config {
        endpoint: format!("http://{}", listener.local_addr().unwrap()),
        ..Config::default()
    };
    let start = Instant::now();
    let report = test_connection(&config).await;
    assert!(!report.ok);
    assert!(report.summary.contains("timed out"), "{report:?}");
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn invalid_urls_fail_without_reflecting_credentials() {
    for endpoint in [
        "not a url",
        "ftp://example.com",
        "http://user:secret@localhost:9",
    ] {
        let config = Config {
            endpoint: endpoint.into(),
            ..Config::default()
        };
        let report = test_connection(&config).await;
        assert!(!report.ok);
        assert!(report.summary.contains("URL is invalid"));
        assert!(!format!("{report:?}").contains("secret"));
    }
}

#[tokio::test]
async fn tls_alert_explains_certificate_checks() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = Config {
        endpoint: format!("https://{}", listener.local_addr().unwrap()),
        ..Config::default()
    };
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut hello = [0; 4096];
        assert!(socket.read(&mut hello).await.unwrap() > 0);
        // TLS alert record: fatal bad_certificate, without any external TLS server.
        socket.write_all(&[21, 3, 3, 0, 2, 2, 42]).await.unwrap();
    });
    let report = test_connection(&config).await;
    task.await.unwrap();
    assert!(!report.ok);
    assert!(report.summary.contains("TLS"), "{report:?}");
    assert!(report.fix.unwrap().contains("certificate"));
}

#[tokio::test]
async fn empty_and_malformed_lists_are_not_success() {
    for body in [r#"{"data":[]}"#, "not json"] {
        let (config, task) = fixture(Provider::OpenAi, "200 OK", body).await;
        assert!(!test_connection(&config).await.ok);
        task.await.unwrap();
    }
}
