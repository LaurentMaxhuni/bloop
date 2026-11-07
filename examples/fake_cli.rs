//! Test double for the Claude Code / Codex CLIs (used by `tests/cli.rs`).
//!
//! Environment:
//! - `FAKE_SLEEPER=1`: just sleep forever (used as a grandchild).
//! - `FAKE_STDIN`: file that receives everything read from stdin.
//! - `FAKE_PID`: file that receives this process id.
//! - `FAKE_CHILD_PID`: if set, start a sleeping copy of this program and write its pid there.
//! - `FAKE_FIXTURE`: JSONL file replayed to stdout, one line at a time.
//! - `FAKE_DELAY_MS`: pause between lines.
//! - `FAKE_HANG=1`: after replaying, sleep forever instead of exiting.
//! - `FAKE_MARKER` / `FAKE_AFTER_MS`: write the marker file this long after replaying.
//! - `FAKE_STDERR` / `FAKE_EXIT`: text for stderr and the exit code.

use std::{
    env, fs,
    io::{Read, Write},
    thread::sleep,
    time::Duration,
};

fn main() {
    if env::var_os("FAKE_SLEEPER").is_some() {
        loop {
            sleep(Duration::from_secs(60));
        }
    }
    if let Some(path) = env::var_os("FAKE_PID") {
        fs::write(path, std::process::id().to_string()).unwrap();
    }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    if let Some(path) = env::var_os("FAKE_STDIN") {
        fs::write(path, &input).unwrap();
    }
    if let Some(path) = env::var_os("FAKE_CHILD_PID") {
        // Deliberately left running: the test checks that cancelling kills it.
        #[allow(clippy::zombie_processes)]
        let child = std::process::Command::new(env::current_exe().unwrap())
            .env("FAKE_SLEEPER", "1")
            .spawn()
            .unwrap();
        fs::write(path, child.id().to_string()).unwrap();
    }
    let delay = env::var("FAKE_DELAY_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if let Some(path) = env::var_os("FAKE_FIXTURE") {
        let mut out = std::io::stdout();
        for line in fs::read_to_string(path).unwrap().lines() {
            writeln!(out, "{line}").unwrap();
            out.flush().unwrap();
            sleep(Duration::from_millis(delay));
        }
    }
    if let Some(path) = env::var_os("FAKE_MARKER") {
        // Stands in for a tool that would run after the event was refused.
        let after = env::var("FAKE_AFTER_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        sleep(Duration::from_millis(after));
        fs::write(path, "ran").unwrap();
    }
    if let Ok(text) = env::var("FAKE_STDERR") {
        eprintln!("{text}");
    }
    if env::var_os("FAKE_HANG").is_some() {
        loop {
            sleep(Duration::from_secs(60));
        }
    }
    std::process::exit(
        env::var("FAKE_EXIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
    );
}
