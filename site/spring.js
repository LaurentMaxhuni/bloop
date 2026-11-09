// Critically damped springs, a direct port of bloop's src/motion.rs: stepped by real frame
// time, so the notch moves the same at 60 Hz and 144 Hz and can change target mid-way.

/** Spring stiffness per feel; damping is always critical (2√k), so nothing overshoots. */
export const FEEL = Object.freeze({ snappy: 900, smooth: 400, calm: 180 });

/** Longest frame a step accounts for: after a stall the motion resumes, it doesn't jump. */
const MAX_DT = 1 / 30;
/** Integration sub-step, small enough to stay stable for the stiffest preset. */
const SUB_STEP = 1 / 240;

export class Spring {
  constructor(value) {
    this.value = value;
    this.velocity = 0;
    this.target = value;
  }

  step(k, dt) {
    let left = Math.min(Math.max(dt, 0), MAX_DT);
    const damping = 2 * Math.sqrt(k);
    while (left > 0) {
      const h = Math.min(left, SUB_STEP);
      // Semi-implicit Euler: velocity first, then position with the new velocity.
      const acceleration = k * (this.target - this.value) - damping * this.velocity;
      this.velocity += acceleration * h;
      this.value += this.velocity * h;
      left -= h;
    }
    if (this.settled()) this.snap();
  }

  settled() {
    return Math.abs(this.target - this.value) < 0.5 && Math.abs(this.velocity) < 5;
  }

  snap() {
    this.value = this.target;
    this.velocity = 0;
  }
}

/** The notch's drawn width and height. */
export class SizeSpring {
  constructor(w, h) {
    this.ws = new Spring(w);
    this.hs = new Spring(h);
  }
  setTarget(w, h) {
    this.ws.target = w;
    this.hs.target = h;
  }
  step(k, dt) {
    this.ws.step(k, dt);
    this.hs.step(k, dt);
  }
  settled() {
    return this.ws.settled() && this.hs.settled();
  }
  snap() {
    this.ws.snap();
    this.hs.snap();
  }
  get w() { return this.ws.value; }
  get h() { return this.hs.value; }
  get targetW() { return this.ws.target; }
  get targetH() { return this.hs.target; }
}
