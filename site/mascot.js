// Bloop, drawn as SVG from the same 24-unit grid as the app's canvas (src/mascot.rs).
// Body uses currentColor, the face uses --mascot-face, cheeks use --blush.

const TAU = Math.PI * 2;
const ANIMATED = new Set(['thinking', 'talking', 'happy', 'oops', 'perk']);
export const isAnimated = (mood) => ANIMATED.has(mood);

const f = (n) => Math.round(n * 100) / 100;

/**
 * @param {{mood?: string, blink?: boolean, look?: [number, number], t?: number, since?: number}} o
 * @returns {string} an <svg> string
 */
export function mascotSVG({ mood = 'idle', blink = false, look = [0, 0], t = 0, since = 0 } = {}) {
  let dx = 0, dy = 0, stretch = 0;
  switch (mood) {
    case 'thinking':
      dy = Math.sin(t * TAU * 0.8) * 0.7;
      dx = Math.sin(t * TAU * 0.4) * 0.5;
      break;
    case 'talking': {
      const beat = Math.abs(Math.sin(t * TAU * 2.2));
      dy = -beat * 0.8;
      stretch = beat * 0.6;
      break;
    }
    case 'happy': {
      const hop = Math.abs(Math.sin(since * TAU * 1.6)) * Math.max(1 - since / 1.6, 0);
      dy = -hop * 2.2;
      stretch = hop * 0.8;
      break;
    }
    case 'oops':
      dx = Math.sin(since * TAU * 7) * 1.1 * Math.max(1 - since / 0.7, 0);
      break;
    case 'sleepy':
      dy = 0.6;
      break;
    case 'perk': {
      const hop = Math.abs(Math.sin(since * TAU * 0.85)) * Math.max(1 - since / 0.6, 0);
      dy = -hop * 1.6;
      stretch = hop * 0.5;
      break;
    }
  }
  const X = (x) => f(x + dx);
  const Y = (y) => f(y + dy);
  const P = (x, y) => `${X(x)} ${Y(y)}`;
  const parts = [];

  // Antenna sprout.
  const tip = mood === 'sleepy' ? [14.6, 2.6] : [14.0, 1.6];
  parts.push(`<path class="mb-s" stroke-width="1.3" d="M${P(12, 5 - stretch)}L${P(...tip)}"/>`);
  parts.push(`<circle class="mb" cx="${X(tip[0])}" cy="${Y(tip[1])}" r="1.5"/>`);

  // Body: a soft rounded square that stretches when excited.
  const top = 5 - stretch;
  parts.push(`<rect class="mb" x="${X(2.5)}" y="${Y(top)}" width="19" height="${f(21.5 - top)}" rx="8"/>`);

  // Where the eyes look.
  let [lx, ly] = look;
  if (mood === 'typing' && !lx && !ly) [lx, ly] = [0.5, 1];
  if (mood === 'thinking') [lx, ly] = [0.9, -1.1];

  const eyeY = 12.5;
  const eyes = [8.5, 15.5];
  const line = (w, d) => `<path class="mf-s" stroke-width="${w}" d="${d}"/>`;
  if (mood === 'happy') {
    for (const x of eyes) parts.push(line(1.4, `M${P(x - 1.8, eyeY + 0.8)}Q${P(x, eyeY - 1.9)} ${P(x + 1.8, eyeY + 0.8)}`));
  } else if (mood === 'oops') {
    for (const [x, d] of [[eyes[0], 1], [eyes[1], -1]]) {
      parts.push(line(1.3, `M${P(x - 1.3 * d, eyeY - 1.4)}L${P(x + 1.2 * d, eyeY)}L${P(x - 1.3 * d, eyeY + 1.4)}`));
    }
  } else if (mood === 'sleepy') {
    for (const x of eyes) parts.push(line(1.2, `M${P(x - 1.7, eyeY)}Q${P(x, eyeY + 1.4)} ${P(x + 1.7, eyeY)}`));
  } else if (blink) {
    for (const x of eyes) parts.push(line(1.2, `M${P(x - 1.6, eyeY)}L${P(x + 1.6, eyeY)}`));
  } else {
    const r = mood === 'perk' ? 2.3 : 1.9;
    for (const x of eyes) {
      parts.push(`<circle class="mf" cx="${X(x + lx)}" cy="${Y(eyeY + ly)}" r="${r}"/>`);
      // Tiny highlight makes the eyes read as shiny.
      parts.push(`<circle class="mb" cx="${X(x + lx + 0.6)}" cy="${Y(eyeY + ly - 0.7)}" r="0.55"/>`);
    }
  }

  // Blush on happy faces.
  if (mood === 'happy') {
    parts.push(`<circle class="mk" cx="${X(5.6)}" cy="${Y(15.6)}" r="1.3"/><circle class="mk" cx="${X(18.4)}" cy="${Y(15.6)}" r="1.3"/>`);
  }

  // Mouth.
  const my = 16.6;
  switch (mood) {
    case 'talking': {
      const open = 0.5 + Math.abs(Math.sin(t * TAU * 2.2)) * 1.8;
      parts.push(`<rect class="mf" x="${X(10.6)}" y="${Y(my - 0.6)}" width="2.8" height="${f(open)}" rx="${f(Math.min(1.2, open / 2))}"/>`);
      break;
    }
    case 'happy':
      parts.push(`<path class="mf" d="M${P(9.8, my - 0.4)}Q${P(12, my + 2.6)} ${P(14.2, my - 0.4)}Z"/>`);
      break;
    case 'oops':
      parts.push(line(1.1, `M${P(9.8, my + 0.6)}L${P(10.9, my - 0.2)}L${P(12, my + 0.6)}L${P(13.1, my - 0.2)}L${P(14.2, my + 0.6)}`));
      break;
    case 'thinking':
      parts.push(line(1.2, `M${P(11.2, my + 0.3)}L${P(13.4, my)}`));
      break;
    case 'sleepy':
      parts.push(`<circle class="mf" cx="${X(12)}" cy="${Y(my + 0.4)}" r="0.8"/>`);
      // A little "z" drifting off the antenna.
      parts.push(`<path class="mb-s" stroke-width="0.9" d="M${P(18.2, 1)}L${P(20.6, 1)}L${P(18.2, 3.4)}L${P(20.6, 3.4)}"/>`);
      break;
    case 'perk':
      parts.push(`<circle class="mf" cx="${X(12)}" cy="${Y(my + 0.3)}" r="1"/>`);
      break;
    default:
      parts.push(line(1.2, `M${P(10.4, my)}Q${P(12, my + 1.5)} ${P(13.6, my)}`));
  }

  return `<svg class="mascot-svg" viewBox="-1 -2.5 26 26" aria-hidden="true" focusable="false">${parts.join('')}</svg>`;
}

/** Eye offset toward a pointer, as in the app: up to 1.2 units, scaled by distance. */
export function lookToward(rect, px, py) {
  const side = Math.min(rect.width, rect.height);
  const vx = px - (rect.left + rect.width / 2);
  const vy = py - (rect.top + rect.height / 2);
  const len = Math.max(Math.hypot(vx, vy), 1);
  const reach = Math.min(len / (side * 2), 1) * 1.2;
  return [(vx / len) * reach, (vy / len) * reach];
}
