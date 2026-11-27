//! Fan Curve Tuner, native: the "ECU tuning" bar chart from fan_curve_tuner.py.
//!
//! Each column is one speed step; the x labels between columns are the
//! temperature thresholds. Colour layering keeps the original's meaning:
//!   light teal  0 .. min(tune, default)   the part both curves share
//!   dark teal   default .. tune           tuned ABOVE the MSI factory curve
//!   red         tune .. default           tuned BELOW it (quieter, hotter)
//! The orange box is the step the fan is on right now; lime = selected column.
//!
//! EC reads and Apply run on worker threads (pkexec waits for the user), so
//! the animation never stalls.

use crate::anim::Spring;
use crate::draw::{AMBER, Align, LIME, RED, TEAL, TXT, TXT_DIM, Ui, hit};
use crate::dyno::Tele;
use crate::ec::{self, Curve, FACTORY, N_SPEEDS, N_TEMPS, PRESETS, RPM_PER_PCT, SPEED_MAX, TEMP_MAX, TEMP_MIN};
use macroquad::prelude::*;
use std::sync::mpsc::{Receiver, channel};

pub const HELP: &str = "Each column is one fan step; its height is the fan speed. Click or drag a column to set it; drag a temperature label sideways (or scroll on it) to move that threshold. Keys: Left/Right pick a column, Up/Down ±5 % (Shift ±1 %), comma and period move the threshold at the column's right edge, Tab CPU/GPU table. Red = below the MSI factory curve, dark teal = above it, the orange box is the step the fan is on now. Speeds stop at 100 %: that is already the 6000 RPM ceiling. The fan runs at whichever table asks for more. Note: an open MControlCenter window keeps showing its old values; pressing Apply there would undo this tune.";

const ROW_STEP: i32 = 20;
const BAR_TEAL: Color = Color::new(0.29, 0.63, 0.61, 0.88);
const BAR_TEAL_DARK: Color = Color::new(0.18, 0.42, 0.49, 0.92);
const BAR_RED: Color = Color::new(0.59, 0.15, 0.15, 0.90);
const NOW: Color = Color::new(1.0, 0.35, 0.24, 1.0);

/// Things the tuner asks the app to do.
pub enum TEvent {
    Toast(String, Color),
    Message(String),
    ConfirmApply,
    ConfirmLeave,
    Back,
    Sound(crate::sound::Fx),
}

enum Work {
    Load(Receiver<Result<(Curve, Curve, u8), String>>),
    Apply(Receiver<Result<ec::ApplyReport, String>>),
    Mode(Receiver<Result<(), String>>),
}

pub struct Tuner {
    pub loaded: bool,
    pub error: Option<String>,
    saved: [Curve; 2],
    curves: [Curve; 2],
    pub tab: usize,
    sel: usize,
    linked: bool,
    preset: usize,
    work: Option<Work>,
    bars: [Spring; N_SPEEDS],
    /// CPU/GPU tiles slide under a fixed centre box, like the menu carousels
    tab_slide: Spring,
    drag_col: bool,
    drag_label: Option<(usize, f32, i32)>,
    hover_label: Option<usize>,
    changed_at: f64,
    // hit boxes from the last frame
    plot: Rect,
    tiles: [Rect; 2],
    arrows: [Rect; 2],
}

fn base_at(c: &Curve, n: usize) -> i32 {
    // default speed for column n, judged at the temperature where the column
    // starts, so it stays fair when the thresholds differ from the default's
    let t = if n == 0 { c.t[0] - 1 } else { c.t[n - 1] };
    let k = FACTORY.t.iter().filter(|&&bt| t >= bt).count();
    FACTORY.s[k]
}

fn range_text(c: &Curve, n: usize) -> String {
    if n == 0 {
        format!("below {}°C", c.t[0])
    } else if n == N_SPEEDS - 1 {
        format!("{}°C and up", c.t[N_TEMPS - 1])
    } else {
        format!("{}–{}°C", c.t[n - 1], c.t[n])
    }
}


impl Tuner {
    pub fn new() -> Tuner {
        Tuner {
            loaded: false,
            error: None,
            saved: [FACTORY; 2],
            curves: [FACTORY; 2],
            tab: 0,
            sel: 0,
            linked: true,
            preset: 0,
            work: None,
            bars: [Spring::new(0.0); N_SPEEDS],
            tab_slide: Spring::new(0.0),
            drag_col: false,
            drag_label: None,
            hover_label: None,
            changed_at: -10.0,
            plot: Rect::default(),
            tiles: [Rect::default(); 2],
            arrows: [Rect::default(); 2],
        }
    }

    /// Read the curves from the EC (on a worker thread).
    pub fn load(&mut self) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(ec::read_curves());
        });
        self.work = Some(Work::Load(rx));
    }

    pub fn busy(&self) -> bool {
        self.work.is_some()
    }

    pub fn dirty(&self) -> bool {
        self.loaded && self.effective() != self.saved
    }

    /// The pair that Apply would write (GPU = CPU while linked).
    fn effective(&self) -> [Curve; 2] {
        if self.linked { [self.curves[0], self.curves[0]] } else { self.curves }
    }

    fn cur(&self) -> &Curve {
        if self.tab == 1 && self.linked { &self.curves[0] } else { &self.curves[self.tab] }
    }

    fn cur_mut(&mut self) -> &mut Curve {
        let i = if self.linked { 0 } else { self.tab };
        &mut self.curves[i]
    }

    pub fn start_apply(&mut self) {
        let [c1, c2] = self.effective();
        let mirror = self.linked;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(ec::apply(c1, c2, mirror));
        });
        self.work = Some(Work::Apply(rx));
    }

    /// Poll worker threads. Call every frame.
    pub fn poll(&mut self) -> Vec<TEvent> {
        let mut ev = Vec::new();
        let Some(work) = self.work.take() else { return ev };
        match work {
            Work::Load(rx) => match rx.try_recv() {
                Ok(Ok((c1, c2, _mode))) => {
                    self.saved = [c1, c2];
                    self.curves = [c1.clamped(), c2.clamped()];
                    self.linked = c1 == c2;
                    self.loaded = true;
                    self.error = None;
                }
                Ok(Err(e)) => {
                    self.error = Some(e);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.work = Some(Work::Load(rx)),
                Err(_) => self.error = Some("EC read thread died".into()),
            },
            Work::Apply(rx) => match rx.try_recv() {
                Ok(Ok(rep)) => {
                    let mut notes: Vec<String> = Vec::new();
                    notes.extend(rep.conf_error);
                    notes.extend(rep.etc_note);
                    if notes.is_empty() {
                        ev.push(TEvent::Toast("Applied: EC, MControlCenter.conf and boot fallback in sync".into(), LIME));
                    } else {
                        ev.push(TEvent::Message(format!("Applied.  {}", notes.join("  "))));
                    }
                    ev.push(TEvent::Sound(crate::sound::Fx::Done));
                    self.load();
                }
                Ok(Err(e)) => {
                    ev.push(TEvent::Sound(crate::sound::Fx::Error));
                    ev.push(TEvent::Message(e));
                    self.load();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.work = Some(Work::Apply(rx)),
                Err(_) => ev.push(TEvent::Message("Apply thread died".into())),
            },
            Work::Mode(rx) => match rx.try_recv() {
                Ok(Ok(())) => ev.push(TEvent::Toast("Fan mode set to advanced".into(), LIME)),
                Ok(Err(e)) => ev.push(TEvent::Message(e)),
                Err(std::sync::mpsc::TryRecvError::Empty) => self.work = Some(Work::Mode(rx)),
                Err(_) => {}
            },
        }
        ev
    }

    fn set_speed(&mut self, n: usize, s: i32) -> bool {
        let s = s.clamp(0, SPEED_MAX);
        let c = self.cur_mut();
        if c.s[n] != s {
            c.s[n] = s;
            return true;
        }
        false
    }

    fn set_temp(&mut self, i: usize, t: i32) -> bool {
        let c = *self.cur();
        let lo = if i > 0 { c.t[i - 1] + 1 } else { TEMP_MIN };
        let hi = if i < N_TEMPS - 1 { c.t[i + 1] - 1 } else { TEMP_MAX };
        let t = t.clamp(lo, hi);
        let c = self.cur_mut();
        if c.t[i] != t {
            c.t[i] = t;
            return true;
        }
        false
    }

    fn select(&mut self, n: usize, now: f64) -> bool {
        if n != self.sel {
            self.sel = n;
            self.changed_at = now;
            return true;
        }
        false
    }

    fn switch_tab(&mut self, t: usize, now: f64) {
        if self.tab != t {
            self.tab = t;
            self.changed_at = now;
        }
    }

    fn load_preset(&mut self, idx: usize) {
        self.preset = idx % PRESETS.len();
        let c = PRESETS[self.preset].1;
        *self.cur_mut() = c;
    }

    fn revert(&mut self) {
        self.curves = [self.saved[0].clamped(), self.saved[1].clamped()];
        self.linked = self.saved[0] == self.saved[1];
    }

    /// Flip between the CPU and GPU tables; the arrow on that side pops.
    pub fn toggle_tab(&mut self, ui: &Ui, now: f64) {
        let to = 1 - self.tab;
        ui.press(if to == 0 { "tab_l" } else { "tab_r" });
        self.switch_tab(to, now);
    }

    pub fn linked(&self) -> bool {
        self.linked
    }

    /// 0 link, 1 next preset, 2 revert, 3 factory default, 4 apply.
    pub fn pill_action(&mut self, i: usize, ev: &mut Vec<TEvent>) {
        use crate::sound::Fx;
        match i {
            0 => {
                self.linked = !self.linked;
                if self.linked {
                    self.curves[1] = self.curves[0];
                }
                ev.push(TEvent::Sound(Fx::Tick));
                ev.push(TEvent::Toast(if self.linked { "GPU table now copies the CPU table".into() } else { "GPU table unlinked: edit it on its own".into() }, LIME));
            }
            1 => {
                let next = self.preset + 1;
                self.load_preset(next);
                ev.push(TEvent::Sound(Fx::Tick));
                ev.push(TEvent::Toast(format!("Preset: {} (press Apply to use it)", PRESETS[self.preset].0), LIME));
            }
            2 => {
                self.revert();
                ev.push(TEvent::Sound(Fx::Back));
                ev.push(TEvent::Toast("Back to the curve in the EC".into(), TXT));
            }
            3 => {
                *self.cur_mut() = FACTORY;
                self.preset = 0;
                ev.push(TEvent::Sound(Fx::Tick));
                ev.push(TEvent::Toast("MSI factory curve loaded (press Apply to use it)".into(), LIME));
            }
            _ => {
                if self.dirty() {
                    ev.push(TEvent::ConfirmApply);
                } else {
                    ev.push(TEvent::Sound(Fx::Error));
                    ev.push(TEvent::Toast("Nothing to apply: the EC already has this curve".into(), TXT_DIM));
                }
            }
        }
    }

    /// Demo mode only: an in-memory edit (never applied) to show the colours.
    pub fn demo_nudge(&mut self, now: f64) {
        if !self.loaded {
            return;
        }
        self.select(2, now);
        let v = self.cur().s[2] - 20;
        self.set_speed(2, v);
        self.select(4, now);
        let v = self.cur().s[4] + 10;
        self.set_speed(4, v);
    }

    pub fn demo_tab(&mut self, now: f64) {
        self.switch_tab(1 - self.tab, now);
    }

    pub fn fix_mode(&mut self) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(ec::force_advanced());
        });
        self.work = Some(Work::Mode(rx));
    }

    /// Keyboard + mouse. `chars` are typed characters this frame.
    pub fn input(&mut self, ui: &Ui, tele: &Tele, now: f64, rep: &mut dyn FnMut(KeyCode) -> bool) -> Vec<TEvent> {
        use crate::sound::Fx;
        let mut ev = Vec::new();
        if !self.loaded || self.busy() {
            if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Backspace) {
                ev.push(TEvent::Back);
            }
            return ev;
        }
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        let step = if shift { 1 } else { 5 };
        let editable = !(self.tab == 1 && self.linked);

        if rep(KeyCode::Left) && self.select(self.sel.saturating_sub(1), now) {
            ev.push(TEvent::Sound(Fx::Tick));
        }
        if rep(KeyCode::Right) && self.select((self.sel + 1).min(N_SPEEDS - 1), now) {
            ev.push(TEvent::Sound(Fx::Tick));
        }
        if editable {
            let n = self.sel;
            if rep(KeyCode::Up) {
                let v = self.cur().s[n] + step;
                ev.push(TEvent::Sound(if self.set_speed(n, v) { Fx::Tick } else { Fx::Error }));
            }
            if rep(KeyCode::Down) {
                let v = self.cur().s[n] - step;
                ev.push(TEvent::Sound(if self.set_speed(n, v) { Fx::Tick } else { Fx::Error }));
            }
            // , and . move the threshold at the selected column's right edge
            if n < N_TEMPS {
                if rep(KeyCode::Comma) {
                    let v = self.cur().t[n] - 1;
                    ev.push(TEvent::Sound(if self.set_temp(n, v) { Fx::Tick } else { Fx::Error }));
                }
                if rep(KeyCode::Period) {
                    let v = self.cur().t[n] + 1;
                    ev.push(TEvent::Sound(if self.set_temp(n, v) { Fx::Tick } else { Fx::Error }));
                }
            }
        }
        if is_key_pressed(KeyCode::Tab) {
            self.toggle_tab(ui, now);
            ev.push(TEvent::Sound(Fx::Whoosh));
        }
        if is_key_pressed(KeyCode::L) {
            self.pill_action(0, &mut ev);
        }
        if is_key_pressed(KeyCode::P) {
            self.pill_action(1, &mut ev);
        }
        if is_key_pressed(KeyCode::R) {
            self.pill_action(2, &mut ev);
        }
        if is_key_pressed(KeyCode::D) {
            self.pill_action(3, &mut ev);
        }
        if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
            self.pill_action(4, &mut ev);
        }
        if is_key_pressed(KeyCode::F) && tele.fan_mode != "advanced" && !tele.fan_mode.is_empty() {
            self.fix_mode();
        }
        if is_key_pressed(KeyCode::H) {
            ev.push(TEvent::Message(HELP.into()));
        }
        if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Backspace) {
            ev.push(if self.dirty() { TEvent::ConfirmLeave } else { TEvent::Back });
        }

        // ---- mouse
        let m = ui.mouse();
        let p = self.plot;
        let cw = p.w / N_SPEEDS as f32;
        let label_rect = |i: usize| Rect::new(p.x + (i + 1) as f32 * cw - 26.0, p.y + p.h + 4.0, 52.0, 26.0);
        let label_at = |pt: Vec2| (0..N_TEMPS).find(|&i| hit(&label_rect(i), pt));
        let col_at = |pt: Vec2| if hit(&p, pt) { Some((((pt.x - p.x) / cw) as usize).min(N_SPEEDS - 1)) } else { None };
        let s_of = |y: f32| ((p.y + p.h - y) / p.h * SPEED_MAX as f32).round() as i32;

        if is_mouse_button_pressed(MouseButton::Left) {
            for i in 0..2 {
                if hit(&self.tiles[i], m) && self.tab != i {
                    self.switch_tab(i, now);
                    ev.push(TEvent::Sound(Fx::Whoosh));
                }
            }
            let arrows = self.arrows;
            for (i, r) in arrows.iter().enumerate() {
                if hit(r, m) {
                    ui.press(if i == 0 { "tab_l" } else { "tab_r" });
                    if self.tab != i {
                        self.switch_tab(i, now);
                        ev.push(TEvent::Sound(Fx::Whoosh));
                    }
                }
            }
            if editable {
                if let Some(li) = label_at(m) {
                    self.drag_label = Some((li, m.x, self.cur().t[li]));
                } else if let Some(n) = col_at(m) {
                    self.select(n, now);
                    self.drag_col = true;
                    self.set_speed(n, s_of(m.y));
                    ev.push(TEvent::Sound(Fx::Tick));
                }
            } else if let Some(n) = col_at(m) {
                self.select(n, now);
            }
        }
        if is_mouse_button_down(MouseButton::Left) {
            if let Some((i, x0, t0)) = self.drag_label {
                let t = t0 + ((m.x - x0) / 7.0).round() as i32;
                if self.set_temp(i, t) {
                    ev.push(TEvent::Sound(Fx::Tick));
                }
            } else if self.drag_col {
                let n = self.sel;
                self.set_speed(n, s_of(m.y));
            }
        } else {
            self.drag_col = false;
            self.drag_label = None;
        }
        self.hover_label = if editable { label_at(m) } else { None };
        let (_, wy) = mouse_wheel();
        if wy != 0.0 && editable {
            let d = if wy > 0.0 { 1 } else { -1 };
            if let Some(li) = label_at(m) {
                let v = self.cur().t[li] + d;
                self.set_temp(li, v);
            } else if let Some(n) = col_at(m) {
                self.select(n, now);
                let v = self.cur().s[n] + d * 5;
                self.set_speed(n, v);
            }
        }
        ev
    }

    pub fn draw(&mut self, ui: &Ui, tele: &Tele, dt: f32, hl: f32) {
        let now = ui.t;
        // header: title + live readings
        ui.text_sh("Fan Curve Tuner", 34.0, 62.0, 46.0, LIME, Align::Left);
        let cpu = tele.cpu_t.map(|v| format!("{v:.0}°C")).unwrap_or("?".into());
        let gpu = match tele.gpu_t {
            Some(g) if g > 0.0 => format!("{g:.0}°C"),
            _ => "asleep".into(),
        };
        let rpm = tele.rpm.map(|v| format!("{v:.0}")).unwrap_or("?".into());
        ui.text_sh(&format!("CPU {cpu}   ·   GPU {gpu}   ·   {rpm} RPM"), ui.w - 34.0, 56.0, 22.0, TXT, Align::Right);

        if let Some(e) = &self.error {
            let t = ui.fit(e, 22.0, ui.w - 120.0);
            ui.text_sh("Could not read the fan curves", ui.w * 0.5, 320.0, 32.0, RED, Align::Center);
            ui.text_sh(&t, ui.w * 0.5, 360.0, 22.0, TXT, Align::Center);
            return;
        }
        if !self.loaded {
            ui.text_sh("Reading the EC…", ui.w * 0.5, 340.0, 30.0, LIME, Align::Center);
            return;
        }

        // position track: which column is selected
        let (tx0, tx1, ty) = (24.0, ui.w - 24.0, 74.0);
        ui.rrect(tx0, ty, tx1 - tx0, 9.0, 4.5, Color::new(0.65, 0.68, 0.68, 0.30));
        let dx = tx0 + 10.0 + (tx1 - tx0 - 20.0) * self.sel as f32 / (N_SPEEDS - 1) as f32;
        ui.disc(dx, ty + 4.5, 4.2, TXT);

        // table selector: CPU / GPU tiles in a strip
        let (sy, sh) = (90.0, 62.0);
        ui.rrect_grad(24.0, sy, ui.w - 48.0, sh, 12.0, Color::new(0.62, 0.65, 0.65, 0.40), Color::new(0.45, 0.48, 0.48, 0.38));
        let m = ui.mouse();
        let la = Rect::new(58.0 - 18.0, sy + sh * 0.5 - 18.0, 36.0, 36.0);
        let ra = Rect::new(ui.w - 58.0 - 18.0, sy + sh * 0.5 - 18.0, 36.0, 36.0);
        ui.arrow_btn("tab_l", 58.0, sy + sh * 0.5, 17.0, -1, hit(&la, m), self.tab == 1);
        ui.arrow_btn("tab_r", ui.w - 58.0, sy + sh * 0.5, 17.0, 1, hit(&ra, m), self.tab == 0);
        self.arrows = [la, ra];
        let pulse = 0.5 + 0.5 * ((now - self.changed_at) as f32 * std::f32::consts::TAU / 1.25).cos();
        let labels = [('\u{F0EE0}', "CPU  ·  table 1"), ('\u{F08AE}', "GPU  ·  table 2")];
        // the box never moves: the selected tile slides under it, the other
        // one waits a slot away (the end of the list is just an empty slot)
        self.tab_slide.target = self.tab as f32;
        self.tab_slide.update(dt, 17.0);
        let (tw, gap) = (250.0, 290.0);
        let bx = ui.w * 0.5 - tw * 0.5;
        let by = sy + 7.0;
        ui.rrect(bx, by, tw, sh - 14.0, 10.0, Color::new(0.10, 0.11, 0.11, 0.70));
        ui.rrect_line(bx - 1.0, by - 1.0, tw + 2.0, sh - 12.0, 11.0, 2.5, Color::new(LIME.r, LIME.g, LIME.b, hl * (0.35 + 0.65 * pulse)));
        for (i, (g, l)) in labels.iter().enumerate() {
            let d = i as f32 - self.tab_slide.value;
            let x = bx + d * gap;
            let r = Rect::new(x, by, tw, sh - 14.0);
            let near = 1.0 - d.abs().min(1.0); // 1 under the box, 0 a slot away
            let a = 0.7 + 0.3 * near;
            let col = Color::new(0.85 + 0.10 * near, 0.87 + 0.09 * near, 0.88 + 0.08 * near, a);
            let lab = Color::new(col.r + (LIME.r - col.r) * near, col.g + (LIME.g - col.g) * near, col.b + (LIME.b - col.b) * near, a);
            ui.icon(*g, r.x + 34.0, r.y + r.h * 0.5, 32.0, col);
            ui.text_sh(l, r.x + 64.0, r.y + 31.0, 22.0, lab, Align::Left);
            self.tiles[i] = r;
        }

        // ---- the chart
        let c = *self.cur();
        let px0 = 96.0;
        let px1 = ui.w - 96.0;
        let py0 = 178.0;
        let py1 = ui.h - crate::FOOTER_H - 92.0;
        let p = Rect::new(px0, py0, px1 - px0, py1 - py0);
        self.plot = p;
        let cw = p.w / N_SPEEDS as f32;
        let y_of = |s: f32| p.y + p.h - s / SPEED_MAX as f32 * p.h;
        for (n, b) in self.bars.iter_mut().enumerate() {
            b.target = c.s[n] as f32;
            b.update(dt, 18.0);
        }
        ui.rect(p.x, p.y, p.w, p.h, Color::new(0.04, 0.047, 0.055, 0.66));
        let rows = (SPEED_MAX / ROW_STEP) as usize;
        for n in 0..N_SPEEDS {
            for k in 0..rows {
                if (n + k) % 2 == 0 {
                    let (y0, y1) = (y_of(((k + 1) as i32 * ROW_STEP) as f32), y_of((k as i32 * ROW_STEP) as f32));
                    ui.rect(p.x + n as f32 * cw, y0, cw, y1 - y0, Color::new(0.0, 0.0, 0.0, 0.43));
                }
            }
        }
        for n in 0..N_SPEEDS {
            let x = p.x + n as f32 * cw;
            let tune = self.bars[n].value;
            let base = base_at(&c, n) as f32;
            let lo = tune.min(base);
            let band = |a: f32, b: f32, col: Color| {
                if b > a {
                    ui.rect(x, y_of(b), cw, y_of(a) - y_of(b), col);
                }
            };
            band(0.0, lo, BAR_TEAL);
            band(base, tune, BAR_TEAL_DARK);
            band(tune, base, BAR_RED);
        }
        let grid = Color::new(1.0, 1.0, 1.0, 0.15);
        for k in 0..=rows {
            let y = y_of((k as i32 * ROW_STEP) as f32);
            ui.line(p.x, y, p.x + p.w, y, 1.0, grid);
        }
        for n in 0..=N_SPEEDS {
            let x = p.x + n as f32 * cw;
            ui.line(x, p.y, x, p.y + p.h, 1.0, grid);
        }
        let linked_view = self.tab == 1 && self.linked;
        // live column: the step the fan is on now
        let live = if self.tab == 0 { tele.cpu_t } else { tele.gpu_t.filter(|&g| g > 0.0) };
        if let (Some(lt), false) = (live, linked_view) {
            let act = c.t.iter().filter(|&&t| lt >= t as f32).count();
            let x = p.x + act as f32 * cw;
            ui.rrect_line(x + 1.0, p.y + 1.0, cw - 2.0, p.h - 2.0, 2.0, 2.0, NOW);
            ui.text_sh(&format!("NOW {lt:.0}°"), x + cw * 0.5, p.y + 20.0, 16.0, NOW, Align::Center);
        }
        // selected column
        let sx = p.x + self.sel as f32 * cw;
        let since = (now - self.changed_at) as f32;
        let flash = (1.0 - since / 0.25).clamp(0.0, 1.0);
        if flash > 0.0 {
            ui.rect(sx, p.y, cw, p.h, Color::new(LIME.r, LIME.g, LIME.b, 0.12 * flash));
        }
        ui.rrect_line(sx, p.y, cw, p.h, 1.0, 2.0, Color::new(LIME.r, LIME.g, LIME.b, 0.55 + 0.45 * pulse));
        // dots
        for n in 0..N_SPEEDS {
            let x = p.x + (n as f32 + 0.5) * cw;
            let y = y_of(self.bars[n].value);
            let sel = n == self.sel;
            ui.disc(x, y, if sel { 7.5 } else { 6.5 }, Color::new(0.0, 0.0, 0.0, 0.5));
            ui.disc(x, y, if sel { 6.0 } else { 5.0 }, if sel { Color::new(0.86, 0.94, 0.36, 1.0) } else { Color::new(0.96, 0.96, 0.96, 1.0) });
        }
        // axes
        for k in 0..=rows {
            let s = k as i32 * ROW_STEP;
            let y = y_of(s as f32);
            ui.text_sh(&s.to_string(), p.x - 12.0, y + 7.0, 18.0, TXT, Align::Right);
            let rpm_k = (s * RPM_PER_PCT) as f32 / 1000.0;
            let txt = if rpm_k.fract() == 0.0 { format!("{rpm_k:.0}") } else { format!("{rpm_k:.1}") };
            ui.text_sh(&txt, p.x + p.w + 12.0, y + 7.0, 18.0, TXT_DIM, Align::Left);
        }
        ui.text_sh("FAN %", p.x - 12.0, p.y - 10.0, 15.0, LIME, Align::Right);
        ui.text_sh("RPM x1000", p.x + p.w + 12.0, p.y - 10.0, 15.0, TXT_DIM, Align::Left);
        for i in 0..N_TEMPS {
            let x = p.x + (i + 1) as f32 * cw;
            let r = Rect::new(x - 26.0, p.y + p.h + 4.0, 52.0, 26.0);
            let hot = self.hover_label == Some(i) || self.drag_label.map(|d| d.0) == Some(i);
            if hot {
                ui.rrect(r.x, r.y, r.w, r.h, 5.0, Color::new(LIME.r, LIME.g, LIME.b, 0.25));
            }
            ui.text_sh(&format!("{}°", c.t[i]), x, p.y + p.h + 24.0, 19.0, if hot { LIME } else { TXT }, Align::Center);
        }
        ui.text_sh("TEMP °C  (drag)", p.x + p.w, p.y + p.h + 48.0, 15.0, TXT_DIM, Align::Right);
        // legend, top-left like the game's Maximum/Minimum box
        let (lx, ly) = (p.x + 8.0, p.y + 8.0);
        ui.rrect(lx, ly, 132.0, 50.0, 5.0, Color::new(0.0, 0.0, 0.0, 0.6));
        ui.rect(lx + 10.0, ly + 11.0, 10.0, 10.0, Color::new(0.8, 0.3, 0.3, 1.0));
        ui.text("Below default", lx + 28.0, ly + 21.0, 15.0, TXT, Align::Left);
        ui.disc(lx + 15.0, ly + 36.0, 5.0, Color::new(0.86, 0.94, 0.36, 1.0));
        ui.text("Selected", lx + 28.0, ly + 41.0, 15.0, TXT, Align::Left);
        if linked_view {
            ui.rect(p.x, p.y, p.w, p.h, Color::new(0.0, 0.0, 0.0, 0.55));
            ui.text_sh("LINKED TO CPU CURVE", p.x + p.w * 0.5, p.y + p.h * 0.5 + 10.0, 30.0, LIME, Align::Center);
            ui.text_sh("press L to edit the GPU table on its own", p.x + p.w * 0.5, p.y + p.h * 0.5 + 40.0, 18.0, TXT_DIM, Align::Center);
        }

        // ---- readout + status
        let n = self.sel;
        let tune = c.s[n];
        let base = base_at(&c, n);
        let delta = tune - base;
        let ry = py1 + 70.0;
        let main = format!("{}:   {}%   (about {} RPM)", range_text(&c, n), tune, tune * RPM_PER_PCT);
        let w0 = ui.text_sh(&main, px0, ry, 22.0, TXT, Align::Left);
        if delta != 0 {
            let s = format!("   ({}{} vs default)", if delta > 0 { "+" } else { "" }, delta);
            ui.text_sh(&s, px0 + w0, ry, 22.0, if delta > 0 { TEAL } else { Color::new(0.88, 0.35, 0.29, 1.0) }, Align::Left);
        }
        let mut sx = px1;
        if self.busy() {
            sx -= ui.text_sh("working…", sx, ry, 20.0, AMBER, Align::Right) + 18.0;
        } else if self.dirty() {
            sx -= ui.text_sh("UNSAVED: press Apply", sx, ry, 20.0, AMBER, Align::Right) + 18.0;
        }
        if tele.boost == "on" {
            sx -= ui.chip(sx - 110.0, ry - 7.0, "BOOST ON", LIME) + 10.0;
        }
        if !tele.fan_mode.is_empty() && tele.fan_mode != "advanced" {
            ui.chip(sx - 270.0, ry - 7.0, "FAN MODE AUTO: F fixes it", RED);
        }

    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_judged_at_column_start() {
        // same thresholds as factory: column n gets factory speed n
        for n in 0..N_SPEEDS {
            assert_eq!(base_at(&FACTORY, n), FACTORY.s[n]);
        }
        // the user's tune: column 1 starts at 50 °C -> factory says 40 there
        let mine = Curve::new([50, 56, 62, 68, 74, 78], [0, 30, 50, 70, 90, 100, 100]);
        assert_eq!(base_at(&mine, 0), 0);
        assert_eq!(base_at(&mine, 1), 40);
        assert_eq!(base_at(&mine, 3), 50); // starts at 62 -> between 60 and 65
        assert_eq!(base_at(&mine, 6), 80); // starts at 78 -> past the 75 threshold: factory step 5 = 80
    }

    #[test]
    fn ranges() {
        assert_eq!(range_text(&FACTORY, 0), "below 50°C");
        assert_eq!(range_text(&FACTORY, 3), "65–70°C");
        assert_eq!(range_text(&FACTORY, 6), "80°C and up");
    }
}
