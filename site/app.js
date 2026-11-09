// The landing page's behaviour: the live notch, Bloop, the demo chat, the Appearance
// switches and the live download link. No dependencies; everything degrades to plain links.

import { FEEL, SizeSpring } from './spring.js';
import { mascotSVG, isAnimated, lookToward } from './mascot.js';

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];
const root = document.documentElement;
const reduceQuery = matchMedia('(prefers-reduced-motion: reduce)');
const reduced = () => reduceQuery.matches;
const store = {
  get(key) { try { return localStorage.getItem(key); } catch { return null; } },
  set(key, value) {
    try { value == null ? localStorage.removeItem(key) : localStorage.setItem(key, value); } catch {}
  },
};
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/* ------------------------------------------------------------------ Bloop */

const ONE_SHOT = { happy: 1.6, perk: 0.6, oops: 0.7 };
const TRACKS = new Set(['idle', 'typing', 'perk']);
const pointer = { x: -1, y: -1, seen: false };

class Bloop {
  constructor(el) {
    this.el = el;
    this.base = 'idle';
    this.mood = 'idle';
    this.since = performance.now();
    this.look = [0, 0];
    this.visible = true;
    this.last = '';
  }
  /** Changes mood; one-shot moods fall back to the base mood when they finish. */
  set(mood) {
    if (mood in ONE_SHOT) {
      this.mood = mood;
    } else {
      this.base = mood;
      if (!(this.mood in ONE_SHOT)) this.mood = mood;
    }
    this.since = performance.now();
    wantFrame();
  }
  render(now, blink) {
    const age = (now - this.since) / 1000;
    if (this.mood in ONE_SHOT && age > ONE_SHOT[this.mood]) {
      this.mood = this.base;
      this.since = now;
    }
    if (!this.visible) return isAnimated(this.mood);
    let look = [0, 0];
    if (pointer.seen && !reduced() && TRACKS.has(this.mood)) {
      look = lookToward(this.el.getBoundingClientRect(), pointer.x, pointer.y);
    }
    const t = reduced() ? 0 : now / 1000;
    const svg = mascotSVG({
      mood: this.mood,
      blink,
      look,
      t,
      since: reduced() ? 99 : (now - this.since) / 1000,
    });
    if (svg !== this.last) {
      this.el.innerHTML = svg;
      this.last = svg;
    }
    return !reduced() && isAnimated(this.mood);
  }
}

const bloops = Object.fromEntries($$('[data-mascot]').map((el) => [el.dataset.mascot, new Bloop(el)]));
const all = Object.values(bloops);
const chatters = ['pill', 'panel', 'hero'].map((name) => bloops[name]).filter(Boolean);

let blinkUntil = 0;
let mascotFrame = 0;
function wantFrame() {
  if (!mascotFrame) mascotFrame = requestAnimationFrame(drawMascots);
}
function drawMascots(now) {
  mascotFrame = 0;
  const blink = now < blinkUntil;
  let again = blink;
  for (const b of all) again = b.render(now, blink) || again;
  if (again) wantFrame();
}

if ('IntersectionObserver' in window) {
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      const b = all.find((x) => x.el === e.target);
      if (b) b.visible = e.isIntersecting;
    }
    wantFrame();
  });
  for (const b of all) if (!b.el.closest('.notch')) io.observe(b.el);
}

(function blinkLoop() {
  setTimeout(() => {
    if (!reduced()) {
      blinkUntil = performance.now() + 140;
      wantFrame();
    }
    blinkLoop();
  }, 3200 + Math.random() * 2600);
})();

// Naps after a while with nothing happening, as in the app.
const NAP_AFTER = 45_000;
let lastActive = performance.now();
let napping = false;
function activity() {
  lastActive = performance.now();
  if (napping) {
    napping = false;
    for (const b of all) {
      b.set(b.base === 'sleepy' ? 'idle' : b.base);
      b.set('perk');
    }
  }
}
setInterval(() => {
  if (!napping && performance.now() - lastActive > NAP_AFTER && notch.state === 'sliver' && !chatBusy) {
    napping = true;
    for (const b of all) b.set('sleepy');
  }
}, 5000);

addEventListener('pointermove', (e) => {
  pointer.x = e.clientX;
  pointer.y = e.clientY;
  pointer.seen = true;
  activity();
  wantFrame();
}, { passive: true });
addEventListener('keydown', activity);
addEventListener('scroll', () => { activity(); wantFrame(); }, { passive: true });

/* ------------------------------------------------------------ The notch */

const K = 0.5522848;
const SHOULDER = 10;
const HOVER_DELAY = 150;
const LEAVE_DELAY = 500;

/** The notch silhouette (src/mascot.rs Notch::path), offset by ox. */
function notchPath(w, h, ox = 0, closed = true) {
  const s = Math.min(Math.max(h * 0.6, 3), SHOULDER);
  const r = Math.max(Math.min(Math.min(h / 2, 26), (w - 2 * s) / 2, h - s), 0);
  const n = (v) => Math.round(v * 100) / 100;
  const x = (v) => n(ox + v);
  let d = `M${x(0)} 0C${x(s * K)} 0 ${x(s)} ${n(s * (1 - K))} ${x(s)} ${n(s)}`;
  d += `L${x(s)} ${n(h - r)}C${x(s)} ${n(h - r + r * K)} ${x(s + r - r * K)} ${n(h)} ${x(s + r)} ${n(h)}`;
  d += `L${x(w - s - r)} ${n(h)}C${x(w - s - r + r * K)} ${n(h)} ${x(w - s)} ${n(h - r + r * K)} ${x(w - s)} ${n(h - r)}`;
  d += `L${x(w - s)} ${n(s)}C${x(w - s)} ${n(s * (1 - K))} ${x(w - s * K)} 0 ${x(w)} 0`;
  return closed ? d + 'Z' : d;
}

const notchEl = $('#notch');
const hit = $('#notch-hit');
const content = $('#notch-content');
const panel = $('#notch-panel');
const pillEl = $('.notch-pill');
const fillPath = $('.notch-fill', notchEl);
const linePath = $('.notch-line', notchEl);
const hint = $('#hint');

const notch = {
  state: 'sliver',
  feel: FEEL[store.get('bloop-feel')] ? store.get('bloop-feel') : 'smooth',
  spring: new SizeSpring(120, 5),
  sizes: null,
  canvas: [540, 580],
  raf: 0,
  last: 0,
};

function measure() {
  const vw = root.clientWidth;
  const narrow = vw < 600;
  const panelW = narrow ? vw - 32 : 540;
  const panelH = Math.min(580, Math.round(innerHeight * 0.7));
  notch.sizes = {
    sliver: [120, 5],
    pill: [Math.min(300, vw - 32), 40],
    panel: [panelW, panelH],
  };
  notch.canvas = [Math.max(panelW, 300), panelH];
  notchEl.style.setProperty('--canvas-w', `${notch.canvas[0]}px`);
  notchEl.style.setProperty('--canvas-h', `${notch.canvas[1]}px`);
}

function drawNotch() {
  const { spring, canvas, state } = notch;
  const w = spring.w;
  const h = spring.h;
  const ox = (canvas[0] - w) / 2;
  fillPath.setAttribute('d', notchPath(w, h, ox));
  linePath.setAttribute('d', notchPath(w, h - 0.5, ox, false));

  const [tw, th] = notch.sizes[state];
  const cx = (canvas[0] - tw) / 2;
  // Content is laid out at its final size and fades in over the last 40 % of the growth.
  const ratio = Math.min(h, th) / Math.max(h, th, 1);
  const shown = state === 'sliver' ? 0 : Math.min(Math.max((ratio - 0.6) / 0.4, 0), 1);
  notchEl.style.setProperty('--content-x', `${cx}px`);
  notchEl.style.setProperty('--content-w', `${tw}px`);
  notchEl.style.setProperty('--content-h', `${th}px`);
  notchEl.style.setProperty('--shown', shown.toFixed(3));
  content.style.clipPath = `path("${notchPath(w, h, ox - cx)}")`;
  // The hit area: generous around the 5 px sliver, the pill itself otherwise.
  const hw = state === 'sliver' ? Math.max(w, 168) : Math.max(w, 168);
  const hh = state === 'sliver' ? 20 : Math.max(h, 20);
  notchEl.style.setProperty('--hit-w', `${hw}px`);
  notchEl.style.setProperty('--hit-h', `${hh}px`);
}

function frame(now) {
  const dt = (now - notch.last) / 1000;
  notch.last = now;
  notch.spring.step(FEEL[notch.feel], dt);
  drawNotch();
  notch.raf = notch.spring.settled() ? 0 : requestAnimationFrame(frame);
}

function animate() {
  if (reduced()) {
    notch.spring.snap();
    drawNotch();
    return;
  }
  if (!notch.raf) {
    notch.last = performance.now();
    notch.raf = requestAnimationFrame(frame);
  }
}

let hoverTimer = 0;
let leaveTimer = 0;
let peekTimer = 0;
let opened = false;
let quietFocus = false;
function clearTimers() {
  clearTimeout(hoverTimer);
  clearTimeout(leaveTimer);
  clearTimeout(peekTimer);
}

function go(state, { focus = false } = {}) {
  clearTimers();
  if (state === notch.state) return;
  const was = notch.state;
  notch.state = state;
  notchEl.dataset.state = state;
  notch.spring.setTarget(...notch.sizes[state]);
  pillEl.hidden = state !== 'pill';
  panel.hidden = state !== 'panel';
  hit.setAttribute('aria-expanded', String(state === 'panel'));
  animate();
  drawNotch();

  if (state !== 'sliver' && was === 'sliver') {
    for (const b of chatters) b.set('perk');
    activity();
  }
  if (state === 'panel') {
    if (focus) requestAnimationFrame(() => panel.focus({ preventScroll: true }));
    if (!opened) {
      opened = true;
      hint.querySelector('[data-hint-text]').textContent = 'That\'s bloop. Press Esc or click outside to tuck it away.';
      runChat();
    }
  }
  if (state === 'sliver' && was === 'panel' && focus) {
    // Give focus back to the notch without the focus handler popping the pill again.
    quietFocus = true;
    hit.focus({ preventScroll: true });
    quietFocus = false;
  }
}

let lastPointer = 'mouse';
hit.addEventListener('pointerdown', (e) => { lastPointer = e.pointerType; });
hit.addEventListener('pointerenter', (e) => {
  if (e.pointerType === 'touch') return;
  clearTimeout(leaveTimer);
  if (notch.state === 'sliver') hoverTimer = setTimeout(() => go('pill'), HOVER_DELAY);
});
hit.addEventListener('pointerleave', (e) => {
  if (e.pointerType === 'touch') return;
  clearTimeout(hoverTimer);
  if (notch.state === 'pill') leaveTimer = setTimeout(() => go('sliver'), LEAVE_DELAY);
});
hit.addEventListener('click', () => {
  // Touch opens step by step (sliver → pill → panel); mouse and keyboard open the panel.
  if (lastPointer === 'touch' && notch.state === 'sliver') go('pill');
  else go('panel', { focus: true });
  lastPointer = 'mouse';
});
hit.addEventListener('focus', () => {
  if (!quietFocus && hit.matches(':focus-visible') && notch.state === 'sliver') go('pill');
});
hit.addEventListener('blur', () => {
  if (notch.state === 'pill') leaveTimer = setTimeout(() => go('sliver'), LEAVE_DELAY);
});
$('#panel-close').addEventListener('click', () => go('sliver', { focus: true }));
$('#panel-replay').addEventListener('click', () => runChat());
panel.tabIndex = -1;

addEventListener('keydown', (e) => {
  if (e.key === 'Escape' && notch.state !== 'sliver') {
    const inside = notchEl.contains(document.activeElement);
    go('sliver', { focus: inside || notch.state === 'panel' });
  }
});
addEventListener('pointerdown', (e) => {
  if (notch.state === 'sliver') return;
  if (e.target.closest('.notch-hit, .notch-panel, #peek-test')) return;
  if (notch.state === 'panel' || e.pointerType === 'touch') go('sliver');
});

let resizeQueued = false;
addEventListener('resize', () => {
  if (resizeQueued) return;
  resizeQueued = true;
  requestAnimationFrame(() => {
    resizeQueued = false;
    measure();
    notch.spring.setTarget(...notch.sizes[notch.state]);
    notch.spring.snap();
    drawNotch();
  });
});

function peek() {
  if (notch.state !== 'sliver') return;
  go('pill');
  peekTimer = setTimeout(() => {
    if (!hit.matches(':hover') && notch.state === 'pill') go('sliver');
  }, 1800);
}

measure();
drawNotch();
if (matchMedia('(hover: none)').matches) {
  hint.querySelector('[data-hint-text]').textContent = 'The notch up top is real. Tap it.';
}

/* ------------------------------------------------------------ Demo chat */

const SCRIPT = [
  { user: 'Turn this into a to-do list: email Sam back, dentist on Thursday, finish the draft' },
  { bot: 'Here you go:\n• Reply to Sam\n• Book the dentist for Thursday\n• Finish the draft' },
  { user: 'Make the first one sound friendlier.' },
  { bot: '• Send Sam a warm hello and catch up 🙂\n\nAnything else?' },
];
const chat = $('#chat');
const composer = $('#composer-text');
const placeholder = $('#composer-ph');
let chatRun = 0;
let chatBusy = false;

function status(text, mood) {
  for (const el of $$('[data-status]')) el.textContent = text;
  if (mood) for (const b of chatters) b.set(mood);
}
function bubble(kind, text = '') {
  const el = document.createElement('div');
  el.className = `msg ${kind}`;
  el.textContent = text;
  chat.append(el);
  chat.scrollTop = chat.scrollHeight;
  return el;
}

async function runChat() {
  const run = ++chatRun;
  const alive = () => run === chatRun;
  chat.replaceChildren();
  composer.textContent = '';
  placeholder.hidden = false;
  if (reduced()) {
    for (const step of SCRIPT) bubble(step.user ? 'user' : 'bot', step.user ?? step.bot);
    status('Ask anything', 'idle');
    return;
  }
  chatBusy = true;
  await sleep(500);
  for (const step of SCRIPT) {
    if (!alive()) return;
    if (step.user) {
      status('Listening…', 'typing');
      placeholder.hidden = true;
      for (const ch of step.user) {
        if (!alive()) return;
        composer.textContent += ch;
        await sleep(ch === ' ' ? 45 : 26);
      }
      await sleep(320);
      if (!alive()) return;
      composer.textContent = '';
      placeholder.hidden = false;
      bubble('user', step.user);
    } else {
      status('Thinking…', 'thinking');
      const el = bubble('bot');
      el.innerHTML = '<span class="typing" aria-label="Thinking"><i></i><i></i><i></i></span>';
      await sleep(1100);
      if (!alive()) return;
      status('Replying…', 'talking');
      el.textContent = '';
      const words = step.bot.split(/(\s+)/);
      for (const word of words) {
        if (!alive()) return;
        el.textContent += word;
        chat.scrollTop = chat.scrollHeight;
        await sleep(word.trim() ? 55 : 0);
      }
      status('All done', 'idle');
      for (const b of chatters) b.set('happy');
      await sleep(1400);
      if (!alive()) return;
      status('Ask anything');
    }
    await sleep(500);
  }
  chatBusy = false;
}

/* ------------------------------------------------------- Mini notches */

function miniNotch(el, w, h, inner = '') {
  el.style.width = `${w}px`;
  el.style.height = `${h + 1}px`;
  el.innerHTML = `<svg width="${w}" height="${h + 1}" viewBox="0 -1 ${w} ${h + 1}" aria-hidden="true">` +
    `<path class="notch-fill" d="${notchPath(w, h)}"/><path class="notch-line" d="${notchPath(w, h - 0.5, 0, false)}"/></svg>` +
    (inner ? `<div class="mini-inner">${inner}</div>` : '');
}
for (const el of $$('[data-mini]')) {
  const kind = el.dataset.mini;
  if (kind === 'sliver') miniNotch(el, 120, 5);
  if (kind === 'pill') {
    miniNotch(el, 240, 36, `<span class="mascot" data-mini-mascot style="--size:22px"></span><strong>bloop</strong><span class="muted" style="margin-left:auto">Ask anything</span>`);
    el.querySelector('.mini-inner').style.padding = '0 20px 3px 18px';
  }
  if (kind === 'panel') {
    miniNotch(el, 260, 128, `<div style="display:flex;flex-direction:column;gap:8px;width:100%;align-self:flex-start;padding-top:12px">
      <div style="display:flex;align-items:center;gap:6px"><span class="mascot" data-mini-mascot style="--size:20px"></span><strong>bloop</strong><span class="muted">Replying…</span></div>
      <span style="height:1px;background:var(--border);margin:0 -4px"></span>
      <span style="align-self:flex-end;background:var(--secondary);border:1px solid var(--border);border-radius:9px;padding:2px 8px">What's 15% of 80?</span>
      <span>That's 12.</span></div>`);
    el.querySelector('.mini-inner').style.padding = '0 20px';
  }
}
for (const el of $$('[data-mini-mascot]')) el.innerHTML = mascotSVG({ mood: el.closest('[data-mini="panel"]') ? 'talking' : 'idle', t: 0.12 });

/* ------------------------------------------------ Theme, accent, feel */

const THEMES = ['system', 'light', 'dark'];
const themeBtn = $('#theme-toggle');
const ICON = { system: 'monitor', light: 'sun', dark: 'moon' };
const LABEL = { system: 'follow system', light: 'light', dark: 'dark' };

function radios(name) {
  return $$(`[data-seg="${name}"] [role="radio"]`);
}
function check(name, value) {
  for (const r of radios(name)) {
    const on = r.dataset.value === value;
    r.setAttribute('aria-checked', String(on));
    r.tabIndex = on ? 0 : -1;
  }
}

function setTheme(theme, remember = true) {
  if (theme === 'system') delete root.dataset.theme;
  else root.dataset.theme = theme;
  if (remember) store.set('bloop-theme', theme === 'system' ? null : theme);
  themeBtn.querySelector('use').setAttribute('href', `#i-${ICON[theme]}`);
  themeBtn.setAttribute('aria-label', `Theme: ${LABEL[theme]}`);
  themeBtn.title = `Theme: ${LABEL[theme]}`;
  check('theme', theme);
}
function setAccent(accent, remember = true) {
  if (accent === 'neutral') delete root.dataset.accent;
  else root.dataset.accent = accent;
  if (remember) store.set('bloop-accent', accent === 'neutral' ? null : accent);
  check('accent', accent);
}
function setFeel(feel, remember = true) {
  notch.feel = FEEL[feel] ? feel : 'smooth';
  if (remember) store.set('bloop-feel', notch.feel);
  check('feel', notch.feel);
}

themeBtn.addEventListener('click', () => {
  const now = root.dataset.theme ?? 'system';
  setTheme(THEMES[(THEMES.indexOf(now) + 1) % THEMES.length]);
});

const SETTERS = { theme: setTheme, accent: setAccent, feel: (v) => { setFeel(v); peek(); } };
for (const group of $$('[data-seg]')) {
  const name = group.dataset.seg;
  group.addEventListener('click', (e) => {
    const r = e.target.closest('[role="radio"]');
    if (r) SETTERS[name](r.dataset.value);
  });
  group.addEventListener('keydown', (e) => {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!step) return;
    e.preventDefault();
    const list = radios(name);
    const i = list.findIndex((r) => r.getAttribute('aria-checked') === 'true');
    const next = list[(i + step + list.length) % list.length];
    SETTERS[name](next.dataset.value);
    next.focus();
  });
}
$('#peek-test').addEventListener('click', peek);

setTheme(root.dataset.theme ?? 'system', false);
setAccent(root.dataset.accent ?? 'neutral', false);
setFeel(notch.feel, false);

/* -------------------------------------------------- App window tabs */

const tabs = $$('.appwin-nav [role="tab"]');
function selectTab(tab, focus = false) {
  for (const t of tabs) {
    const on = t === tab;
    t.setAttribute('aria-selected', String(on));
    t.tabIndex = on ? 0 : -1;
    document.getElementById(t.getAttribute('aria-controls')).hidden = !on;
  }
  if (focus) tab.focus();
  wantFrame();
}
for (const t of tabs) t.addEventListener('click', () => selectTab(t));
$('.appwin-nav').addEventListener('keydown', (e) => {
  const i = tabs.indexOf(document.activeElement);
  if (i < 0) return;
  const to = { ArrowDown: i + 1, ArrowRight: i + 1, ArrowUp: i - 1, ArrowLeft: i - 1, Home: 0, End: tabs.length - 1 }[e.key];
  if (to === undefined) return;
  e.preventDefault();
  selectTab(tabs[(to + tabs.length) % tabs.length], true);
});

/* ------------------------------------------------ Final call mascot */

const finalCta = $('.final .btn');
if (finalCta && bloops.final) {
  finalCta.addEventListener('pointerenter', () => bloops.final.set('happy'));
  finalCta.addEventListener('focus', () => bloops.final.set('happy'));
}

/* ------------------------------------------------ Live download link */

const RELEASE_API = 'https://api.github.com/repos/LaurentMaxhuni/bloop/releases/latest';
const CACHE_KEY = 'bloop-release';
const CACHE_MS = 10 * 60 * 1000;

async function latestRelease() {
  try {
    const cached = JSON.parse(sessionStorage.getItem(CACHE_KEY) || 'null');
    if (cached && Date.now() - cached.at < CACHE_MS) return cached;
  } catch {}
  if (new URLSearchParams(location.search).has('offline')) throw new Error('offline (debug flag)');
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 6000);
  try {
    const res = await fetch(RELEASE_API, { signal: controller.signal, headers: { Accept: 'application/vnd.github+json' } });
    if (!res.ok) throw new Error(`GitHub API ${res.status}`);
    const json = await res.json();
    for (const asset of json.assets ?? []) {
      const m = /^bloop-setup-(.+)\.exe$/.exec(asset.name);
      if (m) {
        const release = { at: Date.now(), version: m[1], mb: (asset.size / 1048576).toFixed(1), url: asset.browser_download_url };
        try { sessionStorage.setItem(CACHE_KEY, JSON.stringify(release)); } catch {}
        return release;
      }
    }
    throw new Error('no installer asset');
  } finally {
    clearTimeout(timer);
  }
}

latestRelease().then((r) => {
  if (!/^https:\/\/github\.com\/LaurentMaxhuni\/bloop\/releases\/download\//.test(r.url)) return;
  for (const a of $$('[data-download]')) a.href = r.url;
  for (const m of $$('[data-download-meta]')) {
    m.textContent = `v${r.version} · ${r.mb} MB`;
    m.hidden = false;
  }
  for (const v of $$('[data-version]')) v.textContent = r.version;
}).catch(() => {
  // Keep the Releases page link and the plain label.
});

wantFrame();
