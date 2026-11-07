//! Update checking and downloading against a local fake GitHub.

use bloop::update::{self, Release, Version};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Route {
    status: &'static str,
    body: Vec<u8>,
    location: Option<String>,
}

fn ok(body: impl Into<Vec<u8>>) -> Route {
    Route {
        status: "200 OK",
        body: body.into(),
        location: None,
    }
}

/// Serves `routes` by path until the test ends; returns the base URL.
async fn serve(routes: HashMap<&'static str, Route>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes = Arc::new(routes);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut buf = [0u8; 1024];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    head.extend_from_slice(&buf[..n]);
                }
                let line = String::from_utf8_lossy(&head);
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let missing = Route {
                    status: "404 Not Found",
                    body: b"missing".to_vec(),
                    location: None,
                };
                let route = routes.get(path.as_str()).unwrap_or(&missing);
                let mut response = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    route.status,
                    route.body.len()
                );
                if let Some(location) = &route.location {
                    response.push_str(&format!("Location: {location}\r\n"));
                }
                response.push_str("\r\n");
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.write_all(&route.body).await;
            });
        }
    });
    base
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bloop-update-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

fn release_json(base: &str, tag: &str, size: usize) -> String {
    format!(
        r#"{{"tag_name":"{tag}","draft":false,"prerelease":false,"assets":[
        {{"name":"bloop-setup-{v}.exe","size":{size},"browser_download_url":"{base}/exe"}},
        {{"name":"bloop-setup-{v}.exe.sha256","size":90,"browser_download_url":"{base}/sha"}}]}}"#,
        v = tag.trim_start_matches('v')
    )
}

fn sha_line(bytes: &[u8], name: &str) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    format!("{}  {name}\n", update::hex(&digest))
}

fn release(base: &str, version: &str, size: u64) -> Release {
    Release {
        version: v(version),
        installer_url: format!("{base}/exe"),
        checksum_url: format!("{base}/sha"),
        size,
    }
}

#[tokio::test]
async fn latest_finds_newer_release() {
    let base = serve(HashMap::from([(
        "/latest",
        ok(release_json("http://x", "v0.9.0", 10)),
    )]))
    .await;
    let found = update::latest_from(&format!("{base}/latest"), &v("0.2.0"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.version, v("0.9.0"));
}

#[tokio::test]
async fn latest_none_when_current() {
    let base = serve(HashMap::from([(
        "/latest",
        ok(release_json("http://x", "v0.2.0", 10)),
    )]))
    .await;
    assert!(
        update::latest_from(&format!("{base}/latest"), &v("0.2.0"))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn rate_limit_or_html_is_an_error_not_a_panic() {
    let base = serve(HashMap::from([
        (
            "/limited",
            Route {
                status: "403 Forbidden",
                body: b"{\"message\":\"API rate limit exceeded\"}".to_vec(),
                location: None,
            },
        ),
        ("/html", ok("<html>hello</html>")),
    ]))
    .await;
    for path in ["/limited", "/html"] {
        let error = update::latest_from(&format!("{base}{path}"), &v("0.2.0"))
            .await
            .unwrap_err();
        assert!(!error.to_string().is_empty());
    }
}

#[tokio::test]
async fn download_verifies_and_renames() {
    let payload = vec![7u8; 300_000];
    let name = "bloop-setup-0.9.0.exe";
    let base = serve(HashMap::from([
        ("/exe", ok(payload.clone())),
        ("/sha", ok(sha_line(&payload, name))),
    ]))
    .await;
    let dir = temp_dir("ok");
    let mut last = 0;
    let path = update::download_from(&release(&base, "0.9.0", 300_000), &dir, |p| last = p)
        .await
        .unwrap();
    assert_eq!(path, dir.join(name));
    assert_eq!(std::fs::read(&path).unwrap(), payload);
    assert!(!dir.join(format!("{name}.part")).exists());
    assert_eq!(last, 100);
    assert_eq!(update::ready(&dir, &v("0.2.0")), Some((v("0.9.0"), path)));
}

#[tokio::test]
async fn checksum_mismatch_deletes_file() {
    let name = "bloop-setup-0.9.0.exe";
    let base = serve(HashMap::from([
        ("/exe", ok(vec![1u8; 1000])),
        ("/sha", ok(sha_line(b"something else", name))),
    ]))
    .await;
    let dir = temp_dir("mismatch");
    let error = update::download_from(&release(&base, "0.9.0", 1000), &dir, |_| {})
        .await
        .unwrap_err();
    assert!(error.to_string().contains("checksum"), "{error}");
    assert!(!dir.join(name).exists());
    assert!(!dir.join(format!("{name}.part")).exists());
}

#[tokio::test]
async fn size_overrun_is_rejected() {
    let payload = vec![2u8; 5000];
    let name = "bloop-setup-0.9.0.exe";
    let base = serve(HashMap::from([
        ("/exe", ok(payload.clone())),
        ("/sha", ok(sha_line(&payload, name))),
    ]))
    .await;
    let dir = temp_dir("overrun");
    // The release claims 1000 bytes but the server sends 5000.
    assert!(
        update::download_from(&release(&base, "0.9.0", 1000), &dir, |_| {})
            .await
            .is_err()
    );
    assert!(!dir.join(name).exists());
}

#[tokio::test]
async fn redirect_to_other_host_refused() {
    let base = serve(HashMap::from([
        (
            "/exe",
            Route {
                status: "302 Found",
                body: Vec::new(),
                location: Some("http://example.com/evil.exe".into()),
            },
        ),
        ("/sha", ok(sha_line(b"x", "bloop-setup-0.9.0.exe"))),
    ]))
    .await;
    let dir = temp_dir("redirect");
    assert!(
        update::download_from(&release(&base, "0.9.0", 1), &dir, |_| {})
            .await
            .is_err()
    );
}

#[test]
fn ready_ignores_part_and_older() {
    let dir = temp_dir("ready");
    std::fs::create_dir_all(&dir).unwrap();
    let put = |version: &str, bytes: &[u8], part: bool| {
        let name = format!("bloop-setup-{version}.exe");
        std::fs::write(dir.join(format!("{name}.sha256")), sha_line(bytes, &name)).unwrap();
        let file = if part { format!("{name}.part") } else { name };
        std::fs::write(dir.join(file), bytes).unwrap();
    };
    put("0.3.0", b"old", false);
    put("0.5.0", b"newer", false);
    put("0.9.0", b"unfinished", true);
    let (version, path) = update::ready(&dir, &v("0.2.0")).unwrap();
    assert_eq!(version, v("0.5.0"));
    assert!(path.ends_with("bloop-setup-0.5.0.exe"));
    // A tampered file no longer counts.
    std::fs::write(&path, b"tampered").unwrap();
    assert_eq!(
        update::ready(&dir, &v("0.2.0")).map(|(v, _)| v),
        Some(v("0.3.0"))
    );
    assert!(update::ready(&dir, &v("0.5.0")).is_none());
}

#[tokio::test]
async fn no_releases_yet_is_up_to_date() {
    // GitHub answers /releases/latest with 404 while a repo has no releases.
    let base = serve(HashMap::new()).await;
    assert_eq!(
        update::latest_from(&format!("{base}/latest"), &v("0.2.0"))
            .await
            .unwrap(),
        None
    );
}

/// Real GitHub check (network): `cargo test --test update -- --ignored`.
#[tokio::test]
#[ignore = "talks to api.github.com"]
async fn real_github_latest_does_not_error() {
    let result = update::latest(&Version::current()).await;
    assert!(result.is_ok(), "{result:?}");
}
