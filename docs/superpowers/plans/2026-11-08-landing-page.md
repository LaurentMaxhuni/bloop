# Landing Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A static, app-like landing page for bloop in `site/`, published to GitHub Pages.

**Architecture:** Plain HTML/CSS/JS, no build. `spring.js` mirrors `motion.rs`; `mascot.js` draws the mascot as SVG from the app's 24-unit grid; `app.js` runs the hero notch state machine, typing chat, accent/theme/feel switches and the release lookup. A Pages workflow uploads `site/`.

**Tech Stack:** HTML5, CSS (custom properties, `color-mix`), ES modules, GitHub Actions (`actions/upload-pages-artifact@v3`, `actions/deploy-pages@v4`, `actions/configure-pages@v5`).

**Spec:** `docs/superpowers/specs/2026-11-08-landing-page-design.md`

**Update 2026-11-09:** bloop gained an app window (PR LaurentMaxhuni/bloop#1): onboarding and
all settings moved out of the notch into a dashboard (Home, Appearance, Model, Shortcut &
startup, About). Task 2's "hub tiles" become a mock of that window, Task 4's switches live on
its Appearance page, and the mascot also thinks/talks/cheers/naps in the hero chat. Icons are
inline Lucide SVG (`site/icons.js`).

## Global Constraints

- No external scripts or fonts; font stack `"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`.
- Notch sizes: sliver 120×5, pill 300×40, panel 540×min(580, 70vh); shoulders 10 px; hover delay 150 ms; leave delay 500 ms.
- Springs: k Snappy 900 / Smooth 400 / Calm 180, damping 2√k, dt ≤ 1/30 s, sub-steps ≤ 1/240 s, settle |Δ|<0.5 & |v|<5.
- Accents as in the app: Neutral, Indigo #4f46e5, Teal #0d9488, Rose #e11d48, Amber #d97706, Lime #65a30d; body text never uses an accent colour.
- Release API `https://api.github.com/repos/LaurentMaxhuni/bloop/releases/latest`; fallback `https://github.com/LaurentMaxhuni/bloop/releases/latest`; cache 10 min in `sessionStorage`.
- Every storage access in try/catch; page works without storage, JS or network (static content + fallback links).
- Copy: headline "Your AI, tucked into the top of your screen."; install note about SmartScreen "More info → Run anyway".

## Review Focus

1. GitHub API rate-limited or offline → button still downloads via the Releases page. Check in Task 4.
2. A touch device with no hover → notch still opens by tap. Check in Task 3.
3. 375 px wide phone → panel and sections fit, no horizontal scroll. Check in Task 5.
4. Reduced motion on → no spring/typing/eye tracking, everything still reachable. Check in Task 3.
5. Keyboard only → Tab reaches the notch, Enter opens, Escape closes, focus visible. Check in Task 3.

---

### Task 1: Spring module and its browser test

**Files:** Create `site/spring.js`, `site/spring.test.html`.

**Interfaces — Produces:** `export const FEEL = { snappy: 900, smooth: 400, calm: 180 }`; `export class Spring { constructor(value); target; value; velocity; step(k, dt); settled(); snap() }`; `export class SizeSpring { constructor(w, h); setTarget(w, h); step(k, dt); settled(); snap(); get w(); get h() }`.

- [ ] Write `spring.test.html` running: each feel settles 5→580 within 0.6/0.9/1.4 s at 60 and 144 Hz with peak ≤ 580.5; reversal keeps moving forward one frame; dt 0.5 clamped (< 638); snap zeroes velocity. Prints "PASS n/n" or failures into `<pre id=out>`.
- [ ] Open via local server in the browser pane → fails (module missing). Implement `spring.js` (port of `motion.rs`). Reload → "PASS". Commit `feat(site): spring motion`.

### Task 2: Page skeleton, styles, mascot

**Files:** Create `site/index.html`, `site/styles.css`, `site/mascot.js`, `site/favicon.svg`.

**Interfaces — Produces:** `export function mascotSVG({ mood, blink, look:[x,y] }) -> string` (moods idle, perk, happy, thinking, talking, sleepy); CSS tokens `--bg --fg --muted --border --secondary --accent --accent-fg` for light/dark with `[data-theme]` overrides; `--accent` defaults to the neutral foreground.

- [ ] All six sections from the spec with static content, semantic landmarks, one `h1`, download/GitHub links pointing at the fallback URLs, footer install note. Mini-notches and the app-window mock (sidebar + onboarding strip + Appearance page) in CSS.
- [ ] Browser pane check at 1440 px: renders, no console errors. Commit `feat(site): landing page structure and look`.

### Task 3: The live hero notch

**Files:** Create `site/app.js`; modify `site/index.html`, `site/styles.css`.

**Interfaces — Consumes:** `SizeSpring`, `FEEL`, `mascotSVG`. **Produces:** notch states `sliver | pill | panel`, `<button class="notch" aria-expanded>`, `setFeel(name)`.

- [ ] State machine: pointerenter → pill after 150 ms; pointerleave → sliver after 500 ms (pill only); click/Enter on pill → panel; Escape/outside click → sliver; tap on touch opens progressively. Drawn size follows the spring via rAF only while moving; content fades in over the last 40 % of growth (as in the app). Panel types a 3-message scripted chat once. Mascot blinks every ~4 s, perks on pill, eyes follow pointer within the hero. Reduced motion: snap, no typing, no eye tracking.
- [ ] Browser checks: hover → pill, click → panel, Escape → sliver; keyboard Tab/Enter/Escape; emulate touch (mobile preset) tap opens; reduced-motion emulation snaps. Commit `feat(site): live hero notch`.

### Task 4: Switches and the download button

**Files:** Modify `site/app.js`, `site/index.html`.

- [ ] Accent swatches set `--accent`/`--accent-fg` and mascot blush, persisted; theme toggle (system / light / dark) persisted; feel switch calls `setFeel`. Release lookup with cache and fallback; button label `Download for Windows · v<v> · <size> MB`.
- [ ] Browser checks: switches apply and survive reload; button resolves to v0.3.1 from the real API; with the fetch URL forced to fail (`?offline` debug flag) the button keeps the Releases link and plain label. Commit `feat(site): accents, theme and live download link`.

### Task 5: Responsive, accessibility, social card

**Files:** Modify `site/styles.css`, `site/index.html`; create `site/og.png` (drawn by a PowerShell/System.Drawing script `site/make-og.ps1` from the mascot geometry).

- [ ] Layout at 375 / 768 / 1440: no horizontal scroll (`document.documentElement.scrollWidth <= innerWidth`), panel fits with 16 px gutters. Focus rings visible; contrast of body/muted text ≥ 4.5:1 in both themes (compute in the page with a small script in the browser pane). OG/Twitter meta + favicon.
- [ ] Commit `feat(site): responsive, accessible, social card`.

### Task 6: Publish

**Files:** Create `.github/workflows/pages.yml`; modify `README.md` (link to the page).

- [ ] Workflow: on push to `main` with `site/**` changes + `workflow_dispatch`; permissions `pages: write, id-token: write, contents: read`; jobs configure-pages → upload-pages-artifact (path `site`) → deploy-pages; concurrency group `pages`.
- [ ] With the user's go-ahead: merge to `main`, push, enable Pages with source "GitHub Actions" (`gh api -X POST repos/LaurentMaxhuni/bloop/pages -f build_type=workflow`), watch the run, open the live URL and repeat the Task 3–5 checks there. Commit/push docs.
