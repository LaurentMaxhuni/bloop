# Smooth motion, icons and the customization hub — design

Date: 2026-11-07 · Status: awaiting review

## Goal

- Opening, closing, hovering and peeking feel fluid: no wobbling edges, no skipped steps,
  smooth on 60–144 Hz screens, and interruptible mid-way.
- The header uses icons instead of the words Light/Dark, Settings and Hide.
- "Settings" becomes a customization hub: four tiles, each opening a small page.

Success: on a 60 Hz and a high-refresh screen, opening the panel shows no edge jitter and no
blank panel; reversing mid-animation (cursor leaves while growing) turns smoothly; every
setting from today's Settings is still reachable; old configs look and behave as before.

## Why it is choppy today

Each frame (16 ms timer) calls `window::resize`, `platform::shape` (window region) and
`window::move_to`. Three separate OS operations that don't land on the same refresh make the
edges wobble and frames drop; the timer is also not tied to the display. During the animation
the content is blank (`view` shows `Space` below compact height).

## Motion

- **Fixed window.** The OS window is created at the largest notch size (panel, plus the
  widest "L" notch width) and never resized or moved while animating. Position is set once
  per monitor/DPI/notch-position change.
- **Drawing animates, not the window.** `view` draws the notch silhouette at the animated size,
  top-aligned and placed per the notch position, inside the fixed transparent window.
- **Click-through.** `platform::shape` keeps cutting the window region to the drawn outline so
  everything outside it passes clicks to the windows below. While animating, the region is
  updated at most once per frame and only when the rounded size changed by ≥ 1 px.
- **Frame clock.** Animation steps come from `iced::window::frames()` (redraw-synced), not a
  timer. No frame subscription when nothing moves — at rest bloop stays idle as today.
- **Springs.** Width and height each follow a critically damped spring
  (`x'' = k(target − x) − c·x'`, `c = 2√k`) integrated per frame with the real `dt`
  (clamped to 1/30 s). Changing the target mid-way keeps position and velocity, so reversal
  is smooth. Settled when |Δ| < 0.5 px and |v| < 5 px/s.
- **Presets.** Snappy `k = 900`, Smooth `k = 400` (default), Calm `k = 180`.
- **Reduce motion.** Follows Windows' "Animation effects" (`SPI_GETCLIENTAREAANIMATION`) unless
  the user overrides it in the hub: shapes jump to the target and content cross-fades in 120 ms.
- **Content in motion.** Panel content fades (opacity follows height progress, starting at 60%
  of the way) and slides down 6 px into place; the compact pill's text fades likewise. The
  mascot keeps its own animations.

## Icons

- **Lucide** icon font (ISC licence), `assets/lucide.ttf` embedded with `include_bytes!`,
  loaded once at startup; a small `icons.rs` maps names to code points (only the glyphs used).
- Header: sun/moon (theme), sliders (hub), chevron-up (hide), each an icon button with a
  tooltip ("Dark mode", "Customize", "Hide"). Back buttons use arrow-left.
- Tiles: palette (Look), sparkles (Motion), panel-top (Notch), cpu (Model & shortcut).

## Customization hub

The header's sliders icon opens the hub (replacing Settings) with a 2×2 grid of tiles; each
tile shows its icon, title and a one-line summary of its current value ("Dark · Indigo").
A tile opens its page; the page has a back arrow to the grid. Changes in Look, Motion and
Notch apply live and save immediately; the Model page keeps today's Save button because it
validates and talks to Credential Manager.

| Tile | Controls |
|---|---|
| Look | Theme: Light / Dark / Follow Windows. Accent: Neutral, Indigo, Teal, Rose, Amber, Lime. |
| Motion | Feel: Snappy / Smooth / Calm. Reduce motion: Follow Windows / On / Off. |
| Notch | Width S / M / L (compact pill 260 / 300 / 360 px; panel 480 / 540 / 600 px). Position: Centre / Left / Right (left/right inset 24 px from the work-area edge). Mascot on/off. Hover delay: 0.1 / 0.15 / 0.3 s. |
| Model & shortcut | Today's Settings fields unchanged: provider, endpoint, model, API key, shortcut, start with Windows, Show setup again, Quit bloop. |

- **Accent** colours `primary` and `ring` tokens and the mascot's blush; Neutral is today's
  look. **Follow Windows** reads `AppsUseLightTheme` under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize` at start and whenever the
  panel opens.
- Config gains `theme: Theme { Light, Dark, System }`, `accent`, `motion`, `reduce_motion`,
  `notch_width`, `notch_position`, `mascot`, `hover_delay_ms`, all with serde defaults equal to
  today's behaviour. The old `light: bool` still loads: `true` → Light, `false` → Dark.
- Setup's "Make it yours" step keeps its two previews and gains "Follow Windows".

## Architecture

- `src/motion.rs` (library, pure): `Spring`, `Spring2` (size), presets, `step(dt)`,
  `settled()`. Unit-tested without Iced.
- `src/icons.rs` (bin): font bytes, `Icon` enum, `icon(Icon, size) -> Text`.
- `src/ui/hub.rs` (bin): hub grid and the four pages, moving `settings_view` out of `ui.rs`.
- `src/platform.rs`: `reduce_motion_system() -> bool`, `light_theme_system() -> bool`,
  `place(window, notch_position, size)` replacing per-frame `move_to`.
- `ui.rs`: replaces `Animation` with `Spring2`; `Frame` uses `window::frames()`; `view` draws
  the notch at the animated size inside the fixed window; header uses icons.

## Error handling

- Font fails to load → icons fall back to today's words (no crash).
- Windows setting reads fail → treated as "animations on" and "dark".
- Region update failure → logged once; the window still works (only click-through outside the
  notch would be affected).

## Testing

- Unit: spring converges for each preset within 0.6 s (Snappy) / 0.9 s (Smooth) / 1.4 s (Calm)
  at 60 and 144 Hz; reversal keeps velocity continuity; reduce-motion jumps; `dt` clamp; config
  round-trip and old `light` mapping; accent tokens; notch size table; hub navigation
  (grid → page → back), live-apply saves for Look/Motion/Notch.
- Manual on Windows (debug build, scratch data dir): open/close/hover/peek at 60 Hz and at the
  monitor's highest rate, recorded with frame timestamps (log the `dt` histogram while
  animating: no gaps > 2 frames); click-through outside the notch at rest and mid-animation;
  each hub control applies live; Reduce motion follows the Windows toggle.

Out of scope: custom user colours, per-monitor placement choice, mascot skins, sound,
widgets.
