# Native Island Implementation Plan

**Goal:** Runnable Windows floating island with cancellable local streaming chat.
**Architecture:** One Rust package with focused modules and event-driven UI.
**Tech Stack:** Iced 0.14/tiny-skia, Tokio, reqwest, Serde, rusqlite, tray-icon,
global-hotkey, windows-rs. Exact resolved dependencies live in Cargo.lock.
**Spec:** ../../architecture.md and the supplied bloop project brief.

User authorized independent architectural decisions and immediate implementation;
execute inline without a design approval pause.

- [ ] Provider/config: tests for fragmented UTF-8, NDJSON/SSE, API errors, unsafe
  endpoints, output limits, timeouts, cancellation, and bounded conversation context.
  Implement src/config.rs, src/provider.rs; verify against local TCP fixtures.
- [ ] Island: src/main.rs, src/ui.rs; compact/expanded animation with timer active
  only during transition, accessible text controls, light/dark themes, settings,
  streaming/error/cancel states and keyboard focus.
- [ ] Desktop integration: src/platform.rs; blocking tray/hotkey message loop,
  configurable shortcut, primary-monitor placement, taskbar suppression, cleanup.
- [ ] Persistence: src/storage.rs; SQLite settings/history under local app data,
  bounded history, explicit errors, no persisted credentials.
- [ ] Verify: cargo fmt --check, cargo clippy --all-targets -- -D warnings,
  cargo test; release build and real Windows UI smoke test. Record measured limits
  and remaining Phase 3 work in README.md. Commit meaningful completed changes.

Review focus: malformed/truncated streams must fail visibly; cancellation cannot
apply stale chunks; failed persistence must not erase history; hotkey collisions
must be visible; animation must stop requesting frames at rest.
