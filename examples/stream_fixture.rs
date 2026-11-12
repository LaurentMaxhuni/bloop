//! Deterministic UI verification server. No inference; never use for real answers.
use bloop::{config::Config, storage};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    time::Duration,
};

fn respond(mut socket: TcpStream) -> std::io::Result<()> {
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut request = Vec::new();
    loop {
        let mut buf = [0; 1024];
        let n = socket.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        request.extend_from_slice(&buf[..n]);
        if request.len() > 65536 {
            return Ok(());
        }
        if let Some(index) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&request[..index]).to_lowercase();
            let length = headers
                .lines()
                .find_map(|s| s.strip_prefix("content-length: "))
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            if request.len() >= index + 4 + length {
                break;
            }
        }
    }
    socket.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n",
    )?;
    for word in "This is a local streaming fixture. Your native island is receiving tokens, without an inference server. Try Stop to verify cancellation.".split_inclusive(' ') {
        let line = serde_json::json!({"message":{"content":word},"done":false});
        writeln!(socket, "{line}")?;
        socket.flush()?;
        std::thread::sleep(Duration::from_millis(750));
    }
    writeln!(socket, "{{\"done\":true}}")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Require isolated data so this helper cannot overwrite normal app settings.
    if std::env::var_os("BLOOP_DATA_DIR").is_none() {
        return Err("Set BLOOP_DATA_DIR to an isolated test directory first.".into());
    }
    let listener = TcpListener::bind("127.0.0.1:11634")?;
    let config = Config {
        endpoint: "http://127.0.0.1:11634".into(),
        model: "bloop-test-fixture".into(),
        shortcut: "Ctrl+Alt+Shift+B".into(),
        ..Config::default()
    };
    let path = storage::data_path()?;
    // Saved chats stay as they are; only the settings point at this fixture.
    storage::save(&path, &config, None)?;
    println!("Streaming fixture ready on 127.0.0.1:11634. Not a model.");
    for socket in listener.incoming() {
        let socket = socket?;
        std::thread::spawn(move || {
            let _ = respond(socket);
        });
    }
    Ok(())
}
