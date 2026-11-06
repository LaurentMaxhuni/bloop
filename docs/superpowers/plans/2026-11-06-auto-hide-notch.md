# Auto-hide Notch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the always-visible notch with a sliver that pops out on hover, on peeks, and on click/shortcut, and hides during fullscreen apps.

**Architecture:** A pure `presence` state machine (no Iced, no Win32) owns all show/hide rules and is driven by events with explicit `Instant`s. `ui.rs` swaps its `expanded: bool` for `Presence`, animates the window toward the presence shape, and feeds it cursor, focus, click, shortcut, streaming and fullscreen events. `platform.rs` gains a fullscreen query; window-region shaping is reused for every size.

**Tech Stack:** Rust 2024, iced 0.14 (wgpu, canvas), `windows` 0.62.

**Spec:** `docs/superpowers/specs/2026-11-06-auto-hide-notch-design.md`

## Global Constraints

- Sliver 120 × 5 logical px; compact 300 × 40; expanded 540 × 580.
- Hover delay 150 ms; leave delay 500 ms; peek lasts 4 s.
- Fullscreen poll every 2 s, only while not Open.
- No global mouse hooks: cursor tracking uses window-level `CursorEntered` / `CursorLeft` only.
- No timer at rest except the existing blink tick; 16 ms frames only while animating; 50 ms tick only while a presence deadline is pending.
- Fullscreen query failure means "not fullscreen".
- Clippy `undocumented_unsafe_blocks = "deny"` applies: every `unsafe` gets a `// SAFETY:` comment.
- Verify each task with `cargo clippy --all-targets` (no warnings) and `cargo test`.

## Review Focus

1. Cursor jitter at the screen edge (enter → leave → enter within 500 ms) must not flicker: stays Hover. → test in Task 1.
2. Shortcut during fullscreen should still open the panel (the user asked for it explicitly). → test in Task 1.
3. A peek arriving while hovering shows its message and does not shrink while the cursor is inside. → test in Task 1.
4. Two peeks in quick succession: latest message wins, 4 s restarts. → test in Task 1.
5. Focus lost while a reply is streaming in Open → Peek (mascot talking), not Sliver. → test in Task 1.

---

### Task 1: `presence` state machine

**Files:**
- Create: `src/presence.rs` (bin crate; `mod presence;` in `src/main.rs`)
- Test: unit tests inside `src/presence.rs`

**Interfaces:**
- Produces:
  - `pub enum State { Hidden, Sliver, Peek, Hover, Open }` (`Debug, Clone, Copy, PartialEq, Eq`)
  - `pub enum Shape { Sliver, Compact, Expanded }` — window shape to animate toward (Hidden uses `Sliver` and the window is hidden separately)
  - `pub enum Event { CursorEntered, CursorLeft, Tick, Click, Shortcut, Dismiss, FocusLost, Peek(String), Busy(bool), Fullscreen(bool) }`
  - `pub struct Presence` with `pub fn new() -> Self` (starts in `Sliver`), `pub fn on(&mut self, event: Event, now: Instant) -> bool` (true when `state()` or `message()` changed), `pub fn state(&self) -> State`, `pub fn shape(&self) -> Shape`, `pub fn message(&self) -> Option<&str>`, `pub fn next_deadline(&self) -> Option<Instant>`
  - `pub const HOVER_DELAY: Duration = 150 ms`, `LEAVE_DELAY = 500 ms`, `PEEK_FOR = 4 s`

Rules (from the spec):
- Sliver + `CursorEntered` arms a hover deadline; `Tick` at/after it → Hover. `CursorLeft` before it cancels.
- Hover/Peek + `CursorLeft` arms a leave deadline; `CursorEntered` cancels it; `Tick` at/after it → Sliver (Peek only once its 4 s is also over).
- `Peek(msg)`: from Sliver/Hover → Peek with message, deadline `now + PEEK_FOR`; in Open → only sets `message()`; in Hidden → queued (latest wins), replayed on `Fullscreen(false)`.
- Peek expiry with cursor inside → Hover (message cleared).
- `Busy(true)` while not Open pins Peek (no expiry). `Busy(false)` → `Peek("All done")` for 4 s if not Open.
- `Click` from Hover/Peek → Open. `Shortcut` from any state (including Hidden) → Open; from Open → Sliver.
- `Dismiss` / `FocusLost` in Open → Sliver, or Peek pinned if busy.
- `Fullscreen(true)` from any state except Open → Hidden; `Fullscreen(false)` → Sliver, then replay queued peek.
- `shape()`: Sliver/Hidden → Sliver, Peek/Hover → Compact, Open → Expanded.

- [ ] **Step 1: Write the failing tests**

```rust
fn at(ms: u64) -> Instant // base instant + ms
#[test] fn brushing_past_does_not_open()      // enter@0, leave@100, tick@200 → Sliver
#[test] fn resting_opens_hover()              // enter@0, tick@150 → Hover, shape Compact
#[test] fn leaving_hover_shrinks_after_delay() // Hover, leave@1000, tick@1400 → Hover, tick@1500 → Sliver
#[test] fn edge_jitter_keeps_hover()          // Hover, leave@1000, enter@1200, tick@1800 → Hover
#[test] fn peek_expires_to_sliver()           // Peek("hi")@0: message Some("hi"); tick@3999 → Peek; tick@4000 → Sliver
#[test] fn peek_with_cursor_inside_becomes_hover() // Peek@0, enter@100, tick@4000 → Hover, message None
#[test] fn latest_peek_wins()                 // Peek("a")@0, Peek("b")@3000, tick@5000 → Peek, message "b"
#[test] fn peek_while_hovering_keeps_open()   // Hover, Peek("x")@0, tick@4000 (cursor inside) → Hover
#[test] fn click_and_shortcut_open()          // Hover + Click → Open, shape Expanded; Sliver + Shortcut → Open; Open + Shortcut → Sliver
#[test] fn fullscreen_hides_and_replays_peek() // Fullscreen(true) → Hidden; Peek("t") → Hidden; Fullscreen(false) → Peek, message "t"
#[test] fn shortcut_opens_during_fullscreen() // Hidden + Shortcut → Open
#[test] fn focus_loss_while_busy_pins_peek()  // Open, Busy(true), FocusLost@0, tick@60_000 → Peek; Busy(false)@60_000 → message "All done"; tick@64_000 → Sliver
#[test] fn next_deadline_only_when_pending()  // new(): None; after enter@0: Some(at(150))
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --bin bloop presence`
Expected: compile errors (module/items missing).

- [ ] **Step 3: Implement `Presence` in `src/presence.rs` per the rules above.** Store `state`, `inside: bool`, `hover_at: Option<Instant>`, `leave_at: Option<Instant>`, `peek_until: Option<Instant>`, `busy: bool`, `message: Option<String>`, `queued: Option<String>`. `next_deadline()` is the earliest of the three deadlines.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --bin bloop presence` → all pass. `cargo clippy --all-targets` → no warnings (allow `dead_code` on the module until Task 3 wires it, removed in Task 3).

- [ ] **Step 5: Commit** — `feat: add presence state machine for the auto-hide notch`

### Task 2: Fullscreen detection

**Files:**
- Modify: `Cargo.toml` (add `"Win32_UI_Shell"` to the `windows` features)
- Modify: `src/platform.rs` (new function next to `primary_position`)

**Interfaces:**
- Produces: `pub fn fullscreen_active() -> bool` — `SHQueryUserNotificationState`; true for `QUNS_BUSY`, `QUNS_RUNNING_D3D_FULL_SCREEN`, `QUNS_PRESENTATION_MODE`, and `QUNS_NOT_PRESENT` (locked / screen saver / user switched); false on error or non-Windows.

- [ ] **Step 1: Write the test** `fullscreen_query_does_not_panic` in `platform.rs` tests: calls `fullscreen_active()` and asserts nothing (smoke test; real behavior is checked manually in Task 4).
- [ ] **Step 2: Run** `cargo test --bin bloop fullscreen` → fails to compile.
- [ ] **Step 3: Implement `fullscreen_active()`** with a `// SAFETY:` comment on the call.
- [ ] **Step 4: Run** `cargo test --bin bloop fullscreen` → pass; clippy clean.
- [ ] **Step 5: Commit** — `feat: detect fullscreen apps on Windows`

### Task 3: Wire presence into the UI

**Files:**
- Modify: `src/ui.rs` (state, messages, subscription, update, view, tests)
- Modify: `src/main.rs` (initial window size `ui::SLIVER`)
- Modify: `src/mascot.rs` (`Mood::Perk`)

**Interfaces:**
- Consumes: Task 1 `Presence`, `Event`, `State`, `Shape`; Task 2 `platform::fullscreen_active()`; existing `mascot::notch_outline`, `platform::shape`.
- Produces: `pub const SLIVER: Size = Size::new(120.0, 5.0)`; `COMPACT`, `EXPANDED` unchanged; `fn size_for(shape: Shape) -> Size`; new messages `Cursor(bool)` (entered/left), `Focus(bool)`, `Deadline(Instant)`, `FullscreenPoll`, `PeekTest`.

Changes:
- Replace `expanded: bool` with `presence: Presence`; `self.expanded` reads become `self.presence.state() == State::Open`.
- Replace `toggle()` with `fn present(&mut self, event: presence::Event) -> Task<Message>`: applies the event at `Instant::now()`, and when the shape changes starts the existing `Animation` toward `size_for(shape)`; on entering Open also focuses the window and the prompt (current `toggle` behavior). Platform `Toggle` and the in-app Toggle/Hide button map to `Shortcut`; clicking the compact pill maps to `Click`; Escape in Open maps to `Dismiss`.
- Hidden: `window::set_mode(id, Mode::Hidden)` on entering, `Mode::Windowed` on leaving.
- Subscriptions: `iced::event::listen_with` for `mouse::Event::CursorEntered/CursorLeft` → `Cursor`, `window::Event::Focused/Unfocused` → `Focus`; `every(50 ms)` → `Deadline` only while `presence.next_deadline().is_some()`; `every(2 s)` → `FullscreenPoll` only while not Open.
- Streaming: send `Busy(true)` on Send, `Busy(false)` on Finished/Cancel.
- `corner_radius()`: sliver shoulder 3 px and radius 2.5 px; compact/expanded as today (pass shoulder into `notch_outline`/`Notch` from size).
- View: Sliver/Hidden → only the `Notch` canvas; Peek → compact row with `presence.message()` instead of the status line; Hover → compact row as today; Open → header status line shows `presence.message()` when set, else the mood status.
- Mascot: add `Mood::Perk` (eyes wide, small hop, 0.6 s) reacted when presence enters Hover.
- Nap rule: Sleepy when not Open and idle ≥ 45 s (unchanged meaning).
- Debug-only `Peek test` ghost button in Settings (`#[cfg(debug_assertions)]`) sending `Peek("Hi! This is a peek.")`.

- [ ] **Step 1: Update tests** — rewrite `animation_can_reverse_and_stops_at_rest` to use `present(Event::Shortcut)` twice (SLIVER → toward EXPANDED → back to SLIVER); `failed_save_keeps_application_open_on_quit` asserts `presence.state() == State::Open`; add `peek_message_shows_in_compact` (PeekTest → `presence.state() == State::Peek`, `app.size` animates toward COMPACT) and `send_marks_presence_busy` (Send then FocusLost → Peek).
- [ ] **Step 2: Run** `cargo test --bin bloop ui` → failures/compile errors.
- [ ] **Step 3: Implement the changes above.**
- [ ] **Step 4: Run** `cargo test` → all pass; `cargo clippy --all-targets` clean; remove Task 1's `dead_code` allow.
- [ ] **Step 5: Commit** — `feat: auto-hide notch with hover, peek and fullscreen hiding`

### Task 4: Docs and manual verification

**Files:**
- Modify: `README.md` (usage: hover the sliver at the top edge; click/shortcut; hides in fullscreen)
- Modify: `docs/verification.md` (add the checks below with results)

- [ ] **Step 1: Build release** — `cargo build --release`; close any running `bloop.exe` first.
- [ ] **Step 2: Manual checks on Windows** (screenshots with `BLOOP_DATA_DIR` pointing at a scratch dir): only the sliver is visible at rest; clicks pass through right next to it; resting on it shows the pill, leaving shrinks it after ~0.5 s; click opens the panel; clicking another window closes it; Peek test pops then hides after ~4 s; a fullscreen video hides the sliver and it returns after.
- [ ] **Step 3: Record results** in `docs/verification.md`; update README.
- [ ] **Step 4: Commit** — `docs: document and verify the auto-hide notch`

Note: the spec's optional sliver hint (progress line / dot) has no producer until sub-project 2 (timers, music, notifications), so it ships with those widgets rather than here.
