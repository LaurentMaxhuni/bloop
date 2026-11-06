# Auto-hide notch — design

Date: 2026-11-06 · Status: awaiting review · Sub-project 1 of 3 (shape → widgets → installer/onboarding)

## Goal

The notch must stop covering other windows. It stays out of the way as a thin sliver
and only grows when the user hovers it, opens it, or bloop has something to say.

Success looks like:

- At rest only a ~120 × 5 px sliver is on screen; everything else is clickable.
- Resting the mouse on the sliver shows the compact pill for as long as the mouse stays.
- bloop can "peek" the pill on its own for a few seconds to say something.
- Fullscreen video, games and presentations hide it completely.
- The full chat panel still works exactly as today (click / shortcut / Escape).

Out of scope here: notifications, timers, music, weather (sub-project 2) and the
installer/onboarding (sub-project 3). This part builds the presence system they plug into.

## States

| State | Entered when | Window size | Shows |
|---|---|---|---|
| Hidden | A fullscreen app/presentation runs, or the session is locked | sliver, but window hidden | nothing |
| Sliver | Default at rest | 120 × 5 | dark pill; optional hint (progress line or dot) |
| Peek | `peek(message)` is called | compact (300 × 40) | mascot + one-line message, ~4 s |
| Hover | Cursor rests on the sliver/pill ≥ 150 ms | compact | mascot + status line |
| Open | Click on pill, or global shortcut | expanded (540 × 580) | current chat / settings panel |

Transitions:

- Sliver → Hover: cursor inside the window for 150 ms (brushing past does nothing).
- Hover → Sliver: cursor left the window for 500 ms.
- Peek → Sliver: 4 s elapsed **and** cursor not inside; if the cursor is inside, it becomes Hover.
- Hover/Peek → Open: click. Sliver/Hover/Peek → Open: shortcut.
- Open → Sliver: Escape (when nothing else to dismiss), shortcut, "Hide", or the window loses focus.
- Any → Hidden: fullscreen detected. Hidden → previous resting state (Sliver) when it ends.
  While Hidden, peeks are queued (latest wins) and replayed on return.
- If the user closes the panel while a reply is still streaming, the notch stays in
  Peek (compact, mascot talking) until the reply ends, then peeks "All done" for 4 s
  and shrinks to the sliver.

## Architecture

- **`presence` module (new, pure logic, no Iced/Win32):** a small state machine
  `Presence { state, hover_since, left_since, peek_until, queued_peek }` with
  `fn on(event, now) -> Presence` and `fn target_size() -> Size`. Events: `CursorEntered`,
  `CursorLeft`, `Tick`, `Click`, `Shortcut`, `Dismiss`, `FocusLost`, `Peek(msg)`,
  `Fullscreen(bool)`. Fully unit-testable with fake `Instant`s.
- **`ui.rs`:** replaces the `expanded: bool` toggle with `Presence`. The existing size
  animation (`Animation`, eased, interruptible) animates toward `presence.target_size()`.
  The view renders sliver / compact / expanded content from the state.
- **`mascot.rs`:** `notch_outline` already scales with size; the sliver uses the same
  silhouette with a tiny shoulder. Add a `Perk` reaction when hover starts.
- **`platform.rs`:** add `fullscreen_active() -> bool` using `SHQueryUserNotificationState`
  (busy / D3D fullscreen / presentation mode = hidden). Polled every 2 s only while not Open.
  Window region shaping (`shape`) is reused for every size, so the hidden parts never
  block clicks.
- **Cursor tracking:** iced window-level `mouse::Event::CursorEntered/CursorLeft`; no
  global mouse hooks.
- **Timers:** reuse the subscription model — 16 ms frames only while animating, a
  50 ms tick only while a hover/leave/peek deadline is pending, 2 s fullscreen poll,
  otherwise the existing slow blink tick.
- **Dev trigger:** a `Peek test` ghost button in Settings (debug builds only) so the
  peek path can be tried before widgets exist.

## Error handling

- `SHQueryUserNotificationState` failure → assume not fullscreen (never strand the user
  without the notch).
- Region shaping failure → window stays rectangular but functional (current fallback).
- Peek while Open → ignored for size, message shown in the panel's status line instead.

## Testing

- Unit tests for `presence`: hover delay, brush-past ignored, leave delay, peek expiry,
  peek converting to hover when cursor is inside, fullscreen hide + queued peek replay,
  focus loss closing Open, shortcut from every state.
- Existing UI tests updated from `toggle()` to presence events.
- Manual check on Windows: sliver size on screen, hover open/close, click-through around
  the sliver, fullscreen video hides it, peek test button.
