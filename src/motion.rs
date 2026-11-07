//! Spring motion for the notch: critically damped springs stepped by real frame time, so
//! animations stay smooth at any refresh rate and can change target mid-way.

use serde::{Deserialize, Serialize};

/// How lively the notch moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Feel {
    Snappy,
    #[default]
    Smooth,
    Calm,
}

impl Feel {
    pub const ALL: [Feel; 3] = [Feel::Snappy, Feel::Smooth, Feel::Calm];

    /// Spring stiffness `k`; damping is always critical (`2√k`), so nothing overshoots.
    pub fn stiffness(self) -> f32 {
        match self {
            Feel::Snappy => 900.0,
            Feel::Smooth => 400.0,
            Feel::Calm => 180.0,
        }
    }
}

impl std::fmt::Display for Feel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Feel::Snappy => "Snappy",
            Feel::Smooth => "Smooth",
            Feel::Calm => "Calm",
        })
    }
}

/// Longest frame a step accounts for: after a stall the motion resumes, it doesn't jump.
const MAX_DT: f32 = 1.0 / 30.0;
/// Integration sub-step, small enough to stay stable for the stiffest preset.
const SUB_STEP: f32 = 1.0 / 240.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
}

impl Spring {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
        }
    }

    /// Advances by `dt` seconds (clamped) with stiffness `k` and critical damping.
    pub fn step(&mut self, k: f32, dt: f32) {
        let mut left = dt.clamp(0.0, MAX_DT);
        let damping = 2.0 * k.sqrt();
        while left > 0.0 {
            let h = left.min(SUB_STEP);
            // Semi-implicit Euler: velocity first, then position with the new velocity.
            let acceleration = k * (self.target - self.value) - damping * self.velocity;
            self.velocity += acceleration * h;
            self.value += self.velocity * h;
            left -= h;
        }
        if self.settled() {
            self.snap();
        }
    }

    pub fn settled(&self) -> bool {
        (self.target - self.value).abs() < 0.5 && self.velocity.abs() < 5.0
    }

    pub fn snap(&mut self) {
        self.value = self.target;
        self.velocity = 0.0;
    }
}

/// The notch's drawn width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizeSpring {
    pub w: Spring,
    pub h: Spring,
}

impl SizeSpring {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            w: Spring::new(width),
            h: Spring::new(height),
        }
    }

    pub fn set_target(&mut self, width: f32, height: f32) {
        self.w.target = width;
        self.h.target = height;
    }

    pub fn step(&mut self, k: f32, dt: f32) {
        self.w.step(k, dt);
        self.h.step(k, dt);
    }

    pub fn settled(&self) -> bool {
        self.w.settled() && self.h.settled()
    }

    pub fn snap(&mut self) {
        self.w.snap();
        self.h.snap();
    }

    pub fn size(&self) -> (f32, f32) {
        (self.w.value, self.h.value)
    }

    /// How far the height has travelled from `from` to `to`, 0 to 1.
    pub fn progress_h(&self, from: f32, to: f32) -> f32 {
        if (to - from).abs() < f32::EPSILON {
            return 1.0;
        }
        ((self.h.value - from) / (to - from)).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle_time(feel: Feel, hz: f32) -> (f32, f32) {
        let mut spring = Spring::new(5.0);
        spring.target = 580.0;
        let dt = 1.0 / hz;
        let (mut t, mut peak) = (0.0, 0.0_f32);
        while !spring.settled() {
            spring.step(feel.stiffness(), dt);
            peak = peak.max(spring.value);
            t += dt;
            assert!(t < 5.0, "{feel:?} never settled");
        }
        (t, peak)
    }

    #[test]
    fn converges_within_budget() {
        for (feel, budget) in [(Feel::Snappy, 0.6), (Feel::Smooth, 0.9), (Feel::Calm, 1.4)] {
            for hz in [60.0, 144.0] {
                let (t, peak) = settle_time(feel, hz);
                assert!(t <= budget, "{feel:?} at {hz} Hz took {t:.2}s");
                assert!(peak <= 580.5, "{feel:?} overshot to {peak}");
            }
        }
    }

    #[test]
    fn reversal_keeps_velocity() {
        let k = Feel::Smooth.stiffness();
        let mut spring = Spring::new(5.0);
        spring.target = 580.0;
        let mut previous = spring.value;
        while spring.value < 290.0 {
            previous = spring.value;
            spring.step(k, 1.0 / 60.0);
        }
        let last_step = spring.value - previous;
        let before = spring.value;
        spring.target = 5.0;
        spring.step(k, 1.0 / 60.0);
        let next_step = spring.value - before;
        // Momentum carries it on briefly instead of snapping back.
        assert!(next_step > 0.0, "jumped backwards: {next_step}");
        assert!(next_step < last_step * 1.5);
    }

    #[test]
    fn huge_dt_is_clamped() {
        let mut spring = Spring::new(5.0);
        spring.target = 580.0;
        spring.step(Feel::Snappy.stiffness(), 0.5);
        assert!(spring.value < 580.0 * 1.1, "{}", spring.value);
        assert!(spring.value > 5.0);
    }

    #[test]
    fn snap_jumps() {
        let mut spring = Spring::new(5.0);
        spring.target = 300.0;
        spring.velocity = 50.0;
        spring.snap();
        assert_eq!(spring.value, 300.0);
        assert_eq!(spring.velocity, 0.0);
        assert!(spring.settled());
    }

    #[test]
    fn size_spring_progress() {
        let mut size = SizeSpring::new(120.0, 5.0);
        size.set_target(540.0, 580.0);
        assert_eq!(size.progress_h(5.0, 580.0), 0.0);
        while !size.settled() {
            size.step(Feel::Smooth.stiffness(), 1.0 / 60.0);
        }
        assert_eq!(size.size(), (540.0, 580.0));
        assert_eq!(size.progress_h(5.0, 580.0), 1.0);
    }
}
