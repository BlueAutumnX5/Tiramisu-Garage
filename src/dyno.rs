//! Live Dyno: read-only telemetry from msi-ec / hwmon, drawn as race gauges.
//!
//! Sampling is 1 Hz on a background thread and only while the screen is open.
//! Each value is a single small sysfs read. The Fan Curve Tuner notes record
//! why this matters: hammering the EC (a full 256-byte dump twice a second)
//! once made the keyboard repeat keys, because the keyboard sits on the same EC.

use crate::anim::Spring;
use crate::draw::{AMBER, Align, LIME, RED, TEAL, TXT, TXT_DIM, Ui};
use macroquad::prelude::*;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const EC: &str = "/sys/devices/platform/msi-ec";
const HISTORY: usize = 180;

#[derive(Clone, Default, Debug)]
pub struct Tele {
    pub ok: bool,
    pub cpu_t: Option<f32>,
    pub gpu_t: Option<f32>,
    pub rpm: Option<f32>,
    pub duty: Option<f32>,
    pub shift: String,
    pub fan_mode: String,
    pub boost: String,
    pub mhz: Option<f32>,
    pub bat: Option<f32>,
    pub bat_status: String,
    pub kick: Option<bool>,
    pub history: VecDeque<(f32, f32)>,
}

pub struct Dyno {
    shared: Arc<Mutex<Tele>>,
    active: Arc<AtomicBool>,
    pub kick_temp: f32,
    rpm: Spring,
    temp: Spring,
    gpu: Spring,
}

fn rd(p: impl AsRef<std::path::Path>) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}
fn rdf(p: impl AsRef<std::path::Path>) -> Option<f32> {
    rd(p)?.parse().ok()
}

fn fan_rpm_path() -> Option<PathBuf> {
    let dir = fs::read_dir("/sys/class/hwmon").ok()?;
    for e in dir.flatten() {
        let p = e.path();
        if rd(p.join("name")).as_deref() == Some("msi_wmi_platform") && p.join("fan1_input").exists() {
            return Some(p.join("fan1_input"));
        }
    }
    None
}

fn kick_temp() -> f32 {
    rd("/etc/default/fan-kick")
        .and_then(|s| s.lines().find_map(|l| l.trim().strip_prefix("KICK_TEMP=").and_then(|v| v.trim().parse().ok())))
        .unwrap_or(82.0)
}

fn sample(rpm_path: &Option<PathBuf>, tick: u64, last_kick: Option<bool>) -> Tele {
    let ec = std::path::Path::new(EC);
    let mut t = Tele { ok: ec.exists(), ..Default::default() };
    t.cpu_t = rdf(ec.join("cpu/realtime_temperature"));
    t.gpu_t = rdf(ec.join("gpu/realtime_temperature"));
    t.duty = rdf(ec.join("cpu/realtime_fan_speed"));
    t.rpm = rpm_path.as_ref().and_then(rdf);
    t.shift = rd(ec.join("shift_mode")).unwrap_or_default();
    t.fan_mode = rd(ec.join("fan_mode")).unwrap_or_default();
    t.boost = rd(ec.join("cooler_boost")).unwrap_or_default();
    // average current CPU clock
    let mut sum = 0.0;
    let mut n = 0.0;
    for cpu in 0..64 {
        match rdf(format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq")) {
            Some(v) => {
                sum += v;
                n += 1.0;
            }
            None if cpu > 0 => break,
            None => {}
        }
    }
    if n > 0.0 {
        t.mhz = Some(sum / n / 1000.0);
    }
    t.bat = rdf("/sys/class/power_supply/BAT1/capacity");
    t.bat_status = rd("/sys/class/power_supply/BAT1/status").unwrap_or_default();
    // fan-kick status: a fork, so only every 10 s
    t.kick = if tick % 10 == 0 {
        std::process::Command::new("systemctl").args(["is-active", "--quiet", "fan-kick.service"]).status().ok().map(|s| s.success())
    } else {
        last_kick
    };
    t
}

impl Dyno {
    pub fn new() -> Dyno {
        let shared = Arc::new(Mutex::new(Tele::default()));
        let active = Arc::new(AtomicBool::new(false));
        {
            let shared = shared.clone();
            let active = active.clone();
            std::thread::spawn(move || {
                let rpm_path = fan_rpm_path();
                let mut tick: u64 = 0;
                loop {
                    if active.load(Ordering::Relaxed) {
                        let last_kick = shared.lock().unwrap().kick;
                        let mut t = sample(&rpm_path, tick, last_kick);
                        let mut g = shared.lock().unwrap();
                        t.history = std::mem::take(&mut g.history);
                        t.history.push_back((t.cpu_t.unwrap_or(0.0), t.rpm.unwrap_or(0.0)));
                        while t.history.len() > HISTORY {
                            t.history.pop_front();
                        }
                        *g = t;
                        tick += 1;
                    } else {
                        tick = 0;
                    }
                    std::thread::sleep(Duration::from_millis(1000));
                }
            });
        }
        Dyno { shared, active, kick_temp: kick_temp(), rpm: Spring::new(0.0), temp: Spring::new(30.0), gpu: Spring::new(0.0) }
    }

    /// Latest telemetry snapshot (shared with the tuner screen).
    pub fn tele(&self) -> Tele {
        self.shared.lock().unwrap().clone()
    }

    pub fn set_active(&self, on: bool) {
        self.active.store(on, Ordering::Relaxed);
    }

    pub fn draw(&mut self, ui: &Ui, dt: f32) {
        let tele = self.shared.lock().unwrap().clone();
        if let Some(r) = tele.rpm {
            self.rpm.target = r;
        }
        if let Some(t) = tele.cpu_t {
            self.temp.target = t;
        }
        if let Some(g) = tele.gpu_t {
            self.gpu.target = g;
        }
        self.rpm.update(dt, 3.0);
        self.temp.update(dt, 3.5);
        self.gpu.update(dt, 3.5);

        let w = ui.w;
        // mode chips
        let mut x = 40.0;
        let cy = 126.0;
        let up = |s: &str| if s.is_empty() { "?".to_string() } else { s.to_uppercase() };
        x += ui.chip(x, cy, &format!("SHIFT  {}", up(&tele.shift)), LIME) + 10.0;
        let fm_col = if tele.fan_mode == "advanced" { LIME } else { AMBER };
        x += ui.chip(x, cy, &format!("FAN MODE  {}", up(&tele.fan_mode)), fm_col) + 10.0;
        let boost_on = tele.boost == "on";
        x += ui.chip(x, cy, &format!("COOLER BOOST  {}", up(&tele.boost)), if boost_on { RED } else { TXT_DIM }) + 10.0;
        match tele.kick {
            Some(true) => x += ui.chip(x, cy, &format!("FAN-KICK ARMED  {:.0} °C", self.kick_temp), LIME) + 10.0,
            Some(false) => x += ui.chip(x, cy, "FAN-KICK STOPPED", RED) + 10.0,
            None => {}
        }
        let _ = x;
        if !tele.ok {
            ui.text_sh("msi-ec is not loaded: no EC telemetry", w * 0.5, 330.0, 30.0, AMBER, Align::Center);
        }

        // tachometer: fan RPM
        let tcx = w * 0.30;
        let gcy = 372.0;
        let rpm = self.rpm.value;
        gauge(
            ui,
            tcx,
            gcy,
            168.0,
            rpm / 1000.0,
            0.0,
            6.0,
            5.0,
            &["0", "1", "2", "3", "4", "5", "6"],
            "FAN  RPM x1000",
            &format!("{:.0}", rpm),
            "RPM",
        );
        // gear box: EC fan duty
        let gx = tcx + 64.0;
        let gy = gcy + 30.0;
        ui.rrect(gx - 24.0, gy - 22.0, 48.0, 44.0, 7.0, Color::new(0.0, 0.0, 0.0, 0.6));
        ui.rrect_line(gx - 24.0, gy - 22.0, 48.0, 44.0, 7.0, 1.6, LIME);
        ui.text_with(&ui.black, &format!("{:.0}", tele.duty.unwrap_or(0.0)), gx, gy + 10.0, 28.0, LIME, Align::Center);
        ui.text("DUTY %", gx, gy + 36.0, 12.0, TXT_DIM, Align::Center);

        // temperature gauge
        let kick = self.kick_temp;
        let ccx = w * 0.64;
        gauge(
            ui,
            ccx,
            gcy + 8.0,
            142.0,
            self.temp.value,
            30.0,
            100.0,
            kick,
            &["30", "40", "50", "60", "70", "80", "90", "100"],
            "CPU  °C",
            &format!("{:.0}°", self.temp.value),
            "CPU",
        );

        // side bars
        let bx = (w * 0.80).min(w - 250.0);
        let mut by = 250.0;
        let mut bar = |label: &str, val: f32, max: f32, txt: String, col: Color| {
            let bw = 210.0;
            ui.text_sh(label, bx, by, 18.0, TXT, Align::Left);
            ui.text_sh(&txt, bx + bw, by, 18.0, col, Align::Right);
            ui.rrect(bx, by + 8.0, bw, 10.0, 5.0, Color::new(0.0, 0.0, 0.0, 0.55));
            let f = (val / max).clamp(0.0, 1.0);
            if f > 0.0 {
                ui.rrect(bx, by + 8.0, (bw * f).max(10.0), 10.0, 5.0, col);
            }
            by += 62.0;
        };
        let g = self.gpu.value;
        bar("GPU", g, 100.0, if g < 1.0 { "asleep".into() } else { format!("{g:.0} °C") }, TEAL);
        if let Some(m) = tele.mhz {
            bar("CPU CLOCK", m, 4400.0, format!("{m:.0} MHz"), LIME);
        }
        if let Some(b) = tele.bat {
            let st = match tele.bat_status.as_str() {
                "Charging" => " charging",
                "Discharging" => " on battery",
                "Full" => " full",
                "Not charging" => " plugged",
                _ => "",
            };
            bar("BATTERY", b, 100.0, format!("{b:.0} %{st}"), AMBER);
        }

        // dyno chart: temperature and RPM history
        let (cx0, cy0, cw, ch) = (40.0, 560.0, w - 80.0, 92.0);
        ui.rrect(cx0, cy0, cw, ch, 10.0, Color::new(0.0, 0.0, 0.0, 0.55));
        ui.rrect_line(cx0, cy0, cw, ch, 10.0, 1.2, Color::new(1.0, 1.0, 1.0, 0.18));
        let ky = cy0 + ch - (kick - 30.0) / 70.0 * ch;
        let mut xx = cx0 + 10.0;
        while xx < cx0 + cw - 10.0 {
            ui.line(xx, ky, (xx + 10.0).min(cx0 + cw - 10.0), ky, 1.2, Color::new(1.0, 0.36, 0.24, 0.55));
            xx += 18.0;
        }
        let n = tele.history.len();
        if n > 1 {
            let step = (cw - 20.0) / (HISTORY - 1) as f32;
            let x_at = |i: usize| cx0 + 10.0 + (HISTORY - n + i) as f32 * step;
            for i in 1..n {
                let (t0, r0) = tele.history[i - 1];
                let (t1, r1) = tele.history[i];
                let yt = |t: f32| cy0 + ch - 6.0 - ((t - 30.0) / 70.0).clamp(0.0, 1.0) * (ch - 12.0);
                let yr = |r: f32| cy0 + ch - 6.0 - (r / 6000.0).clamp(0.0, 1.0) * (ch - 12.0);
                ui.line(x_at(i - 1), yr(r0), x_at(i), yr(r1), 2.2, TEAL);
                ui.line(x_at(i - 1), yt(t0), x_at(i), yt(t1), 2.2, AMBER);
            }
        }
        ui.text_sh("DYNO RUN", cx0 + 14.0, cy0 - 8.0, 18.0, LIME, Align::Left);
        ui.text_sh("— CPU °C", cx0 + 120.0, cy0 - 8.0, 16.0, AMBER, Align::Left);
        ui.text_sh("— FAN RPM", cx0 + 210.0, cy0 - 8.0, 16.0, TEAL, Align::Left);
        ui.text_sh(&format!("- - fan-kick {kick:.0} °C"), cx0 + 316.0, cy0 - 8.0, 16.0, RED, Align::Left);
        ui.text_sh("last 3 min, 1 sample/s", cx0 + cw - 10.0, cy0 - 8.0, 15.0, TXT_DIM, Align::Right);
    }
}

/// A race-HUD gauge: 270-degree sweep, ticks and numbers, a red zone, the
/// filled value arc, a needle, and a big digital readout.
#[allow(clippy::too_many_arguments)]
fn gauge(ui: &Ui, cx: f32, cy: f32, r: f32, v: f32, lo: f32, hi: f32, red_from: f32, labels: &[&str], caption: &str, big: &str, unit: &str) {
    use std::f32::consts::PI;
    let a0 = 0.75 * PI; // bottom-left
    let a1 = 2.25 * PI; // bottom-right, clockwise through the top
    let ang = |x: f32| a0 + (a1 - a0) * ((x - lo) / (hi - lo)).clamp(0.0, 1.0);

    // bezel and face
    ui.disc(cx, cy, r + 10.0, Color::new(0.0, 0.0, 0.0, 0.45));
    ui.arc_grad(cx, cy, r + 2.0, r + 8.0, -PI, PI, Color::new(0.75, 0.78, 0.8, 0.85), Color::new(0.25, 0.27, 0.3, 0.85));
    ui.disc(cx, cy, r + 2.0, Color::new(0.03, 0.035, 0.04, 0.82));

    // red zone band
    ui.arc(cx, cy, r - 16.0, r - 4.0, ang(red_from), a1, Color::new(0.85, 0.12, 0.08, 0.85));
    // value arc
    let av = ang(v);
    let hot = v >= red_from;
    ui.arc_grad(cx, cy, r - 30.0, r - 20.0, a0, av, Color::new(0.33, 0.70, 0.69, 0.35), if hot { RED } else { LIME });

    // ticks
    let steps = (labels.len() - 1) as f32;
    let minor = 4;
    for i in 0..=((labels.len() - 1) * minor) {
        let f = i as f32 / (steps * minor as f32);
        let a = a0 + (a1 - a0) * f;
        let major = i % minor == 0;
        let (ri, ro) = if major { (r - 34.0, r - 4.0) } else { (r - 18.0, r - 4.0) };
        let col = if lo + (hi - lo) * f >= red_from { Color::new(1.0, 0.5, 0.4, 1.0) } else { TXT };
        ui.line(cx + a.cos() * ri, cy + a.sin() * ri, cx + a.cos() * ro, cy + a.sin() * ro, if major { 3.0 } else { 1.4 }, col);
        if major {
            let idx = i / minor;
            let lr = r - 54.0;
            ui.text_with(&ui.black, labels[idx], cx + a.cos() * lr, cy + a.sin() * lr + 9.0, 24.0, col, Align::Center);
        }
    }
    // a lime marker where the red zone starts
    let ak = ang(red_from);
    ui.line(cx + ak.cos() * (r - 40.0), cy + ak.sin() * (r - 40.0), cx + ak.cos() * (r + 6.0), cy + ak.sin() * (r + 6.0), 3.0, LIME);

    // needle with a soft glow
    let nl = r - 12.0;
    let (s, c) = av.sin_cos();
    ui.line(cx - c * 18.0, cy - s * 18.0, cx + c * nl, cy + s * nl, 7.0, Color::new(1.0, 0.3, 0.15, 0.25));
    ui.line(cx - c * 18.0, cy - s * 18.0, cx + c * nl, cy + s * nl, 3.5, Color::new(1.0, 0.42, 0.22, 1.0));
    ui.glow(cx, cy, 34.0, Color::new(1.0, 0.4, 0.2, 0.35));
    ui.disc(cx, cy, 13.0, Color::new(0.12, 0.13, 0.14, 1.0));
    ui.arc(cx, cy, 11.0, 13.0, 0.0, 2.0 * PI, Color::new(0.8, 0.82, 0.84, 1.0));

    // readout
    ui.text_with(&ui.black, big, cx, cy + r * 0.62, 50.0, if hot { RED } else { TXT }, Align::Center);
    ui.text(caption, cx, cy + r * 0.62 + 22.0, 15.0, LIME, Align::Center);
    let _ = unit;
}
