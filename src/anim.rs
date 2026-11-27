//! Small animation helpers: easing curves and a critically damped spring.

/// Smoothstep between `a` and `b` (returns 0..1).
pub fn smooth(a: f64, b: f64, x: f64) -> f32 {
    if b <= a {
        return if x >= b { 1.0 } else { 0.0 };
    }
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    (t * t * (3.0 - 2.0 * t)) as f32
}

/// Ease-out cubic, 0..1 -> 0..1.
pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// A critically damped spring: no overshoot, settles fast, and a target can be
/// changed mid-flight without a visible jerk. `omega` is the stiffness (higher =
/// snappier); 2/omega is roughly how long it takes to get most of the way there.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub vel: f32,
    pub target: f32,
}

impl Spring {
    pub fn new(v: f32) -> Self {
        Spring { value: v, vel: 0.0, target: v }
    }

    pub fn snap(&mut self, v: f32) {
        self.value = v;
        self.target = v;
        self.vel = 0.0;
    }

    pub fn update(&mut self, dt: f32, omega: f32) {
        // Exact solution of the critically damped oscillator over one step,
        // so it is stable at any frame rate.
        let x = self.value - self.target;
        let exp = (-omega * dt).exp();
        let new_x = (x + (self.vel + omega * x) * dt) * exp;
        let new_v = (self.vel - omega * (self.vel + omega * x) * dt) * exp;
        self.value = self.target + new_x;
        self.vel = new_v;
        if (self.value - self.target).abs() < 1e-4 && self.vel.abs() < 1e-3 {
            self.value = self.target;
            self.vel = 0.0;
        }
    }
}
