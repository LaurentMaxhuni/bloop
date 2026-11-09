# Landing page — design

Date: 2026-11-08 · Status: approved · Updated 2026-11-09 for the app window (PR LaurentMaxhuni/bloop#1)

## Goal

A landing page that shows what bloop feels like and gets Windows users to the installer:
laurentmaxhuni.github.io/bloop, built from `site/` in this repo and published by a GitHub
Action. The page itself behaves like the app: a live notch you can hover and open.

Success: a visitor on a laptop hovers the notch at the top of the page and watches it grow
into the pill and panel with the app's spring motion; the Download button gets them the
newest `bloop-setup-X.Y.Z.exe` in one click; the page scores well on accessibility, works on a
phone, and needs no build step.

Out of scope: a docs site, blog, analytics/tracking, a custom domain (can be added later),
macOS/Linux downloads, localisation.

## Structure

`site/index.html`, `site/styles.css`, `site/app.js`, `site/spring.js`, `site/mascot.js` (SVG mascot),
the handful of Lucide icons used as an inline SVG sprite in `index.html` (ISC licence noted),
`site/favicon.svg`, `site/og.png` (1200×630 social card, rendered from `site/og.html`). No framework, no bundler, no
external scripts; fonts are the system stack (`"Segoe UI Variable Text", "Segoe UI",
system-ui, sans-serif`) like the app. Colours are the app's shadcn/ui "neutral" tokens from
`src/style.rs`, light and dark.

## Sections

1. **Hero.** A live notch fixed at the top centre of the viewport (sliver 120×5 → pill
   300×40 → panel 540×min(580, 70vh)) drawn with the app's silhouette (concave shoulders,
   rounded bottom). Hover the sliver → pill after 150 ms; click the pill → panel; leaving or
   Escape → back. The panel shows a short scripted chat ("What's on my calendar?" style,
   generic, no real data) that types out once. The mascot blinks every few seconds, perks
   on hover, and its eyes follow the cursor. Headline "Your AI, tucked into the top of your
   screen." with a one-line subhead, **Download for Windows** (`vX.Y.Z · N.N MB`) and
   **View on GitHub**.
2. **How it works.** Three static mini-notches (sliver, pill, panel) with "Rest your mouse
   on the sliver", "Click to chat", "Ctrl+Alt+B from anywhere". The notch is only for
   chatting; setup and settings live in the app window (section 4).
3. **Bring your own model.** Cards for Ollama, Unsloth Studio, Claude Code, Codex and any
   OpenAI-compatible server; notes: local-first, keys in Windows Credential Manager, CLIs run
   chat-only.
4. **A real app window for setup.** (Was "the hub's four tiles": the notch's Customize hub
   was replaced by the app window in PR #1.) A static mock of the bloop window: first run
   walks through Welcome → Model → How it works → Make it yours → All set, then the
   dashboard with its sidebar (Home, Appearance, Model, Shortcut & startup, About). The
   Appearance page in the mock is live: the six accent swatches recolour the page (buttons,
   links, mascot blush) and are remembered in `localStorage` (wrapped in try/catch); a
   Snappy / Smooth / Calm switch changes the hero notch's spring. Copy: the notch's settings
   icon or the tray's Settings open the window; closing it leaves bloop running.
5. **Private by design.** Chats stay in a local SQLite file; no tracking; updates are
   checked against a SHA-256 from GitHub before installing.
6. **Footer.** Install note (the installer's "Launch bloop" opens setup in the app window;
   Windows SmartScreen shows "unknown publisher" until the app is
   code-signed: More info → Run anyway), MIT licence, GitHub link, release notes link.

## Behaviour

- **Springs:** the same critically damped spring as `motion.rs` (Snappy k=900, Smooth 400,
  Calm 180, `dt` clamped to 1/30 s), driven by `requestAnimationFrame`, idle when settled.
- **Download button:** on load, fetch `https://api.github.com/repos/LaurentMaxhuni/bloop/releases/latest`
  (no auth), pick the asset `bloop-setup-<v>.exe`, show `v<v> · <size> MB` and link to its
  `browser_download_url`. On any failure (rate limit, offline) keep the default link to
  `https://github.com/LaurentMaxhuni/bloop/releases/latest`. The result is cached in
  `sessionStorage` for 10 minutes.
- **Theme:** follows `prefers-color-scheme`; a header toggle overrides it (remembered).
- **Reduced motion:** `prefers-reduced-motion` snaps the notch, disables typing and eye
  tracking.
- **Touch / small screens:** tapping the sliver opens the pill, tapping again opens the
  panel; below 600 px the panel is full-width minus 16 px gutters; no horizontal scroll.
- **Accessibility:** the notch is a real `<button>` with `aria-expanded`; Escape closes;
  visible focus rings; text contrast ≥ 4.5:1 in both themes and every accent (accents only
  colour large text, icons and borders where contrast is lower); `lang="en"`; one `h1`.

## Publishing

`.github/workflows/pages.yml`: on push to `main` touching `site/**` (and manual dispatch),
upload `site/` with `actions/upload-pages-artifact` and deploy with `actions/deploy-pages`
(pinned major versions). Repository setting Pages → Source "GitHub Actions" must be enabled
once (the user does this, or `gh api` with their go-ahead).

## Testing

- The spring code lives in `site/spring.js` as a small pure module with a matching
  `site/spring.test.html` that runs its checks in the browser (settles within budget per
  preset, no overshoot, reversal keeps velocity, dt clamp) and prints pass/fail.
- In the browser pane, served locally: no console errors; hover/click/Escape states of the notch; spring
  feel at Smooth; accent switch recolours and persists; download button resolves to
  v0.3.1 and its size; layout at 375 px, 768 px and 1440 px with no horizontal scroll;
  reduced-motion emulation snaps; keyboard-only path reaches and operates every control;
  axe-style contrast spot checks on both themes.
- After deploy: the live URL loads, assets resolve, the OG card shows.
