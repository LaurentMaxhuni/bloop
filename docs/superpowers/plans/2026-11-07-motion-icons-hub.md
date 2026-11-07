# Motion, Icons and Hub Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fluid, frame-synced notch animation inside a fixed window; icon buttons; a four-tile customization hub replacing Settings.

**Architecture:** A pure spring module drives the drawn notch size; the OS window is created once at the largest size and only its click-through region changes. Appearance settings live in `Config` with serde defaults equal to today. A Lucide icon font renders header and tile icons. The hub is a new `ui/hub.rs` that absorbs `settings_view`.

**Tech Stack:** Rust 2024, Iced 0.14 (`window::frames`, canvas, tooltip), `windows` 0.62, Lucide font (ISC).

**Spec:** `docs/superpowers/specs/2026-11-07-motion-icons-hub-design.md`

## Global Constraints

- Spring: `x'' = k(target − x) − 2√k·x'`; `dt` clamped to 1/30 s; settled when |Δ| < 0.5 px and |v| < 5 px/s. Presets: Snappy k=900, Smooth k=400 (default), Calm k=180.
- Reduce motion: shapes jump; content cross-fades 120 ms. Default follows `SPI_GETCLIENTAREAANIMATION`.
- Content fade starts at 60 % of height progress; slide 6 px.
- Notch widths: S compact 260 / panel 480; M 300 / 540 (default); L 360 / 600. Panel height 580, compact height 40, sliver 120×5. Left/right inset 24 px.
- Hover delay choices 100 / 150 (default) / 300 ms.
- Accents: Neutral (today), Indigo `#4f46e5`, Teal `#0d9488`, Rose `#e11d48`, Amber `#d97706`, Lime `#65a30d`; accent sets `primary`, `ring`, mascot blush.
- Theme: Light / Dark / System (`AppsUseLightTheme`); old `light: true` → Light, `false` → Dark.
- No frame subscription when nothing animates. Region update only when the rounded size changes ≥ 1 px.
- Icon font failure → word labels. Win32 only in `platform.rs`, `// SAFETY:` on every `unsafe`.
- Tooltips: "Dark mode"/"Light mode", "Customize", "Hide".

## Review Focus

1. Mouse leaves mid-grow → shape reverses smoothly without a jump (velocity continuity). Test in Task 1.
2. Clicks just outside the drawn notch while animating and at rest reach the window underneath. Manual check in Task 4 (region tracks the drawn outline with the notch offset).
3. A 240 Hz frame rate or a 500 ms stall (`dt` spike) → no overshoot explosion. Test in Task 1.
4. Switching notch width/position while the panel is open → window re-placed, outline follows, no stale region. Test in Task 6 (state) + manual.
5. Old config with only `light: true` → Light theme, M width, centre, Smooth. Test in Task 2.

---

### Task 1: Springs (pure)

**Files:** Create `src/motion.rs`; modify `src/lib.rs`.

**Interfaces — Produces:** `enum Feel { Snappy, Smooth, Calm }` with `stiffness(self) -> f32`; `struct Spring { pub value: f32, pub velocity: f32, pub target: f32 }` with `new(value)`, `step(&mut self, k: f32, dt: f32)`, `settled(&self) -> bool`, `snap(&mut self)`; `struct SizeSpring { pub w: Spring, pub h: Spring }` with `new(Size)`, `set_target(Size)`, `step(k, dt)`, `settled()`, `size() -> (f32, f32)`, `progress_h(from_h, to_h) -> f32` (0..1 clamped).

- [ ] Tests: `converges_within_budget` (each Feel from 5→580 at dt 1/60 and 1/144 settles within 0.6/0.9/1.4 s); `reversal_keeps_velocity` (step halfway, change target, next value differs from previous by < 1.5× the prior per-frame delta); `huge_dt_is_clamped` (single step dt 0.5 never overshoots target by > 10 %); `snap_jumps` (value == target, velocity 0, settled).
- [ ] Run → fail; implement (semi-implicit Euler, sub-steps of ≤ 1/240 s inside `step`); run → pass; commit `feat: spring motion`.

### Task 2: Appearance settings in config

**Files:** Modify `src/config.rs`.

**Interfaces — Produces:** enums `ThemeChoice { Light, Dark, System }`, `Accent { Neutral, Indigo, Teal, Rose, Amber, Lime }`, `NotchWidth { S, M, L }` with `compact(self) -> (f32,f32)` and `panel(self) -> (f32,f32)`, `NotchPosition { Centre, Left, Right }`, `ReduceMotion { System, On, Off }`; `Config` fields `theme`, `accent`, `motion: Feel`, `reduce_motion`, `notch_width`, `notch_position`, `mascot: bool` (default true), `hover_delay_ms: u16` (default 150; validate ∈ {100,150,300}). Keep `light` readable: custom `Deserialize` path via `#[serde(default)] light: Option<bool>` + `fn normalize(&mut self)` that maps it to `theme` when `theme` was absent (track with `#[serde(default)] theme: Option<ThemeChoice>` internally, or an equivalent approach — tests decide). Serialized output never writes `light`.

- [ ] Tests: `old_light_true_loads_as_light_theme`; `old_light_false_loads_as_dark`; `defaults_match_today` (M width sizes 300×40 / 540×580, Centre, Smooth, Neutral, mascot on, 150 ms); `new_fields_round_trip`; `bad_hover_delay_rejected`.
- [ ] Run → fail; implement; update every `config.light` use in `ui.rs`/`ui/onboarding.rs` to the resolved theme (`Island::light()` keeps its name, resolving System via a cached `system_light: bool`); run → pass; commit `feat: appearance settings`.

### Task 3: Platform reads and accent tokens

**Files:** Modify `src/platform.rs`, `src/style.rs`, `src/mascot.rs`.

**Interfaces — Produces:** `platform::reduce_motion_system() -> bool` (true when animations are off); `platform::light_theme_system() -> bool`; `style::Tokens::with_accent(light: bool, accent: Accent) -> Tokens` (Neutral == `Tokens::new`); `Mascot` gains `blush: Color` (used instead of face-alpha when not transparent).

- [ ] Tests: `accent_neutral_is_today` (equal to `Tokens::new` for both themes); `accent_sets_primary_and_ring` (Indigo primary == #4f46e5); `system_reads_do_not_panic` (Windows).
- [ ] Run → fail; implement (`SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`, registry `RegGetValueW` DWORD); run → pass; commit `feat: Windows theme/motion reads and accents`.

### Task 4: Fixed window, frame-synced springs

**Files:** Modify `src/main.rs`, `src/ui.rs`, `src/platform.rs`.

**Interfaces:**
- Consumes: Task 1 `SizeSpring`/`Feel`, Task 2 `NotchWidth`/`NotchPosition`, Task 3 `reduce_motion_system`.
- Produces: `platform::place(size: Size, position: NotchPosition) -> Option<Point>` (window origin for the fixed canvas); `Island::canvas_size() -> Size` (largest panel + 2·SHOULDER, height 580); `Island::notch_rect() -> Rectangle` (drawn notch inside the window, top-aligned, x per position); `platform::shape` takes the outline already offset by `notch_rect().x`.
- Replaces `Animation` with `spring: SizeSpring`; `self.size` becomes the spring's current size. `Message::Frame(Instant)` comes from `window::frames()` only while `!spring.settled()`; it steps with real `dt` (k from the motion preset; reduce motion → `snap`), then updates the region when the rounded size changed. `Window` and notch-setting changes call a new `place_window()` (resize to `canvas_size`, move once, region).
- View: a top-level `container` of `canvas_size` with the notch `stack` placed at `notch_rect()`; content opacity = 0 below 60 % height progress then linear to 1, translated −6 px → 0 (use `container` padding offset); compact pill text fades the same way. Remove the "blank below compact" branch.

- [ ] Tests (update existing animation tests): `animation_can_reverse_and_stops_at_rest` uses Frame steps with explicit instants; `reduce_motion_snaps`; `no_frames_when_settled` (subscription returns no frames after settle — assert via a `fn animating(&self) -> bool`); `notch_rect_follows_position` (Left → x == SHOULDER, Right → x == canvas − width − SHOULDER, Centre centred).
- [ ] Run → fail; implement; run → pass; clippy clean.
- [ ] Manual (debug, scratch data dir): open/close/hover/peek at 60 Hz and highest rate with `BLOOP_FRAME_LOG=1` logging `dt` while animating (implement as `tracing::debug!` behind the env var): no gap > 2 frames; clicking 20 px beside the sliver and beside the pill mid-animation reaches the window below (verify with `WindowFromPoint` script as in the auto-hide check).
- [ ] Commit `feat: smooth frame-synced notch animation`.

### Task 5: Icon font and header icons

**Files:** Create `assets/lucide.ttf`, `assets/LUCIDE-LICENSE.txt`, `src/icons.rs`; modify `src/main.rs` (`.font(icons::FONT_BYTES)`), `src/ui.rs` header.

**Interfaces — Produces:** `icons::FONT_BYTES: &[u8]`, `icons::FONT: Font` (family "lucide"), `enum Icon { Sun, Moon, SlidersHorizontal, ChevronUp, ArrowLeft, Palette, Sparkles, PanelTop, Cpu }` with `codepoint(self) -> char`; `Island::icon_button(icon, fallback: &str, tooltip: &str, msg) -> Element`.

- [ ] Download (ask the user first, state source and size): `lucide-static` font + LICENSE from `https://cdn.jsdelivr.net/npm/lucide-static@<pinned version>/font/lucide.ttf` and `/LICENSE`; read code points for the nine icons from that version's `font/lucide.css`; record version in `icons.rs`.
- [ ] Tests: `codepoints_are_private_use` (all in U+E000–U+F8FF); `font_bytes_look_like_ttf` (starts with 0x00010000 or "true").
- [ ] Run → fail; implement header: theme icon (Sun when dark / Moon when light, tooltip "Light mode"/"Dark mode"), sliders (tooltip "Customize", opens hub), chevron-up (tooltip "Hide"); fallback words if the font failed to load (track load result from Iced's `font::load` task → `Message::FontLoaded(bool)`).
- [ ] Run → pass; commit `feat: icon buttons`.

### Task 6: Customization hub

**Files:** Create `src/ui/hub.rs`; modify `src/ui.rs` (remove `settings_view`, `settings: bool` → `hub: Option<HubPage>`), `src/presence.rs` (hover delay setter), `src/ui/onboarding.rs` (Follow Windows option).

**Interfaces — Produces:** `enum HubPage { Grid, Look, Motion, Notch, Model }`; messages `OpenHub(HubPage)`, `SetTheme(ThemeChoice)`, `SetAccent(Accent)`, `SetFeel(Feel)`, `SetReduceMotion(ReduceMotion)`, `SetNotchWidth(NotchWidth)`, `SetNotchPosition(NotchPosition)`, `SetMascot(bool)`, `SetHoverDelay(u16)`; `Presence::set_hover_delay(Duration)` replacing the `HOVER_DELAY` constant use (constant stays as default). Platform tray "Settings" event opens `HubPage::Grid`. Look/Motion/Notch setters change `config` + `draft_config`, persist immediately, and (width/position) call `place_window()`; Model page = today's settings form + Save.

- [ ] Tests: `hub_navigation` (OpenHub(Grid) → Look → Escape returns to Grid → Escape closes hub); `look_changes_apply_and_persist_live` (SetAccent(Teal) → config.accent Teal, tokens primary #0d9488); `notch_width_change_replaces_window` (SetNotchWidth(L) → compact target 360 wide and a placement pending flag set); `hover_delay_applies_to_presence`; `tile_summaries` ("Dark · Indigo", "Smooth", "M · Centre", "Claude Code"); existing settings tests ported to `HubPage::Model`.
- [ ] Run → fail; implement; run → pass; clippy clean.
- [ ] Manual: each control applies live; Follow Windows tracks a Windows theme switch on next open; Reduce motion follows the Windows toggle.
- [ ] Commit `feat: customization hub`.

### Task 7: Docs, verification, review

- [ ] `docs/architecture.md`: fixed-window animation + springs paragraph; icons font note.
- [ ] `docs/verification.md`: automated counts, Task 4/6 manual tables, what wasn't checked.
- [ ] `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` clean; commit `docs: motion, icons and hub verification`.
