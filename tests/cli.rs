//! Runs `cli::stream` against `examples/fake_cli.rs` to prove stdin delivery, streaming,
//! deadlines, and that cancelling kills the whole process tree.
#![cfg(windows)]

use bloop::{
    cli::{self, Kind, Limits},
    provider::StreamEvent,
};
use std::{
    path::{Path, PathBuf},
    sync::Once,
    time::Duration,
};

fn fake() -> PathBuf {
    static BUILD: Once = Once::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    BUILD.call_once(|| {
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--quiet", "--example", "fake_cli"])
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success());
    });
    root.join("target/debug/examples/fake_cli.exe")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A scratch dir per test, with environment for the fake.
struct Scene {
    dir: PathBuf,
}

impl Scene {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("bloop-cli-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn read_pid(&self, name: &str) -> u32 {
        for _ in 0..100 {
            if let Ok(text) = std::fs::read_to_string(self.file(name))
                && let Ok(pid) = text.trim().parse()
            {
                return pid;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("{name} never written");
    }
}

fn alive(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
}

fn wait_dead(pid: u32) -> bool {
    (0..40).any(|_| {
        std::thread::sleep(Duration::from_millis(50));
        !alive(pid)
    })
}

#[tokio::test]
async fn stdin_receives_prompt_and_reply_streams() {
    let scene = Scene::new("stream");
    let env = [
        ("FAKE_STDIN", scene.file("stdin.txt")),
        ("FAKE_FIXTURE", fixture("claude-ok.jsonl")),
    ];
    let mut text = String::new();
    cli::stream_with(
        cli::Request {
            kind: Kind::Claude,
            program: &fake(),
            args: &["-p".into()],
            prompt: "hello prompt",
            model: "",
            dir: None,
        },
        &env,
        Limits::default(),
        |event| {
            if let StreamEvent::Delta(d) = event {
                text.push_str(&d);
            }
            std::future::ready(true)
        },
    )
    .await
    .unwrap();
    assert_eq!(text, "hi there");
    assert_eq!(
        std::fs::read_to_string(scene.file("stdin.txt")).unwrap(),
        "hello prompt"
    );
}

/// The app's runtime caps its blocking pool, which tokio uses for child pipes on Windows;
/// the CLI must still get its prompt with stdout and stderr reads already parked there.
fn stdin_arrives_with_blocking_threads(threads: usize) -> Result<String, String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(threads)
        .enable_all()
        .build()
        .unwrap();
    let scene = Scene::new(&format!("pool{threads}"));
    let env = [
        ("FAKE_STDIN", scene.file("stdin.txt")),
        ("FAKE_FIXTURE", fixture("claude-ok.jsonl")),
    ];
    let limits = Limits {
        idle: Duration::from_secs(4),
        total: Duration::from_secs(8),
    };
    runtime.block_on(async {
        let mut text = String::new();
        cli::stream_with(
            cli::Request {
                kind: Kind::Claude,
                program: &fake(),
                args: &[],
                prompt: "hello prompt",
                model: "",
                dir: None,
            },
            &env,
            limits,
            |event| {
                if let StreamEvent::Delta(d) = event {
                    text.push_str(&d);
                }
                std::future::ready(true)
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(text)
    })
}

#[test]
fn prompt_arrives_on_the_apps_runtime() {
    assert_eq!(
        stdin_arrives_with_blocking_threads(cli::BLOCKING_THREADS).as_deref(),
        Ok("hi there")
    );
}

async fn cancel_kills_tree(program: PathBuf, scene: &Scene) {
    let env = [
        ("FAKE_PID", scene.file("pid")),
        ("FAKE_CHILD_PID", scene.file("child")),
        ("FAKE_FIXTURE", fixture("claude-ok.jsonl")),
        ("FAKE_DELAY_MS", "200".into()),
        ("FAKE_HANG", "1".into()),
    ];
    let (seen, mut first) = tokio::sync::mpsc::unbounded_channel();
    let task = tokio::spawn(async move {
        let _ = cli::stream_with(
            cli::Request {
                kind: Kind::Claude,
                program: &program,
                args: &[],
                prompt: "x",
                model: "",
                dir: None,
            },
            &env,
            Limits::default(),
            move |event| {
                let _ = seen.send(event);
                std::future::ready(true)
            },
        )
        .await;
    });
    first.recv().await.unwrap();
    let (pid, child) = (scene.read_pid("pid"), scene.read_pid("child"));
    assert!(alive(pid) && alive(child));
    task.abort();
    let _ = task.await;
    assert!(wait_dead(pid), "CLI process survived cancel");
    assert!(wait_dead(child), "CLI's child process survived cancel");
}

#[tokio::test]
async fn cancel_kills_process_tree() {
    let scene = Scene::new("cancel");
    cancel_kills_tree(fake(), &scene).await;
}

#[tokio::test]
async fn cmd_shim_runs_and_is_killed() {
    let scene = Scene::new("shim");
    let shim = scene.file("fake.cmd");
    std::fs::write(&shim, format!("@\"{}\" %*\r\n", fake().display())).unwrap();
    cancel_kills_tree(shim, &scene).await;
}

#[tokio::test]
async fn idle_deadline_fails() {
    let env = [("FAKE_HANG", PathBuf::from("1"))];
    let limits = Limits {
        idle: Duration::from_millis(400),
        total: Duration::from_secs(10),
    };
    let error = cli::stream_with(
        cli::Request {
            kind: Kind::Codex,
            program: &fake(),
            args: &[],
            prompt: "x",
            model: "",
            dir: None,
        },
        &env,
        limits,
        |_| std::future::ready(true),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("stopped responding"), "{error}");
}

#[tokio::test]
async fn signed_out_cli_gets_a_hint() {
    let env = [
        (
            "FAKE_STDERR",
            PathBuf::from("Error: not logged in. Please run /login"),
        ),
        ("FAKE_EXIT", PathBuf::from("1")),
    ];
    let error = cli::stream_with(
        cli::Request {
            kind: Kind::Claude,
            program: &fake(),
            args: &[],
            prompt: "x",
            model: "",
            dir: None,
        },
        &env,
        Limits::default(),
        |_| std::future::ready(true),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("not logged in"), "{error}");
    assert!(
        error.contains("Run `claude` once in a terminal to sign in."),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn find_in_needs_execute_bit() {
    use std::os::unix::fs::PermissionsExt;
    let scene = Scene::new("find-unix");
    let (a, b) = (scene.file("a"), scene.file("b"));
    std::fs::create_dir_all(a.join("codex")).unwrap(); // a directory, not a program
    std::fs::create_dir_all(&b).unwrap();
    let path = std::env::join_paths([&a, &b]).unwrap();
    std::fs::write(
        b.join("codex"),
        "#!/bin/sh
",
    )
    .unwrap();
    std::fs::set_permissions(b.join("codex"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(cli::find_in("codex", &path), None, "not executable");
    std::fs::set_permissions(b.join("codex"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(cli::find_in("codex", &path), Some(b.join("codex")));
    assert_eq!(cli::find_in("claude", &path), None);
}

#[cfg(windows)]
#[test]
fn find_in_prefers_exe_then_cmd() {
    let scene = Scene::new("find");
    let (a, b) = (scene.file("a"), scene.file("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(b.join("codex.cmd"), "").unwrap();
    std::fs::write(b.join("codex.ps1"), "").unwrap();
    let path = std::env::join_paths([&a, &b]).unwrap();
    assert_eq!(cli::find_in("codex", &path), Some(b.join("codex.cmd")));
    std::fs::write(b.join("codex.exe"), "").unwrap();
    assert_eq!(cli::find_in("codex", &path), Some(b.join("codex.exe")));
    assert_eq!(cli::find_in("claude", &path), None);
}

/// Real-CLI check (spends a little quota): `cargo test --test cli -- --ignored`.
#[tokio::test]
#[ignore = "runs the installed Codex CLI"]
async fn real_codex_cancel_kills_tree() {
    let program = cli::find(Kind::Codex).expect("codex installed");
    let dir = std::env::temp_dir().join("bloop-real-codex");
    std::fs::create_dir_all(&dir).unwrap();
    let args = cli::args(Kind::Codex, "", &dir);
    let env: [(&str, PathBuf); 0] = [];
    let task = tokio::spawn(async move {
        let _ = cli::stream_with(
            cli::Request {
                kind: Kind::Codex,
                program: &program,
                args: &args,
                prompt: "Write the numbers 1 to 2000, one per line.",
                model: "",
                dir: None,
            },
            &env,
            Limits::default(),
            |_| std::future::ready(true),
        )
        .await;
    });
    tokio::time::sleep(Duration::from_secs(6)).await;
    let me = std::process::id();
    let children = |root: u32| {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_Process | Where-Object { $_.Name -in 'codex.exe','node.exe','cmd.exe' } | ForEach-Object { \"$($_.ProcessId) $($_.ParentProcessId)\" }",
            ])
            .output()
            .unwrap();
        let pairs: Vec<(u32, u32)> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace().map(|v| v.parse().ok());
                Some((it.next()??, it.next()??))
            })
            .collect();
        let mut tree = vec![root];
        let mut i = 0;
        while i < tree.len() {
            let parent = tree[i];
            tree.extend(pairs.iter().filter(|(_, p)| *p == parent).map(|(c, _)| *c));
            i += 1;
        }
        tree.split_off(1)
    };
    let running = children(me);
    assert!(!running.is_empty(), "codex never started");
    task.abort();
    let _ = task.await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let alive: Vec<u32> = running.into_iter().filter(|pid| alive(*pid)).collect();
    assert!(alive.is_empty(), "still running after cancel: {alive:?}");
}

#[tokio::test]
async fn refused_tool_kills_the_cli_at_once() {
    let scene = Scene::new("refuse");
    let marker = scene.file("marker");
    let env = [
        ("FAKE_FIXTURE", fixture("codex-tool.jsonl")),
        ("FAKE_MARKER", marker.clone()),
        ("FAKE_AFTER_MS", "500".into()),
    ];
    let error = cli::stream_with(
        cli::Request {
            kind: Kind::Codex,
            program: &fake(),
            args: &[],
            prompt: "x",
            model: "",
            dir: None,
        },
        &env,
        Limits::default(),
        |_| std::future::ready(true),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("Tool execution is unavailable"), "{error}");
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(
        !marker.exists(),
        "the CLI kept running after its tool event was refused"
    );
}

/// Real-CLI check (spends a little quota): `cargo test --test cli -- --ignored`.
#[tokio::test]
#[ignore = "runs the installed Codex CLI"]
async fn real_codex_replies_through_the_allowlist() {
    let program = cli::find(Kind::Codex).expect("codex installed");
    let dir = std::env::temp_dir().join("bloop-real-codex");
    std::fs::create_dir_all(&dir).unwrap();
    let env: [(&str, PathBuf); 0] = [];
    let mut text = String::new();
    let mut pieces = 0;
    cli::stream_with(
        cli::Request {
            kind: Kind::Codex,
            program: &program,
            args: &cli::args(Kind::Codex, "", &dir),
            prompt: "Think it through, then answer in two short sentences: why is the sky blue?",
            model: "",
            dir: Some(&dir),
        },
        &env,
        Limits::default(),
        |event| {
            if let StreamEvent::Delta(d) = event {
                text.push_str(&d);
                pieces += 1;
            }
            std::future::ready(true)
        },
    )
    .await
    .unwrap();
    assert!(!text.trim().is_empty());
    assert!(pieces > 1, "the reply arrived in one piece: not streamed");
}

/// Real-CLI check (spends a little quota): `cargo test --test cli -- --ignored`.
#[tokio::test]
#[ignore = "runs the installed Claude Code CLI"]
async fn real_claude_follows_a_flattened_conversation() {
    use bloop::model::ChatMessage;
    // SAFETY: only this ignored test sets it, pointing the CLI work dir at a temp folder.
    unsafe {
        std::env::set_var(
            "BLOOP_DATA_DIR",
            std::env::temp_dir().join("bloop-real-claude"),
        )
    };
    let turn = |role: &str, content: &str| ChatMessage {
        role: role.into(),
        content: content.into(),
    };
    let messages = [
        turn("user", "My favourite colour is teal. Just say ok."),
        turn("assistant", "ok"),
        turn("user", "What is my favourite colour? One word."),
    ];
    let mut text = String::new();
    cli::chat(Kind::Claude, "", &messages, |event| {
        if let StreamEvent::Delta(d) = event {
            text.push_str(&d);
        }
        std::future::ready(true)
    })
    .await
    .unwrap();
    assert!(text.to_lowercase().contains("teal"), "{text}");
}
