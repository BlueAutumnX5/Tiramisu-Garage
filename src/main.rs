//! Tiramisu Garage: every workshop script behind one animated front end in
//! the style of a 2004 street-racing game.
//!
//! Screens: Main (icon strip of tools) -> Tool (icon strip of that tool's
//! sub-commands) -> Log (live output) / Dyno (gauges) / Options (list panel).

mod anim;
mod catalog;
mod draw;
mod dyno;
mod ec;
mod options;
mod runner;
mod scene;
mod sound;
mod term;
mod tuner;

use anim::{Spring, ease_out, smooth};
use catalog::{Ask, Native, Run, Tool};
use draw::{AMBER, Align, LIME, LIME_DIM, RED, TXT, TXT_DIM, Ui, hit, ico};
use macroquad::miniquad::window as mq_window;
use macroquad::prelude::*;
use options::Options;
use runner::{Job, JobState, Phone, Spec};
use sound::{Fx, Sfx};
use std::sync::mpsc::Receiver;

const FONT: &[u8] = include_bytes!("../assets/garage-italic.ttf");
const FONT_BLACK: &[u8] = include_bytes!("../assets/garage-black.ttf");
const FONT_MONO: &[u8] = include_bytes!("../assets/garage-mono.ttf");

const SLOT: f32 = 236.0;
/// Height of the bottom panel (2 button rows + padding); content stays above it.
/// The game's dialog palette (from the "Jump to Garage?" box), shared by
/// dialogs and the Options list: pale lime frame, grey panel, a button grey a
/// shade darker than the panel, and a near-black selected button.
const FRAME: Color = Color::new(0.84, 0.90, 0.66, 0.95);
const PANEL: Color = Color::new(0.29, 0.30, 0.30, 0.94);
const BTN: Color = Color::new(0.25, 0.26, 0.26, 0.96);
const BTN_SEL: Color = Color::new(0.13, 0.135, 0.135, 0.96);

pub const FOOTER_H: f32 = 28.0 * 2.0 + 7.0 + 22.0;
const STRIP_Y: f32 = 94.0;
const STRIP_H: f32 = 150.0;

fn window_conf() -> Conf {
    // Window icon for the taskbar / Alt+Tab, rendered from
    // packaging/tiramisu-garage.svg (raw RGBA, 16/32/64 px).
    let icon = macroquad::miniquad::conf::Icon {
        small: *include_bytes!("../assets/icon-16.rgba"),
        medium: *include_bytes!("../assets/icon-32.rgba"),
        big: *include_bytes!("../assets/icon-64.rgba"),
    };
    let mut platform = macroquad::miniquad::conf::Platform::default();
    // miniquad's default class is "miniquad-application"; KDE matches the
    // window to tiramisu-garage.desktop (StartupWMClass) through this.
    platform.linux_wm_class = "tiramisu-garage";
    Conf {
        window_title: "Tiramisu Garage".to_owned(),
        window_width: 1280,
        window_height: 720,
        window_resizable: true,
        sample_count: 4,
        high_dpi: false,
        icon: Some(icon),
        platform,
        ..Default::default()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Screen {
    Main,
    Tool(usize),
    Log,
    Dyno,
    Tuner,
    Options,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Nav {
    Push(Screen),
    Pop,
}

struct StripState {
    sel: usize,
    /// item index at the centre (fractional while sliding)
    scroll: Spring,
    /// selection box offset from the centre, in slots (0 except at the ends)
    boxpos: Spring,
    dot: Spring,
    changed_at: f64,
}

impl StripState {
    fn new() -> Self {
        StripState { sel: 0, scroll: Spring::new(0.0), boxpos: Spring::new(0.0), dot: Spring::new(0.0), changed_at: -10.0 }
    }
}

/// Which item index sits at the centre of the strip. Like the game's
/// carousel: the selection box never moves, the selected icon is always
/// scrolled under it, and the ends of the list just leave empty slots.
fn strip_scroll_target(_n: usize, sel: usize) -> f32 {
    sel as f32
}

/// Opacity of an icon `d` slots away from the centre: the three middle
/// icons are solid, the far ones fade smoothly (continuous in d, so the fade
/// follows the icons while they slide).
fn strip_fade(d: f32) -> f32 {
    let d = d.abs() as f64;
    (1.0 - 0.74 * smooth(1.0, 2.0, d)) * (1.0 - smooth(2.0, 2.75, d))
}

struct StripItem {
    glyph: char,
    label: String,
    dim: bool,
}

/// A run request moving through ask -> confirm -> execute.
#[derive(Clone, Debug)]
struct Pending {
    tool: usize,
    cmd: usize,
    text: Option<String>,
    files: Option<Vec<String>>,
    asked: bool,
    confirmed: bool,
}

#[derive(Clone, Debug)]
enum DAction {
    Run(Pending),
    StopJob,
    ReplaceJob(Pending),
    ResetOptions,
    TunerApply,
    TunerLeave,
    Quit,
    ViewLog,
}

enum Dialog {
    Confirm { text: String, yes: bool, action: DAction, opened: f64 },
    Input { prompt: String, placeholder: String, text: String, allow_empty: bool, pending: Pending, opened: f64 },
    Message { text: String, opened: f64 },
}

/// What a footer button does when clicked.
#[derive(Clone, Copy, PartialEq, Debug)]
enum FAct {
    Select,
    Quit,
    Back,
    LastLog,
    PhoneNext,
    SendAnswer,
    Stop,
    RunAgain,
    CopyLog,
    OpenTuner,
    TunerPill(usize),
    TunerHelp,
    TunerTab,
    RestoreDefaults,
}

/// One footer button: animation key, label, hotkey hint, hotkey, action.
struct FBtn {
    key: &'static str,
    label: String,
    hint: &'static str,
    keys: &'static [KeyCode],
    act: FAct,
}

fn fb(key: &'static str, label: impl Into<String>, hint: &'static str, keys: &'static [KeyCode], act: FAct) -> FBtn {
    FBtn { key, label: label.into(), hint, keys, act }
}

#[derive(Default)]
struct Hits {
    items: Vec<(Rect, usize)>,
    prev: Option<Rect>,
    next: Option<Rect>,
    footer: Vec<(Rect, &'static str, FAct)>,
    cyc_prev: Option<Rect>,
    cyc_next: Option<Rect>,
    dlg_yes: Option<Rect>,
    dlg_no: Option<Rect>,
    rows: Vec<(Rect, usize)>,
    row_prev: Option<Rect>,
    row_next: Option<Rect>,
    log_up: Option<Rect>,
    log_down: Option<Rect>,
}

struct App {
    ui: Ui,
    tools: Vec<Tool>,
    installed: Vec<bool>,
    stack: Vec<Screen>,
    main_strip: StripState,
    tool_strips: Vec<StripState>,
    pending_nav: Option<(f64, Nav)>,
    enter_at: f64,
    dir: f32,
    dialog: Option<Dialog>,
    job: Option<Job>,
    log_scroll: f32,
    log_follow: bool,
    log_input: String,
    log_seen_version: u64,
    dyno: dyno::Dyno,
    tuner: tuner::Tuner,
    opts: Options,
    opt_sel: usize,
    sfx: Sfx,
    scene: scene::Scene,
    /// last mouse position while the middle button holds the turntable
    grab_last: Option<Vec2>,
    dim: Spring,
    toast: Option<(String, f64, Color)>,
    picker: Option<(Receiver<Option<Vec<String>>>, Pending)>,
    hits: Hits,
    chars: Vec<char>,
    rep: Vec<(KeyCode, f64, f64)>,
    wheel_acc: f32,
    quit: bool,
    /// screenshot file to write after this frame is drawn
    shot: Option<String>,
    demo: Option<Demo>,
    /// window scale the font cache was warmed for
    warm_k: f32,
    /// Options scrollbar marker, gliding between rows
    opt_dot: Spring,
    /// first visible Options row (fractional while scrolling)
    opt_top: Spring,
}

/// Scripted walk through every screen, saving a PNG at each stop.
/// Run with GARAGE_DEMO=/some/dir; used to check the rendering.
#[derive(Clone, Copy, Debug)]
enum DStep {
    Shot(&'static str),
    Right,
    Enter,
    Back,
    Tool(usize),
    Cmd(usize),
    PhoneNext,
    DialogNo,
    Stop,
    TunerNudge,
    TunerTab,
    TunerHelp,
    TunerEsc,
    /// start auto-repeating Left (-1) / Right (+1) like a held key; 0 releases
    Hold(i32),
    /// Options screen: like pressing Down
    OptDown,
    /// easter egg: a middle-button drag by (dx, dy) virtual pixels
    Grab(f32, f32),
    /// the "job finished" question
    JobDone,
    /// a short OK-only note
    Note(&'static str),
    Quit,
}

struct Demo {
    dir: String,
    t0: f64,
    i: usize,
    steps: Vec<(f64, DStep)>,
    /// (direction, last step time) while a key is "held"
    hold: Option<(i32, f64)>,
    /// frame times (s) recorded while holding
    frames: Vec<f32>,
    /// carousel position (slots) each frame while holding
    scrolls: Vec<(i64, f32)>,
}

impl Demo {
    fn new(dir: String) -> Demo {
        use DStep::*;
        let steps = vec![
            (1.0, Shot("01-main")),
            (1.2, Right),
            (1.27, Shot("02-icons-slide")),
            (1.7, Right),
            (1.76, Shot("03-icons-slide-more")),
            (2.3, Shot("04-main-centred")),
            (2.5, Tool(1)),
            (2.9, Shot("05-main-performance")),
            (3.1, Enter),
            (3.16, Shot("06-leaving")),
            (3.27, Shot("07-arriving")),
            (3.9, Shot("08-performance-strip")),
            (4.1, Cmd(1)),
            (4.3, Enter),
            (6.0, Shot("09-tuner")),
            (6.2, TunerNudge),
            (6.9, Shot("10-tuner-edited")),
            (7.0, TunerTab),
            (7.6, Shot("11-tuner-gpu-linked")),
            (7.7, TunerTab),
            (7.8, TunerHelp),
            (8.1, Shot("12-tuner-help")),
            (8.2, DialogNo),
            (8.4, TunerEsc),
            (8.8, Shot("13-tuner-leave-confirm")),
            (9.0, DialogNo),
            (9.1, Back),
            (9.6, Cmd(0)),
            (9.8, Enter),
            (12.8, Shot("14-dyno")),
            (13.0, Back),
            (13.5, Cmd(5)),
            (13.7, Enter),
            (15.5, Shot("15-log")),
            (15.7, Back),
            (16.2, Back),
            (16.7, Tool(7)),
            (17.0, Enter),
            (17.6, Cmd(1)),
            (17.8, Enter),
            (21.5, Shot("x-live-view")),
            (21.7, Stop),
            (23.5, Shot("x-stopped")),
            (23.7, Back),
            (24.2, Back),
            (24.7, Tool(2)),
            (25.0, Enter),
            (25.6, PhoneNext),
            (26.2, Shot("16-phone-doctor")),
            (26.4, Cmd(4)),
            (26.6, Enter),
            (27.2, Shot("17-input-dialog")),
            (27.4, DialogNo),
            (27.6, Cmd(12)),
            (27.8, Enter),
            (28.4, Shot("18-confirm-dialog")),
            (28.6, DialogNo),
            (29.0, Back),
            (29.6, Tool(11)),
            (29.9, Enter),
            (30.7, Shot("19-options")),
            (30.9, Back),
            (31.8, Shot("20-before-grab")),
            (32.2, Grab(-160.0, 0.0)),
            (32.6, Shot("21-grab-spun-left")),
            (33.0, Grab(0.0, -400.0)),
            (33.4, Shot("22-grab-camera-up")),
            (33.8, Grab(0.0, 900.0)),
            (34.2, Shot("23-grab-camera-down")),
            (35.5, Shot("24-still-resting")),
            (35.8, JobDone),
            (36.4, Shot("25-job-done-dialog")),
            (36.95, Shot("25b-job-done-blink")),
            (37.1, DialogNo),
            (37.4, Note("The EC already has this curve.")),
            (38.0, Shot("26-ok-only-dialog")),
            (38.3, Quit),
        ];
        let steps = if std::env::var("GARAGE_DEMO_HOLD").is_ok() {
            // hold-key stress test: hold Right, then Left, on the main menu and in Phone Doctor
            vec![
                (1.0, Hold(1)),
                (2.2, Hold(-1)),
                (3.4, Hold(1)),
                (4.2, Hold(0)),
                (4.6, Shot("hold-main")),
                (4.8, Tool(2)),
                (5.0, Enter),
                (5.8, Hold(1)),
                (7.0, Hold(-1)),
                (8.0, Hold(0)),
                (8.4, Shot("hold-tool")),
                (8.6, Back),
                (9.1, Tool(11)),
                (9.4, Enter),
                (10.2, OptDown),
                (10.25, Shot("opt-arrow-flash")),
                (10.6, OptDown),
                (10.64, Shot("opt-dot-gliding")),
                (11.2, Shot("opt-settled")),
                (11.4, OptDown),
                (11.5, OptDown),
                (11.6, OptDown),
                (11.7, OptDown),
                (11.8, OptDown),
                (11.9, OptDown),
                (12.0, OptDown),
                (12.1, OptDown),
                (12.2, OptDown),
                (12.26, Shot("opt-scrolling")),
                (12.9, Shot("opt-scrolled-bottom")),
                (13.1, Quit),
            ]
        } else {
            steps
        };
        Demo { dir, t0: -1.0, i: 0, steps, hold: None, frames: Vec::new(), scrolls: Vec::new() }
    }
}

const OPT_ROWS: [&str; 12] = [
    "UI Sounds",
    "Sound Volume",
    "Turntable",
    "Animations",
    "Fullscreen",
    "Default Phone",
    "Log Text Size",
    "Command Count",
    "Description",
    "Tags",
    "Hotkey Hints",
    "Restore Defaults",
];
/// index of the last row, "Restore Defaults"
const OPT_RESTORE: usize = OPT_ROWS.len() - 1;

impl App {
    fn now(&self) -> f64 {
        self.ui.t
    }

    /// Rasterise every glyph the menus use, at the sizes they use, so no
    /// frame ever stalls on first-time glyph drawing. Re-run when the
    /// window scale changes.
    fn prewarm(&mut self) {
        let k = self.ui.k;
        if (k - self.warm_k).abs() < 1e-3 {
            return;
        }
        self.warm_k = k;
        let mut text: Vec<char> = (32u8..127).map(|b| b as char).collect();
        let mut icons: Vec<char> = Vec::new();
        for t in &self.tools {
            text.extend(t.name.chars().chain(t.blurb.chars()));
            icons.push(t.glyph);
            for c in &t.cmds {
                text.extend(c.label.chars().chain(c.desc.chars()));
                icons.push(c.glyph);
            }
        }
        text.extend("°·…—–".chars());
        text.sort_unstable();
        text.dedup();
        icons.sort_unstable();
        icons.dedup();
        let fs = |size: f32| (size * k).round().clamp(4.0, 400.0) as u16;
        // every literal size drawn anywhere: a size missing here rasterises mid-frame
        // (opening Options froze for 0.7 s on its 26/27 px rows)
        for size in [50.0, 46.0, 40.0, 38.0, 32.0, 30.0, 28.0, 27.0, 26.0, 24.0, 22.0, 20.0, 19.0, 18.0, 17.0, 16.0, 15.0, 12.0] {
            self.ui.font.populate_font_cache(&text, fs(size));
        }
        for size in [24.0, 22.0, 15.0, 14.0] {
            self.ui.black.populate_font_cache(&text, fs(size));
        }
        // icon glyphs: every 8 px base size an animated icon can round up to
        let mut bases: Vec<u16> = (40..=110).map(|s| ((s as f32 * k / 8.0).ceil() * 8.0).min(400.0) as u16).collect();
        bases.dedup();
        for b in bases {
            self.ui.font.populate_font_cache(&icons, b);
        }
    }

    fn save_opts(&self) {
        if self.demo.is_none() {
            self.opts.save();
        }
    }

    fn demo_tick(&mut self) {
        let now = self.now();
        let Some(demo) = self.demo.as_mut() else { return };
        if demo.t0 < 0.0 {
            demo.t0 = now;
        }
        let el = now - demo.t0;
        if get_frame_time() > 0.15 { eprintln!("SLOW el={el:.2} dt={:.2}", get_frame_time()); }
        let until: f64 = std::env::var("GARAGE_DEMO_UNTIL").ok().and_then(|v| v.parse().ok()).unwrap_or(f64::MAX);
        if el > until {
            self.quit = true;
        }
        if let Some((d, last)) = demo.hold {
            demo.frames.push(get_frame_time());
            let (id, pos) = match self.stack.last() {
                Some(Screen::Main) => (-1, self.main_strip.scroll.value),
                Some(Screen::Tool(i)) => (*i as i64, self.tool_strips[*i].scroll.value),
                _ => (-9, 0.0),
            };
            let demo = self.demo.as_mut().unwrap();
            demo.scrolls.push((id, pos));
            if now - last >= 0.075 {
                demo.hold = Some((d, now));
                self.move_sel(d);
            }
        }
        let Some(demo) = self.demo.as_mut() else { return };
        let mut todo = Vec::new();
        while demo.i < demo.steps.len() && demo.steps[demo.i].0 <= el {
            todo.push(demo.steps[demo.i].1);
            demo.i += 1;
        }
        let dir = demo.dir.clone();
        for step in todo {
            match step {
                DStep::Shot(name) => self.shot = Some(format!("{dir}/{name}.png")),
                DStep::Right => self.move_sel(1),
                DStep::Enter => self.activate(),
                DStep::Back => {
                    if self.screen() != Screen::Main {
                        self.go(Nav::Pop)
                    }
                }
                DStep::Tool(i) => {
                    self.main_strip.sel = i;
                    self.main_strip.changed_at = now;
                }
                DStep::Cmd(i) => {
                    if let Screen::Tool(ti) = self.screen() {
                        self.tool_strips[ti].sel = i;
                        self.tool_strips[ti].changed_at = now;
                    }
                }
                DStep::PhoneNext => self.opts.phone = (self.opts.phone + 1) % 3,
                DStep::DialogNo => self.dialog = None,
                DStep::Stop => {
                    if let Some(j) = self.job.as_mut() {
                        j.stop(now)
                    }
                }
                DStep::TunerNudge => self.tuner.demo_nudge(now),
                DStep::TunerTab => self.tuner.demo_tab(now),
                DStep::TunerHelp => self.dialog = Some(Dialog::Message { text: tuner::HELP.into(), opened: now }),
                DStep::TunerEsc => self.back(),
                DStep::Grab(dx, dy) => self.scene.grab(vec2(dx, dy), now),
                DStep::JobDone => self.job_done_dialog("Pre-Flight Check", true),
                DStep::Note(t) => self.dialog = Some(Dialog::Message { text: t.into(), opened: now }),
                DStep::OptDown => {
                    self.opt_sel = (self.opt_sel + 1) % OPT_ROWS.len();
                    self.ui.press("sb_down");
                }
                DStep::Hold(d) => {
                    if let Some(demo) = self.demo.as_mut() {
                        demo.hold = if d == 0 { None } else { Some((d, now - 1.0)) };
                    }
                }
                DStep::Quit => {
                    if let Some(demo) = self.demo.as_ref() {
                        let mut f: Vec<f32> = demo.frames.iter().map(|x| x * 1000.0).collect();
                        if !f.is_empty() {
                            f.sort_by(|a, b| a.partial_cmp(b).unwrap());
                            let avg = f.iter().sum::<f32>() / f.len() as f32;
                            let p95 = f[(f.len() as f32 * 0.95) as usize];
                            let slow = f.iter().filter(|&&x| x > 25.0).count();
                            let jump = demo.scrolls.windows(2).filter(|w| w[0].0 == w[1].0 && w[0].0 != -9).map(|w| (w[1].1 - w[0].1).abs()).fold(0.0f32, f32::max);
                            eprintln!("HOLD-STATS frames={} avg={avg:.1}ms p95={p95:.1}ms max={:.1}ms over25ms={slow} max_jump_per_frame={jump:.3}slot", f.len(), f[f.len() - 1]);
                        }
                    }
                    self.quit = true
                }
            }
        }
    }

    fn screen(&self) -> Screen {
        *self.stack.last().unwrap()
    }

    fn fx(&self, fx: Fx) {
        self.sfx.play(fx, self.opts.sound, self.opts.volume);
    }

    /// A job ended while you were elsewhere: ask, the game's way, whether to look.
    fn job_done_dialog(&mut self, title: &str, ok: bool) {
        let text = if ok { format!("“{title}” finished. View the log?") } else { format!("“{title}” failed. View the log?") };
        if self.dialog.is_none() {
            self.dialog = Some(Dialog::Confirm { text, yes: true, action: DAction::ViewLog, opened: self.now() });
        } else {
            // never knock away a question that is already open
            self.toast(text, if ok { LIME } else { RED });
        }
    }

    fn toast(&mut self, s: impl Into<String>, col: Color) {
        self.toast = Some((s.into(), self.now(), col));
    }

    fn times(&self) -> (f64, f64) {
        if self.opts.anim_full { (0.09, 0.17) } else { (0.0, 0.05) }
    }

    fn go(&mut self, nav: Nav) {
        if self.pending_nav.is_some() {
            return;
        }
        self.fx(Fx::Whoosh);
        self.pending_nav = Some((self.now(), nav));
    }

    fn apply_nav(&mut self, nav: Nav) {
        match nav {
            Nav::Push(s) => {
                self.stack.push(s);
                self.dir = 1.0;
            }
            Nav::Pop => {
                if self.stack.len() > 1 {
                    self.stack.pop();
                }
                self.dir = -1.0;
            }
        }
        self.enter_at = self.now();
        self.dyno.set_active(matches!(self.screen(), Screen::Dyno | Screen::Tuner));
        if nav == Nav::Push(Screen::Tuner) {
            self.tuner.load();
        }
        if self.screen() == Screen::Log {
            self.log_follow = true;
        }
    }

    // ------------------------------------------------------------------ input helpers

    fn rep_pressed(&mut self, k: KeyCode) -> bool {
        let now = self.now();
        key_repeat(&mut self.rep, k, now)
    }

    fn clicked(&self, r: &Option<Rect>) -> bool {
        is_mouse_button_pressed(MouseButton::Left) && r.as_ref().map(|r| hit(r, self.ui.mouse())).unwrap_or(false)
    }

    fn hovered(&self, r: &Rect) -> bool {
        hit(r, self.ui.mouse())
    }

    // ------------------------------------------------------------------ strips

    fn main_items(&self) -> Vec<StripItem> {
        self.tools.iter().enumerate().map(|(i, t)| StripItem { glyph: t.glyph, label: t.name.to_string(), dim: !self.installed[i] }).collect()
    }

    fn tool_items(&self, ti: usize) -> Vec<StripItem> {
        self.tools[ti].cmds.iter().map(|c| StripItem { glyph: c.glyph, label: c.label.to_string(), dim: false }).collect()
    }

    fn strip_mut(&mut self, s: Screen) -> Option<&mut StripState> {
        match s {
            Screen::Main => Some(&mut self.main_strip),
            Screen::Tool(i) => self.tool_strips.get_mut(i),
            _ => None,
        }
    }

    fn strip_len(&self, s: Screen) -> usize {
        match s {
            Screen::Main => self.tools.len(),
            Screen::Tool(i) => self.tools[i].cmds.len(),
            _ => 0,
        }
    }

    fn move_sel(&mut self, delta: i32) {
        let s = self.screen();
        let n = self.strip_len(s) as i32;
        let now = self.now();
        if n == 0 {
            return;
        }
        let Some(st) = self.strip_mut(s) else { return };
        let new = (st.sel as i32 + delta).clamp(0, n - 1) as usize;
        if new != st.sel {
            self.ui.press(if delta < 0 { "strip_l" } else { "strip_r" });
        }
        let Some(st) = self.strip_mut(s) else { return };
        if new != st.sel {
            st.sel = new;
            st.changed_at = now;
            self.fx(Fx::Tick);
        } else {
            self.fx(Fx::Error);
        }
    }

    fn update_strips(&mut self, dt: f32) {
        let w = self.ui.w;
        let s = self.screen();
        let n = self.strip_len(s);
        let _ = w;
        if let Some(st) = self.strip_mut(s) {
            let target = strip_scroll_target(n, st.sel);
            st.scroll.target = target;
            st.boxpos.target = st.sel as f32 - target;
            st.dot.target = if n > 1 { st.sel as f32 / (n - 1) as f32 } else { 0.5 };
            // The further the carousel is behind (spamming or holding an
            // arrow), the stiffer its spring: it catches up quickly but
            // always glides. (Snapping it closer made icons teleport.)
            for sp in [&mut st.scroll, &mut st.boxpos] {
                let lag = (sp.target - sp.value).abs();
                sp.update(dt, 17.0 + 30.0 * (lag - 0.5).max(0.0));
            }
            st.dot.update(dt, 14.0);
        }
        // the Options list scrolls to keep the selected row in view
        {
            let h = self.ui.h - 100.0 - FOOTER_H - 14.0;
            let visible = (((h - 22.0 - 44.0) / 52.0).floor() as usize).max(1);
            let mut top = self.opt_top.target.round() as usize;
            if self.opt_sel < top {
                top = self.opt_sel;
            } else if self.opt_sel >= top + visible {
                top = self.opt_sel + 1 - visible;
            }
            top = top.min(OPT_ROWS.len().saturating_sub(visible));
            self.opt_top.target = top as f32;
            self.opt_top.update(dt, 20.0);
        }
        // the Options scrollbar marker glides between rows
        let rows = OPT_ROWS.len();
        self.opt_dot.target = self.opt_sel as f32 / (rows - 1) as f32;
        self.opt_dot.update(dt, 18.0);
    }

    // ------------------------------------------------------------------ running

    fn activate(&mut self) {
        match self.screen() {
            Screen::Main => {
                let i = self.main_strip.sel;
                let tool = &self.tools[i];
                if tool.cmds.len() == 1 {
                    if let Run::Native(Native::Options) = tool.cmds[0].run {
                        self.fx(Fx::Accept);
                        self.go(Nav::Push(Screen::Options));
                        return;
                    }
                }
                if !self.installed[i] {
                    self.fx(Fx::Error);
                    let n = tool.needs;
                    self.toast(format!("{n} is not installed on this machine"), RED);
                    return;
                }
                self.fx(Fx::Accept);
                self.go(Nav::Push(Screen::Tool(i)));
            }
            Screen::Tool(ti) => {
                let ci = self.tool_strips[ti].sel;
                self.fx(Fx::Accept);
                self.proceed(Pending { tool: ti, cmd: ci, text: None, files: None, asked: false, confirmed: false });
            }
            _ => {}
        }
    }

    /// Walk a request through its ask / confirm steps, then run it.
    fn proceed(&mut self, mut p: Pending) {
        let cmd = self.tools[p.tool].cmds[p.cmd].clone();
        let now = self.now();
        if !p.asked {
            match cmd.ask {
                Ask::Text(prompt, placeholder, allow_empty) => {
                    self.dialog = Some(Dialog::Input {
                        prompt: prompt.into(),
                        placeholder: placeholder.into(),
                        text: String::new(),
                        allow_empty,
                        pending: Pending { asked: true, ..p },
                        opened: now,
                    });
                    return;
                }
                Ask::Files(multi) => {
                    self.toast("Pick the file(s) in the dialog…", LIME);
                    self.picker = Some((runner::pick_files(multi), Pending { asked: true, ..p }));
                    return;
                }
                Ask::None => p.asked = true,
            }
        }
        if !p.confirmed {
            if let Some(q) = cmd.confirm {
                self.dialog = Some(Dialog::Confirm { text: q.into(), yes: false, action: DAction::Run(Pending { confirmed: true, ..p }), opened: now });
                return;
            }
            p.confirmed = true;
        }
        if cmd.run == Run::Log && self.job.as_ref().map(|j| j.running()).unwrap_or(false) {
            let old = self.job.as_ref().unwrap().spec.title.clone();
            self.dialog = Some(Dialog::Confirm { text: format!("Stop “{old}” and start this one?"), yes: false, action: DAction::ReplaceJob(p), opened: now });
            return;
        }
        self.execute(p);
    }

    fn execute(&mut self, p: Pending) {
        let tool = self.tools[p.tool].clone();
        let cmd = tool.cmds[p.cmd].clone();
        let argv = runner::expand(cmd.argv, p.text.as_deref(), p.files.as_deref());
        let env: Vec<(String, String)> = cmd.env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        if let Some(first) = argv.first() {
            if !runner::which(first) {
                self.fx(Fx::Error);
                self.toast(format!("{first} is not installed"), RED);
                return;
            }
        }
        match cmd.run {
            Run::Native(Native::Dyno) => self.go(Nav::Push(Screen::Dyno)),
            Run::Native(Native::Tuner) => self.go(Nav::Push(Screen::Tuner)),
            Run::Native(Native::Options) => self.go(Nav::Push(Screen::Options)),
            Run::Launch => match runner::launch(&argv, &env) {
                Ok(()) => self.toast(format!("Started {}", cmd.label), LIME),
                Err(e) => {
                    self.fx(Fx::Error);
                    self.toast(e, RED)
                }
            },
            Run::Terminal => match runner::in_terminal(&format!("{} · {}", tool.name, cmd.label), &argv, &env) {
                Ok(()) => self.toast(format!("{} opened in Konsole", cmd.label), LIME),
                Err(e) => {
                    self.fx(Fx::Error);
                    self.toast(e, RED)
                }
            },
            Run::Log => {
                let phone = if cmd.phone { Some(Phone::from_index(self.opts.phone)) } else { None };
                let mut env = env;
                if cmd.phone && !self.opts.vivo_serial.is_empty() {
                    env.push(("GARAGE_VIVO_SERIAL".into(), self.opts.vivo_serial.clone()));
                }
                let spec = Spec { title: format!("{} · {}", tool.name, cmd.label), glyph: cmd.glyph, argv, env, phone, cols: self.log_geom().6 as u16 };
                self.start_job(spec);
                if self.screen() != Screen::Log {
                    self.go(Nav::Push(Screen::Log));
                }
            }
        }
    }

    fn start_job(&mut self, spec: Spec) {
        self.job = None; // drops (and stops) any previous job
        self.job = Some(Job::start(spec, self.now()));
        self.log_scroll = 0.0;
        self.log_follow = true;
        self.log_input.clear();
        self.log_seen_version = 0;
    }

    fn dialog_choice(&mut self, action: DAction) {
        match action {
            DAction::Run(p) => self.proceed(p),
            DAction::ViewLog => {
                if self.job.is_some() && self.screen() != Screen::Log {
                    self.go(Nav::Push(Screen::Log));
                }
            }
            DAction::ReplaceJob(p) => {
                self.job = None;
                self.execute(p);
            }
            DAction::StopJob => {
                let now = self.now();
                if let Some(j) = self.job.as_mut() {
                    j.stop(now);
                }
            }
            DAction::ResetOptions => {
                let fs = self.opts.fullscreen;
                self.opts = Options::default();
                if fs != self.opts.fullscreen {
                    set_fullscreen(self.opts.fullscreen);
                }
                self.save_opts();
                self.toast("Options restored to defaults", LIME);
            }
            DAction::TunerApply => self.tuner.start_apply(),
            DAction::TunerLeave => {
                self.fx(Fx::Back);
                self.go(Nav::Pop);
            }
            DAction::Quit => self.quit = true,
        }
    }

    // ------------------------------------------------------------------ frame

    fn frame_input(&mut self) {
        self.hits_input_and_keys();
    }

    fn hits_input_and_keys(&mut self) {
        let now = self.now();
        // dialogs take all input
        if self.dialog.is_some() {
            self.dialog_input();
            return;
        }
        if self.pending_nav.is_some() {
            return;
        }
        // hotkeys light up their footer button
        for b in self.footer_buttons() {
            if b.keys.iter().any(|&k| is_key_pressed(k)) {
                self.ui.press(b.key);
            }
        }
        if is_mouse_button_pressed(MouseButton::Left) {
            let m = self.ui.mouse();
            if let Some((_, key, act)) = self.hits.footer.iter().find(|(r, _, _)| hit(r, m)).cloned() {
                self.ui.press(key);
                self.footer_act(act);
                return;
            }
        }
        match self.screen() {
            Screen::Main | Screen::Tool(_) => self.strip_input(),
            Screen::Log => self.log_input_keys(),
            Screen::Dyno => {
                if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
                    self.fx(Fx::Accept);
                    self.go(Nav::Push(Screen::Tuner));
                }
                if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Backspace) {
                    self.back();
                }
            }
            Screen::Options => self.options_input(),
            Screen::Tuner => {
                let now = self.now();
                let tele = self.dyno.tele();
                let rep = &mut self.rep;
                let ev = self.tuner.input(&self.ui, &tele, now, &mut |k| key_repeat(rep, k, now));
                self.tuner_events(ev);
            }
        }
        if is_key_pressed(KeyCode::F12) {
            // like the old PC game: F12 saves a screenshot
            let stamp = std::process::Command::new("date").arg("+%Y%m%d-%H%M%S").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
            let dir = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Pictures");
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(format!("tiramisu-garage-{stamp}.png"));
            self.shot = Some(path.to_string_lossy().into_owned());
            self.fx(Fx::Tick);
            self.toast(format!("Screenshot saved to ~/Pictures/tiramisu-garage-{stamp}.png"), LIME);
        }
        if is_key_pressed(KeyCode::F11) {
            self.opts.fullscreen = !self.opts.fullscreen;
            set_fullscreen(self.opts.fullscreen);
            self.save_opts();
        }
        let _ = now;
    }

    fn back(&mut self) {
        if self.screen() == Screen::Tuner && self.tuner.dirty() && self.dialog.is_none() {
            self.fx(Fx::Accept);
            self.dialog = Some(Dialog::Confirm { text: "Leave without applying the new curve?".into(), yes: false, action: DAction::TunerLeave, opened: self.now() });
            return;
        }
        if self.stack.len() > 1 {
            self.fx(Fx::Back);
            if self.screen() == Screen::Options {
                self.save_opts();
            }
            self.go(Nav::Pop);
        }
    }

    fn tuner_events(&mut self, ev: Vec<tuner::TEvent>) {
        let now = self.now();
        for e in ev {
            match e {
                tuner::TEvent::Toast(t, c) => self.toast(t, c),
                tuner::TEvent::Message(t) => self.dialog = Some(Dialog::Message { text: t, opened: now }),
                tuner::TEvent::Sound(fx) => self.fx(fx),
                tuner::TEvent::Back => {
                    self.fx(Fx::Back);
                    self.go(Nav::Pop);
                }
                tuner::TEvent::ConfirmLeave => {
                    self.fx(Fx::Accept);
                    self.dialog = Some(Dialog::Confirm { text: "Leave without applying the new curve?".into(), yes: false, action: DAction::TunerLeave, opened: now });
                }
                tuner::TEvent::ConfirmApply => {
                    self.fx(Fx::Accept);
                    self.dialog = Some(Dialog::Confirm { text: "Write this curve to the EC and both config files?".into(), yes: true, action: DAction::TunerApply, opened: now });
                }
            }
        }
    }

    fn ask_quit(&mut self) {
        self.fx(Fx::Accept);
        let text = if self.job.as_ref().map(|j| j.running()).unwrap_or(false) {
            "A job is still running. Stop it and leave the garage?".to_string()
        } else {
            "Leave the garage?".to_string()
        };
        self.dialog = Some(Dialog::Confirm { text, yes: false, action: DAction::Quit, opened: self.now() });
    }

    fn strip_input(&mut self) {
        let s = self.screen();
        if self.rep_pressed(KeyCode::Left) || self.rep_pressed(KeyCode::A) {
            self.move_sel(-1);
        }
        if self.rep_pressed(KeyCode::Right) || self.rep_pressed(KeyCode::D) {
            self.move_sel(1);
        }
        if is_key_pressed(KeyCode::Home) {
            let d = -(self.strip_mut(s).map(|st| st.sel).unwrap_or(0) as i32);
            self.move_sel(d);
        }
        if is_key_pressed(KeyCode::End) {
            let n = self.strip_len(s) as i32;
            let d = n - 1 - self.strip_mut(s).map(|st| st.sel).unwrap_or(0) as i32;
            self.move_sel(d);
        }
        if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) || is_key_pressed(KeyCode::Space) {
            self.activate();
            return;
        }
        if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Backspace) {
            if s == Screen::Main {
                self.ask_quit();
            } else {
                self.back();
            }
            return;
        }
        if is_key_pressed(KeyCode::L) && self.job.is_some() {
            self.fx(Fx::Accept);
            self.go(Nav::Push(Screen::Log));
            return;
        }
        // phone picker
        if let Screen::Tool(ti) = s {
            if self.tools[ti].phone_picker {
                let mut d = 0;
                if self.rep_pressed(KeyCode::Up) || self.clicked(&self.hits.cyc_prev.clone()) {
                    d = -1;
                }
                if self.rep_pressed(KeyCode::Down) || self.clicked(&self.hits.cyc_next.clone()) {
                    d = 1;
                }
                if d != 0 {
                    self.ui.press(if d < 0 { "cyc_l" } else { "cyc_r" });
                    self.opts.phone = ((self.opts.phone as i32 + d).rem_euclid(3)) as u8;
                    self.save_opts();
                    self.fx(Fx::Tick);
                }
            }
        }
        // mouse
        if self.clicked(&self.hits.prev.clone()) {
            self.move_sel(-1);
        }
        if self.clicked(&self.hits.next.clone()) {
            self.move_sel(1);
        }
        if is_mouse_button_pressed(MouseButton::Left) {
            let m = self.ui.mouse();
            let target = self.hits.items.iter().find(|(r, _)| hit(r, m)).map(|(_, i)| *i);
            if let Some(i) = target {
                let sel = self.strip_mut(s).map(|st| st.sel).unwrap_or(0);
                if i == sel {
                    self.activate();
                } else {
                    self.move_sel(i as i32 - sel as i32);
                }
            }
        }
        let (_, wy) = mouse_wheel();
        if wy != 0.0 {
            self.wheel_acc += wy.signum();
            if self.wheel_acc.abs() >= 1.0 {
                let d = if self.wheel_acc > 0.0 { -1 } else { 1 };
                self.wheel_acc = 0.0;
                self.move_sel(d);
            }
        }
    }

    fn log_input_keys(&mut self) {
        let now = self.now();
        let running = self.job.as_ref().map(|j| j.running()).unwrap_or(false);
        let (.., rows) = self.log_geom_rows();
        let page = (rows as f32 - 2.0).max(1.0);
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        let mut scroll = 0.0;
        if self.rep_pressed(KeyCode::PageUp) {
            scroll -= page;
        }
        if self.rep_pressed(KeyCode::PageDown) {
            scroll += page;
        }
        if self.rep_pressed(KeyCode::Up) {
            scroll -= 1.0;
        }
        if self.rep_pressed(KeyCode::Down) {
            scroll += 1.0;
        }
        let (_, wy) = mouse_wheel();
        if wy != 0.0 {
            scroll -= wy.signum() * 3.0;
        }
        if self.clicked(&self.hits.log_up.clone()) {
            scroll -= page;
        }
        if self.clicked(&self.hits.log_down.clone()) {
            scroll += page;
        }
        if scroll != 0.0 {
            self.ui.press(if scroll < 0.0 { "sb_up" } else { "sb_down" });
            self.log_scroll += scroll;
            self.log_follow = false;
        }
        if is_key_pressed(KeyCode::Home) && !running {
            self.log_scroll = 0.0;
            self.log_follow = false;
        }
        if is_key_pressed(KeyCode::End) {
            self.log_follow = true;
        }

        if running {
            if ctrl && is_key_pressed(KeyCode::C) {
                if let Some(j) = self.job.as_mut() {
                    j.stop(now);
                }
                self.chars.clear();
                return;
            }
            if ctrl && is_key_pressed(KeyCode::V) {
                if let Some(s) = mq_window::clipboard_get() {
                    self.log_input.push_str(s.lines().next().unwrap_or(""));
                }
            }
            for ch in std::mem::take(&mut self.chars) {
                if !ch.is_control() && !ctrl {
                    self.log_input.push(ch);
                }
            }
            if self.rep_pressed(KeyCode::Backspace) {
                self.log_input.pop();
            }
            if is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter) {
                let line = std::mem::take(&mut self.log_input);
                if let Some(j) = self.job.as_mut() {
                    j.send_line(&line);
                }
                self.log_follow = true;
                self.fx(Fx::Tick);
            }
            if is_key_pressed(KeyCode::Escape) {
                self.fx(Fx::Accept);
                let title = self.job.as_ref().map(|j| j.spec.title.clone()).unwrap_or_default();
                self.dialog = Some(Dialog::Confirm { text: format!("Stop “{title}”?"), yes: false, action: DAction::StopJob, opened: now });
            }
        } else {
            if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Backspace) {
                self.back();
                return;
            }
            if is_key_pressed(KeyCode::R) {
                if let Some(spec) = self.job.as_ref().map(|j| j.spec.clone()) {
                    self.fx(Fx::Accept);
                    self.start_job(spec);
                }
            }
            if is_key_pressed(KeyCode::C) {
                if let Some(j) = self.job.as_ref() {
                    let text = j.term.lock().unwrap().plain();
                    mq_window::clipboard_set(&text);
                    self.fx(Fx::Tick);
                    self.toast("Log copied to the clipboard", LIME);
                }
            }
        }
    }

    fn options_input(&mut self) {
        let n = OPT_ROWS.len();
        if self.rep_pressed(KeyCode::Up) || self.clicked(&self.hits.log_up.clone()) {
            if self.opt_sel > 0 {
                self.opt_sel -= 1;
                self.ui.press("sb_up");
                self.fx(Fx::Tick);
            } else {
                self.fx(Fx::Error);
            }
        }
        if self.rep_pressed(KeyCode::Down) || self.clicked(&self.hits.log_down.clone()) {
            if self.opt_sel + 1 < n {
                self.opt_sel += 1;
                self.ui.press("sb_down");
                self.fx(Fx::Tick);
            } else {
                self.fx(Fx::Error);
            }
        }
        let mut d = 0i32;
        if self.rep_pressed(KeyCode::Left) || self.clicked(&self.hits.row_prev.clone()) {
            d = -1;
        }
        if self.rep_pressed(KeyCode::Right) || self.clicked(&self.hits.row_next.clone()) {
            d = 1;
        }
        if is_mouse_button_pressed(MouseButton::Left) && d == 0 {
            let m = self.ui.mouse();
            if let Some(i) = self.hits.rows.iter().find(|(r, _)| hit(r, m)).map(|(_, i)| *i) {
                if i != self.opt_sel {
                    self.opt_sel = i;
                    self.fx(Fx::Tick);
                } else if i == OPT_RESTORE {
                    d = 1;
                }
            }
        }
        if (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter)) && self.opt_sel == OPT_RESTORE {
            d = 1;
        }
        if d != 0 {
            self.ui.press(if d < 0 { "row_l" } else { "row_r" });
            self.change_option(d);
        }
        if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Backspace) {
            self.back();
        }
    }

    fn change_option(&mut self, d: i32) {
        let o = &mut self.opts;
        match self.opt_sel {
            0 => o.sound = !o.sound,
            1 => o.volume = (o.volume as i32 + d).clamp(0, 10) as u8,
            2 => o.spin = !o.spin,
            3 => o.anim_full = !o.anim_full,
            4 => {
                o.fullscreen = !o.fullscreen;
                set_fullscreen(o.fullscreen);
            }
            5 => o.phone = ((o.phone as i32 + d).rem_euclid(3)) as u8,
            6 => o.log_size = (o.log_size as i32 + d).clamp(12, 24) as u8,
            7 => o.show_count = !o.show_count,
            8 => o.show_desc = !o.show_desc,
            9 => o.show_tags = !o.show_tags,
            10 => o.show_hints = !o.show_hints,
            _ => {
                self.dialog = Some(Dialog::Confirm { text: "Restore every option to its default?".into(), yes: false, action: DAction::ResetOptions, opened: self.ui.t });
                self.fx(Fx::Accept);
                return;
            }
        }
        self.save_opts();
        self.fx(Fx::Tick);
    }

    fn dialog_input(&mut self) {
        let mouse_click = is_mouse_button_pressed(MouseButton::Left);
        let m = self.ui.mouse();
        let yes_hit = mouse_click && self.hits.dlg_yes.map(|r| hit(&r, m)).unwrap_or(false);
        let no_hit = mouse_click && self.hits.dlg_no.map(|r| hit(&r, m)).unwrap_or(false);
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        let chars = std::mem::take(&mut self.chars);
        let left = self.rep_pressed(KeyCode::Left);
        let right = self.rep_pressed(KeyCode::Right);
        let backspace = self.rep_pressed(KeyCode::Backspace);
        let enter = is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter);
        let esc = is_key_pressed(KeyCode::Escape);
        let Some(dlg) = self.dialog.as_mut() else { return };
        match dlg {
            Dialog::Confirm { yes, action, .. } => {
                if left || right || is_key_pressed(KeyCode::Tab) {
                    *yes = !*yes;
                    self.sfx.play(Fx::Tick, self.opts.sound, self.opts.volume);
                }
                if is_key_pressed(KeyCode::Y) {
                    *yes = true;
                }
                let decide = if yes_hit {
                    Some(true)
                } else if no_hit || esc || is_key_pressed(KeyCode::N) {
                    Some(false)
                } else if enter || is_key_pressed(KeyCode::Y) || is_key_pressed(KeyCode::Space) {
                    Some(*yes || is_key_pressed(KeyCode::Y))
                } else {
                    None
                };
                if let Some(ok) = decide {
                    let action = action.clone();
                    self.dialog = None;
                    if ok {
                        self.fx(Fx::Accept);
                        self.dialog_choice(action);
                    } else {
                        self.fx(Fx::Back);
                    }
                }
            }
            Dialog::Message { .. } => {
                if enter || esc || yes_hit || no_hit || is_key_pressed(KeyCode::Space) {
                    self.dialog = None;
                    self.fx(Fx::Back);
                }
            }
            Dialog::Input { text, allow_empty, pending, .. } => {
                if ctrl && is_key_pressed(KeyCode::V) {
                    if let Some(s) = mq_window::clipboard_get() {
                        text.push_str(s.lines().next().unwrap_or("").trim());
                    }
                }
                for ch in chars {
                    if !ch.is_control() && !ctrl {
                        text.push(ch);
                    }
                }
                if backspace {
                    text.pop();
                }
                if esc || no_hit {
                    self.dialog = None;
                    self.fx(Fx::Back);
                } else if enter || yes_hit {
                    if text.trim().is_empty() && !*allow_empty {
                        self.fx(Fx::Error);
                    } else {
                        let p = Pending { text: Some(text.clone()), ..pending.clone() };
                        self.dialog = None;
                        self.fx(Fx::Accept);
                        self.proceed(p);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------ geometry

    /// Log panel: (x, y, w, h, char_w, line_h, cols)
    fn log_geom(&self) -> (f32, f32, f32, f32, f32, f32, usize) {
        let ui = &self.ui;
        let (x, y) = (24.0, 96.0);
        let (w, h) = (ui.w - 48.0, ui.h - 96.0 - FOOTER_H - 12.0);
        let size = self.opts.log_size as f32;
        let cw = ui.measure_with(&ui.mono, "MMMMMMMMMM", size) / 10.0;
        let lh = (size * 1.34).round();
        let text_w = w - 24.0 - 48.0 - 16.0;
        let cols = ((text_w / cw).floor() as usize).max(20);
        (x, y, w, h, cw, lh, cols)
    }

    fn log_geom_rows(&self) -> (f32, f32, usize) {
        let (_, _, _, h, _, lh, _) = self.log_geom();
        let running = self.job.as_ref().map(|j| j.running()).unwrap_or(false);
        let text_h = h - 24.0 - 16.0 - if running { 44.0 } else { 0.0 };
        (text_h, lh, ((text_h / lh).floor() as usize).max(1))
    }

    // ------------------------------------------------------------------ drawing

    fn draw(&mut self, dt: f32) {
        let now = self.now();
        let full = matches!(self.screen(), Screen::Log | Screen::Options | Screen::Dyno | Screen::Tuner);
        self.dim.target = if full { 1.0 } else if self.dialog.is_some() { 0.45 } else { 0.0 };
        self.dim.update(dt, 8.0);
        // easter egg: hold the middle button and drag to spin the turntable
        // and raise the camera (only while the garage is actually visible)
        let m = self.ui.mouse();
        if is_mouse_button_down(MouseButton::Middle) && self.dim.value < 0.6 && m.x > -999.0 {
            if let Some(last) = self.grab_last {
                self.scene.grab(m - last, now);
            }
            self.grab_last = Some(m);
        } else {
            self.grab_last = None;
        }
        self.scene.draw(dt, self.opts.spin, now, self.dim.value);

        self.hits = Hits::default();
        let (out_t, in_t) = self.times();
        let alpha_out = match self.pending_nav {
            Some((t0, _)) => 1.0 - smooth(t0, t0 + out_t, now),
            None => 1.0,
        };
        let a_in = smooth(self.enter_at, self.enter_at + in_t, now);
        let hl = smooth(self.enter_at + in_t * 0.6, self.enter_at + in_t * 0.6 + 0.2, now);
        let alpha = alpha_out * a_in;
        let slide = (1.0 - ease_out(a_in)) * 34.0 * self.dir;

        self.ui.alpha = alpha;
        self.ui.dx = slide;
        match self.screen() {
            Screen::Main => self.draw_main(hl),
            Screen::Tool(ti) => self.draw_tool(ti, hl),
            Screen::Log => self.draw_log(),
            Screen::Dyno => {
                self.header("Live Dyno", None);
                self.dyno.draw(&self.ui, dt);
            }
            Screen::Options => self.draw_options(),
            Screen::Tuner => {
                let tele = self.dyno.tele();
                self.tuner.draw(&self.ui, &tele, dt, hl);
            }
        }
        self.ui.alpha = 1.0;
        self.ui.dx = 0.0;
        self.draw_footer();
        self.draw_dialog();
        self.draw_toast();
    }

    fn header(&self, title: &str, sub: Option<&str>) {
        let ui = &self.ui;
        ui.text_sh(title, 34.0, 62.0, 46.0, LIME, Align::Left);
        if let Some(s) = sub {
            let tw = ui.measure(title, 46.0);
            ui.text_sh(s, 34.0 + tw + 18.0, 62.0, 22.0, TXT_DIM, Align::Left);
        }
    }

    fn logo(&self) {
        let ui = &self.ui;
        let (x, y) = (30.0, 58.0);
        let s = "TIRAMISU";
        let size = 50.0;
        let navy = Color::new(0.04, 0.09, 0.19, 1.0);
        ui.text_with(&ui.black, s, x + 4.0, y + 4.0, size, Color::new(0.0, 0.0, 0.0, 0.7), Align::Left);
        for (ox, oy) in [(-2.5, 0.0), (2.5, 0.0), (0.0, -2.5), (0.0, 2.5), (-1.8, -1.8), (1.8, -1.8), (-1.8, 1.8), (1.8, 1.8)] {
            ui.text_with(&ui.black, s, x + ox, y + oy, size, navy, Align::Left);
        }
        let w = ui.text_with(&ui.black, s, x, y, size, Color::new(0.70, 0.83, 0.96, 1.0), Align::Left);
        ui.text_with(&ui.black, s, x, y - 1.2, size, Color::new(1.0, 1.0, 1.0, 0.35), Align::Left);
        // second word tucked under the right end, with a lime slash leading in
        let right = x + w;
        let gw = ui.measure_with(&ui.black, "GARAGE", 22.0);
        ui.rrect(right - gw - 96.0, y + 13.0, 84.0, 4.0, 2.0, LIME);
        ui.text_with(&ui.black, "GARAGE", right + 2.0, y + 27.0, 22.0, navy, Align::Right);
        ui.text_with(&ui.black, "GARAGE", right, y + 25.0, 22.0, LIME, Align::Right);
    }

    fn draw_track_and_strip(&mut self, s: Screen, items: &[StripItem], hl: f32) {
        let now = self.now();
        let ui = &self.ui;
        let w = ui.w;
        let n = items.len();
        let st = match s {
            Screen::Main => &self.main_strip,
            Screen::Tool(i) => &self.tool_strips[i],
            _ => return,
        };
        // position track with its little marker
        let (tx0, tx1, ty) = (24.0, w - 24.0, 74.0);
        ui.rrect(tx0, ty, tx1 - tx0, 9.0, 4.5, Color::new(0.65, 0.68, 0.68, 0.30));
        let dx = tx0 + 10.0 + (tx1 - tx0 - 20.0) * st.dot.value;
        ui.glow_soft(dx, ty + 4.5, 12.0, Color::new(1.0, 1.0, 1.0, 0.45));
        ui.disc(dx, ty + 4.5, 4.2, TXT);

        // the strip
        ui.rrect_grad(24.0, STRIP_Y, w - 48.0, STRIP_H, 14.0, Color::new(0.62, 0.65, 0.65, 0.42), Color::new(0.45, 0.48, 0.48, 0.40));
        ui.rrect_line(24.0, STRIP_Y, w - 48.0, STRIP_H, 14.0, 1.0, Color::new(1.0, 1.0, 1.0, 0.12));
        let cy = STRIP_Y + STRIP_H * 0.5;
        let lp = Rect::new(24.0 + 40.0 - 22.0, cy - 22.0, 44.0, 44.0);
        let rp = Rect::new(w - 24.0 - 40.0 - 22.0, cy - 22.0, 44.0, 44.0);
        ui.arrow_btn("strip_l", lp.x + 22.0, cy, 21.0, -1, self.hovered(&lp), st.sel > 0);
        ui.arrow_btn("strip_r", rp.x + 22.0, cy, 21.0, 1, self.hovered(&rp), st.sel + 1 < n);

        let cxm = w * 0.5;
        let (x0, x1) = (24.0 + 60.0, w - 24.0 - 60.0);
        let since = (now - st.changed_at) as f32;
        let _ = n;

        // selection box: slides, its lime rim pulses, and it flashes on change
        let bx = cxm + st.boxpos.value * SLOT;
        let (bw, bh) = (204.0, 116.0); // wide like the original, not square
        let pulse = 0.5 + 0.5 * ((now - st.changed_at) as f32 * std::f32::consts::TAU / 1.25).cos();
        let rim = hl * (0.30 + 0.70 * pulse);
        ui.rrect(bx - bw * 0.5, cy - bh * 0.5, bw, bh, 14.0, Color::new(0.10, 0.11, 0.11, 0.66));
        let flash = (1.0 - since / 0.28).clamp(0.0, 1.0) * hl;
        if flash > 0.0 {
            ui.rrect(bx - bw * 0.5, cy - bh * 0.5, bw, bh, 14.0, Color::new(LIME.r, LIME.g, LIME.b, 0.28 * flash));
        }
        ui.rrect_line(bx - bw * 0.5 - 1.0, cy - bh * 0.5 - 1.0, bw + 2.0, bh + 2.0, 15.0, 3.0, Color::new(LIME.r, LIME.g, LIME.b, rim));

        let mut hits = Vec::new();
        for (i, it) in items.iter().enumerate() {
            let x = cxm + (i as f32 - st.scroll.value) * SLOT;
            let d = (x - cxm) / SLOT;
            // far icons fade with distance; never draw under the arrow buttons
            let edge = ((x - x0) / 60.0).clamp(0.0, 1.0) * ((x1 - x) / 60.0).clamp(0.0, 1.0);
            let fade = strip_fade(d) * edge;
            if fade <= 0.01 {
                continue;
            }
            let selected = i == st.sel;
            let pop = if selected { 1.0 + 0.13 * (-since * 11.0).exp() } else { 1.0 };
            let size = if selected { 84.0 } else { 70.0 } * pop * (1.0 - 0.14 * smooth(1.0, 2.0, d.abs() as f64));
            let base = if it.dim { 0.32 } else if selected { 1.0 } else { 0.80 };
            let mut col = Color::new(0.95, 0.96, 0.96, base * fade);
            if selected && !it.dim {
                let k = 0.30 * rim;
                col = Color::new(col.r + (LIME.r - col.r) * k, col.g + (LIME.g - col.g) * k, col.b + (LIME.b - col.b) * k, col.a);
            }
            ui.icon(it.glyph, x, cy + 2.0, size, col);
            hits.push((Rect::new(x - SLOT * 0.5, STRIP_Y, SLOT, STRIP_H), i));
        }

        // label under the strip, in lime; stays centred like the game's, even when the box slides to an end
        if let Some(it) = items.get(st.sel) {
            ui.text_sh(&it.label, w * 0.5, STRIP_Y + STRIP_H + 40.0, 32.0, Color::new(LIME.r, LIME.g, LIME.b, hl.max(0.25)), Align::Center);
        }
        self.hits.items = hits;
        self.hits.prev = Some(lp);
        self.hits.next = Some(rp);
    }

    fn draw_main(&mut self, hl: f32) {
        self.logo();
        let ui = &self.ui;
        ui.text_sh("Main Menu", ui.w - 34.0, 58.0, 38.0, LIME, Align::Right);
        let items = self.main_items();
        self.draw_track_and_strip(Screen::Main, &items, hl);
        let ui = &self.ui;
        let i = self.main_strip.sel;
        let tool = &self.tools[i];
        let y = STRIP_Y + STRIP_H + 76.0;
        let mut ly = y;
        if self.opts.show_desc {
            ui.text_sh(tool.blurb, ui.w * 0.5, ly, 22.0, TXT, Align::Center);
            ly += 28.0;
        }
        // "not installed" always shows: it explains why Enter does nothing
        if !self.installed[i] {
            ui.text_sh(&format!("{} is not installed", tool.needs), ui.w * 0.5, ly, 17.0, RED, Align::Center);
        } else if self.opts.show_count && tool.cmds.len() > 1 {
            ui.text_sh(&format!("{} commands inside", tool.cmds.len()), ui.w * 0.5, ly, 17.0, TXT_DIM, Align::Center);
        }
    }

    fn draw_tool(&mut self, ti: usize, hl: f32) {
        let tool = self.tools[ti].clone();
        self.header(tool.name, None);
        // phone picker in the header, like a "◀ All Cars ▶" filter
        if tool.phone_picker {
            let ui = &self.ui;
            let label = Phone::from_index(self.opts.phone).label();
            let rx = ui.w - 52.0;
            let tw = ui.measure(label, 24.0).max(150.0);
            let lx = rx - 34.0 - tw - 34.0;
            let rp = Rect::new(rx - 16.0, 26.0, 32.0, 32.0);
            let lp = Rect::new(lx - 16.0, 26.0, 32.0, 32.0);
            ui.arrow_btn("cyc_l", lx, 42.0, 15.0, -1, self.hovered(&lp), true);
            ui.arrow_btn("cyc_r", rx, 42.0, 15.0, 1, self.hovered(&rp), true);
            ui.text_sh(label, (lx + rx) * 0.5, 50.0, 24.0, TXT, Align::Center);
            ui.text_sh("PHONE", lx - 26.0, 50.0, 15.0, TXT_DIM, Align::Right);
            self.hits.cyc_prev = Some(lp);
            self.hits.cyc_next = Some(rp);
        }
        let items = self.tool_items(ti);
        self.draw_track_and_strip(Screen::Tool(ti), &items, hl);
        let ui = &self.ui;
        let cmd = &tool.cmds[self.tool_strips[ti].sel];
        let y = STRIP_Y + STRIP_H + 76.0;
        if self.opts.show_desc {
            let desc = ui.fit(cmd.desc, 22.0, ui.w - 120.0);
            ui.text_sh(&desc, ui.w * 0.5, y, 22.0, TXT, Align::Center);
        }
        let tags = if self.opts.show_tags { catalog::tags(cmd) } else { Vec::new() };
        let y = if self.opts.show_desc { y } else { y - 30.0 };
        if !tags.is_empty() {
            let widths: f32 = tags.iter().map(|t| ui.measure_with(&ui.black, t, 14.0) + 26.0).sum();
            let mut x = ui.w * 0.5 - widths * 0.5;
            for t in tags {
                let col = match t {
                    "ASKS FIRST" => AMBER,
                    "KONSOLE" => Color::new(0.55, 0.75, 1.0, 1.0),
                    _ => LIME,
                };
                x += ui.chip(x, y + 30.0, t, col) + 10.0;
            }
        }
    }

    fn draw_log(&mut self) {
        let now = self.now();
        let (x, y, w, h, cw, lh, cols) = self.log_geom();
        let (text_h, _, rows) = self.log_geom_rows();
        let Some(job) = self.job.as_ref() else { return };
        let title = job.spec.title.clone();
        let glyph = job.spec.glyph;
        let state = job.state.clone();
        let elapsed = job.elapsed(now);
        let running = job.running();
        let size = self.opts.log_size as f32;
        let ui = &self.ui;

        // header: icon + title, status on the right
        ui.icon(glyph, 58.0, 44.0, 44.0, LIME);
        let t = ui.fit(&title, 40.0, ui.w - 520.0);
        ui.text_sh(&t, 92.0, 60.0, 40.0, LIME, Align::Left);
        let stopped = job.stopped_by_user();
        let (stxt, scol) = match &state {
            JobState::Running => (format!("RUNNING   {:.0} s", elapsed), LIME),
            JobState::Exited(c) if stopped => (format!("STOPPED   exit {c}   ·   {:.1} s", elapsed), AMBER),
            JobState::Exited(0) => (format!("FINISHED   exit 0   ·   {:.1} s", elapsed), LIME),
            JobState::Exited(c) => (format!("FAILED   exit {c}   ·   {:.1} s", elapsed), RED),
            JobState::Failed(e) => (format!("ERROR   {e}"), RED),
        };
        let sx = ui.w - 34.0;
        let sw = ui.text_sh(&stxt, sx, 56.0, 22.0, scol, Align::Right);
        let dot_x = sx - sw - 18.0;
        if running {
            let p = 0.5 + 0.5 * ((now * 5.0).sin() as f32);
            ui.glow_soft(dot_x, 48.0, 16.0, Color::new(LIME.r, LIME.g, LIME.b, 0.5 * p));
            ui.glyph(ico::DOT, dot_x, 48.0, 22.0, LIME);
        } else {
            ui.glyph(if matches!(state, JobState::Exited(0)) { ico::CHECK } else { ico::ALERT }, dot_x, 48.0, 24.0, scol);
        }

        // pale frame + dark screen inset
        ui.rrect_grad(x, y, w, h, 16.0, Color::new(0.75, 0.77, 0.77, 0.30), Color::new(0.55, 0.57, 0.57, 0.28));
        let (ix, iy, iw, ih) = (x + 12.0, y + 12.0, w - 24.0 - 44.0, h - 24.0);
        ui.rrect(ix, iy, iw, ih, 10.0, Color::new(0.02, 0.025, 0.03, 0.88));

        // wrap lines into rows
        let term = job.term.lock().unwrap();
        let mut vis: Vec<(usize, usize)> = Vec::new(); // (line, start col)
        for (li, l) in term.lines.iter().enumerate() {
            if l.is_empty() {
                vis.push((li, 0));
            } else {
                let mut c = 0;
                while c < l.len() {
                    vis.push((li, c));
                    c += cols;
                }
            }
        }
        // drop trailing empty rows
        while vis.len() > 1 && term.lines[vis.last().unwrap().0].is_empty() && vis.last().unwrap().0 == term.lines.len() - 1 {
            vis.pop();
        }
        let total = vis.len();
        let max_top = total.saturating_sub(rows) as f32;
        if term.version != self.log_seen_version && self.log_follow {
            self.log_scroll = max_top;
        }
        if self.log_follow {
            self.log_scroll = max_top;
        }
        self.log_scroll = self.log_scroll.clamp(0.0, max_top);
        if self.log_scroll >= max_top && !self.log_follow && total > 0 {
            self.log_follow = true;
        }
        self.log_seen_version = term.version;
        let top = self.log_scroll.round() as usize;
        let ui = &self.ui;
        let tx = ix + 12.0;
        let mut ty = iy + 8.0 + size;
        for &(li, c0) in vis.iter().skip(top).take(rows) {
            let line = &term.lines[li];
            let end = (c0 + cols).min(line.len());
            let mut c = c0;
            while c < end {
                let st = line[c].st;
                let mut run = String::new();
                let mut k = c;
                while k < end && line[k].st == st {
                    run.push(line[k].ch);
                    k += 1;
                }
                if !run.trim().is_empty() {
                    let mut col = match st.fg {
                        Some([r, g, b]) => Color::from_rgba(r, g, b, 255),
                        None => Color::new(0.86, 0.88, 0.89, 1.0),
                    };
                    if st.bold && st.fg.is_none() {
                        col = TXT;
                    }
                    if st.dim {
                        col.a = 0.6;
                    }
                    let font = &ui.mono;
                    ui.text_with(font, &run, tx + (c - c0) as f32 * cw, ty, size, col, Align::Left);
                    if st.bold {
                        ui.text_with(font, &run, tx + (c - c0) as f32 * cw + 0.6, ty, size, col, Align::Left);
                    }
                }
                c = k;
            }
            ty += lh;
        }
        drop(term);
        let _ = text_h;

        // scrollbar with round up/down buttons
        let sbx = x + w - 34.0;
        let up = Rect::new(sbx - 15.0, iy, 30.0, 30.0);
        let dn = Rect::new(sbx - 15.0, iy + ih - 30.0, 30.0, 30.0);
        ui.arrow_btn("sb_up", sbx, iy + 15.0, 14.0, -2, self.hovered(&up), self.log_scroll > 0.0);
        ui.arrow_btn("sb_down", sbx, iy + ih - 15.0, 14.0, 2, self.hovered(&dn), self.log_scroll < max_top);
        let ui = &self.ui;
        let (ty0, ty1) = (iy + 38.0, iy + ih - 38.0);
        ui.rrect(sbx - 3.0, ty0, 6.0, ty1 - ty0, 3.0, Color::new(0.0, 0.0, 0.0, 0.45));
        if total > rows {
            let frac = rows as f32 / total as f32;
            let th = ((ty1 - ty0) * frac).max(24.0);
            let pos = if max_top > 0.0 { self.log_scroll / max_top } else { 1.0 };
            let tyy = ty0 + (ty1 - ty0 - th) * pos;
            ui.rrect(sbx - 4.0, tyy, 8.0, th, 4.0, TXT);
        }
        self.hits.log_up = Some(up);
        self.hits.log_down = Some(dn);

        // input line for prompts
        if running {
            let iy2 = iy + ih - 40.0;
            ui.line(ix + 10.0, iy2, ix + iw - 10.0, iy2, 1.0, Color::new(1.0, 1.0, 1.0, 0.15));
            ui.text_with(&ui.mono, "›", tx, iy2 + 27.0, size + 2.0, LIME, Align::Left);
            let px = tx + cw * 2.0;
            if self.log_input.is_empty() {
                ui.text("type an answer for the script (y / n …) and press Enter", px, iy2 + 26.0, 18.0, Color::new(0.6, 0.63, 0.64, 0.8), Align::Left);
            } else {
                ui.text_with(&ui.mono, &self.log_input, px, iy2 + 27.0, size, TXT, Align::Left);
            }
            if (now * 2.0).fract() < 0.55 {
                let cx = px + self.log_input.chars().count() as f32 * cw;
                ui.rect(cx, iy2 + 10.0, 2.0, size + 4.0, LIME);
            }
        }
    }

    fn draw_options(&mut self) {
        self.header("Options", None);
        let ui = &self.ui;
        let (x, y) = (60.0, 100.0);
        let (w, h) = (ui.w - 120.0, ui.h - 100.0 - FOOTER_H - 14.0);
        // same look as the dialogs: dark button grey, pale lime frame
        ui.rrect(x, y, w, h, 14.0, BTN);
        ui.rrect_line(x, y, w, h, 14.0, 2.5, FRAME);
        let lime = self.lime_pulse(0.0);
        let o = &self.opts;
        let row_h = 52.0;
        let vx = x + w * 0.58; // value column
        // the list scrolls when it is taller than the panel
        let list_top = y + 22.0;
        let list_h = h - 22.0 - 44.0;
        let visible = ((list_h / row_h).floor() as usize).max(1);
        let top = self.opt_top.value;
        let mut rows = Vec::new();
        let (mut rp, mut rn) = (None, None);
        for (i, name) in OPT_ROWS.iter().enumerate() {
            let pos = i as f32 - top;
            // rows fade out as they slide past the top or bottom edge
            let fade = (pos + 0.9).clamp(0.0, 1.0) * (visible as f32 - pos + 0.1).clamp(0.0, 1.0);
            if fade <= 0.02 {
                continue;
            }
            let ui = &self.ui.faded(fade);
            let ry = list_top + pos * row_h;
            let sel = i == self.opt_sel;
            let rr = Rect::new(x + 18.0, ry, w - 36.0 - 40.0, row_h - 6.0);
            if sel {
                ui.rrect(rr.x, rr.y, rr.w, rr.h, 9.0, BTN_SEL);
            }
            ui.text_sh(name, rr.x + 24.0, ry + 33.0, 27.0, if sel { lime } else { TXT }, Align::Left);
            let slider = |val: f32, max: f32| {
                let (sx, sw) = (vx + 70.0, w * 0.22);
                ui.rrect(sx, ry + 18.0, sw, 9.0, 4.5, Color::new(0.08, 0.09, 0.09, 0.7));
                let f = (val / max).clamp(0.0, 1.0);
                ui.rrect(sx, ry + 18.0, (sw * f).max(9.0), 9.0, 4.5, Color::new(0.33, 0.70, 0.18, 1.0));
                ui.disc(sx + sw * f, ry + 22.5, 7.5, TXT);
            };
            let value: Option<String> = match i {
                0 => Some(if o.sound { "On" } else { "Off" }.into()),
                1 => {
                    slider(o.volume as f32, 10.0);
                    None
                }
                2 => Some(if o.spin { "Spin" } else { "Still" }.into()),
                3 => Some(if o.anim_full { "Full" } else { "Quick" }.into()),
                4 => Some(if o.fullscreen { "On" } else { "Off" }.into()),
                7 => Some(if o.show_count { "On" } else { "Off" }.into()),
                8 => Some(if o.show_desc { "On" } else { "Off" }.into()),
                9 => Some(if o.show_tags { "On" } else { "Off" }.into()),
                10 => Some(if o.show_hints { "On" } else { "Off" }.into()),
                5 => Some(Phone::from_index(o.phone).label().into()),
                6 => {
                    slider((o.log_size - 12) as f32, 12.0);
                    ui.text_sh(&format!("{} px", o.log_size), vx + 70.0 + w * 0.22 + 20.0, ry + 31.0, 20.0, TXT, Align::Left);
                    None
                }
                _ => Some("Press Enter".into()),
            };
            if let Some(v) = value {
                ui.text_sh(&v, vx + 70.0 + w * 0.11, ry + 33.0, 26.0, TXT, Align::Center);
            }
            if sel {
                let (can_dec, can_inc) = match i {
                    1 => (o.volume > 0, o.volume < 10),
                    6 => (o.log_size > 12, o.log_size < 24),
                    _ => (true, true),
                };
                let lp = Rect::new(vx + 10.0 - 15.0, ry + 8.0, 30.0, 30.0);
                let np = Rect::new(rr.x + rr.w - 34.0 - 15.0, ry + 8.0, 30.0, 30.0);
                ui.arrow_btn("row_l", vx + 10.0, ry + 23.0, 15.0, -1, self.hovered(&lp), can_dec);
                ui.arrow_btn("row_r", rr.x + rr.w - 34.0, ry + 23.0, 15.0, 1, self.hovered(&np), can_inc);
                rp = Some(lp);
                rn = Some(np);
            }
            if fade > 0.6 {
                rows.push((rr, i));
            }
        }
        // decorative scrollbar, like the original list screens
        let sbx = x + w - 28.0;
        let up = Rect::new(sbx - 15.0, y + 15.0, 30.0, 30.0);
        let dn = Rect::new(sbx - 15.0, y + h - 45.0, 30.0, 30.0);
        self.ui.arrow_btn("sb_up", sbx, y + 30.0, 14.0, -2, self.hovered(&up), self.opt_sel > 0);
        self.ui.arrow_btn("sb_down", sbx, y + h - 30.0, 14.0, 2, self.hovered(&dn), self.opt_sel + 1 < OPT_ROWS.len());
        let ui = &self.ui;
        ui.rrect(sbx - 3.0, y + 52.0, 6.0, h - 104.0, 3.0, Color::new(0.0, 0.0, 0.0, 0.35));
        let f = self.opt_dot.value;
        let dy = y + 58.0 + (h - 116.0) * f;
        ui.glow_soft(sbx, dy, 14.0, Color::new(1.0, 1.0, 1.0, 0.35));
        ui.disc(sbx, dy, 6.0, TXT);
        ui.text_sh(
            "Saved to ~/.config/tiramisu-garage/options.conf as you change them.",
            x + 24.0,
            y + h - 20.0,
            17.0,
            TXT_DIM,
            Align::Left,
        );
        self.hits.rows = rows;
        self.hits.log_up = Some(up);
        self.hits.log_down = Some(dn);
        self.hits.row_prev = rp;
        self.hits.row_next = rn;
    }

    /// The footer buttons of the current screen, most important first:
    /// they fill the grid from the right, top row before bottom row.
    fn footer_buttons(&self) -> Vec<FBtn> {
        use KeyCode as K;
        const ENTER: &[KeyCode] = &[K::Enter, K::KpEnter];
        const ESC: &[KeyCode] = &[K::Escape];
        if self.dialog.is_some() {
            return Vec::new();
        }
        let running = self.job.as_ref().map(|j| j.running()).unwrap_or(false);
        let mut v = match self.screen() {
            Screen::Main => vec![fb("f_select", "Select", "ENTER", ENTER, FAct::Select), fb("f_quit", "Quit Garage", "ESC", ESC, FAct::Quit)],
            Screen::Tool(ti) => {
                let mut v = vec![fb("f_run", "Run", "ENTER", ENTER, FAct::Select), fb("f_back", "Back", "ESC", ESC, FAct::Back)];
                if self.tools[ti].phone_picker {
                    v.push(fb("f_phone", "Next Phone", "UP/DN", &[K::Up, K::Down], FAct::PhoneNext));
                }
                v
            }
            Screen::Log if running => vec![fb("f_send", "Send Answer", "ENTER", ENTER, FAct::SendAnswer), fb("f_stop", "Stop", "ESC", ESC, FAct::Stop)],
            Screen::Log => vec![
                fb("f_again", "Run Again", "R", &[K::R], FAct::RunAgain),
                fb("f_back", "Back", "ESC", ESC, FAct::Back),
                fb("f_copy", "Copy Log", "C", &[K::C], FAct::CopyLog),
            ],
            Screen::Dyno => vec![fb("f_tuner", "Fan Curve Tuner", "ENTER", ENTER, FAct::OpenTuner), fb("f_back", "Back", "ESC", ESC, FAct::Back)],
            Screen::Tuner => vec![
                fb("f_apply", "Apply", "ENTER", ENTER, FAct::TunerPill(4)),
                fb("f_back", "Back", "ESC", ESC, FAct::Back),
                fb("f_reset", "Reset to default", "D", &[K::D], FAct::TunerPill(3)),
                fb("f_revert", "Revert", "R", &[K::R], FAct::TunerPill(2)),
                fb("f_preset", "Next Preset", "P", &[K::P], FAct::TunerPill(1)),
                fb("f_link", if self.tuner.linked() { "Unlink GPU" } else { "Link GPU" }, "L", &[K::L], FAct::TunerPill(0)),
                fb("f_help", "Tip", "H", &[K::H], FAct::TunerHelp),
                fb("f_tab", "CPU / GPU", "TAB", &[K::Tab], FAct::TunerTab),
            ],
            Screen::Options => vec![fb("f_reset", "Reset to default", "", &[], FAct::RestoreDefaults), fb("f_back", "Back", "ESC", ESC, FAct::Back)],
        };
        if self.job.is_some() && !matches!(self.screen(), Screen::Log | Screen::Tuner) {
            v.push(fb("f_log", if running { "Job Log (running)" } else { "Last Log" }, "L", &[K::L], FAct::LastLog));
        }
        v
    }

    fn footer_act(&mut self, act: FAct) {
        let now = self.now();
        match act {
            FAct::Select => self.activate(),
            FAct::Quit => self.ask_quit(),
            FAct::Back => self.back(),
            FAct::LastLog => {
                if self.job.is_some() {
                    self.fx(Fx::Accept);
                    self.go(Nav::Push(Screen::Log));
                }
            }
            FAct::PhoneNext => {
                self.ui.press("cyc_r");
                self.opts.phone = (self.opts.phone + 1) % 3;
                self.save_opts();
                self.fx(Fx::Tick);
            }
            FAct::SendAnswer => {
                let line = std::mem::take(&mut self.log_input);
                if let Some(j) = self.job.as_mut() {
                    j.send_line(&line);
                }
                self.log_follow = true;
                self.fx(Fx::Tick);
            }
            FAct::Stop => {
                self.fx(Fx::Accept);
                let title = self.job.as_ref().map(|j| j.spec.title.clone()).unwrap_or_default();
                self.dialog = Some(Dialog::Confirm { text: format!("Stop “{title}”?"), yes: false, action: DAction::StopJob, opened: now });
            }
            FAct::RunAgain => {
                if let Some(spec) = self.job.as_ref().map(|j| j.spec.clone()) {
                    self.fx(Fx::Accept);
                    self.start_job(spec);
                }
            }
            FAct::CopyLog => {
                if let Some(j) = self.job.as_ref() {
                    let text = j.term.lock().unwrap().plain();
                    mq_window::clipboard_set(&text);
                    self.fx(Fx::Tick);
                    self.toast("Log copied to the clipboard", LIME);
                }
            }
            FAct::OpenTuner => {
                self.fx(Fx::Accept);
                self.go(Nav::Push(Screen::Tuner));
            }
            FAct::TunerPill(i) => {
                let mut ev = Vec::new();
                if self.tuner.loaded && !self.tuner.busy() {
                    self.tuner.pill_action(i, &mut ev);
                }
                self.tuner_events(ev);
            }
            FAct::TunerHelp => self.dialog = Some(Dialog::Message { text: tuner::HELP.into(), opened: now }),
            FAct::TunerTab => {
                self.tuner.toggle_tab(&self.ui, now);
                self.fx(Fx::Whoosh);
            }
            FAct::RestoreDefaults => {
                self.fx(Fx::Accept);
                self.dialog = Some(Dialog::Confirm { text: "Restore every option to its default?".into(), yes: false, action: DAction::ResetOptions, opened: now });
            }
        }
    }

    fn draw_footer(&mut self) {
        let buttons = self.footer_buttons();
        let ui = &self.ui;
        // The game's bottom panel: a see-through darkening with rounded top
        // corners (its bottom corners run off-screen), a name section on the
        // left split off by a light divider, buttons in a grid on the right.
        let (bh, gx, gy) = (28.0, 10.0, 7.0);
        let pad = 11.0;
        let ph = bh * 2.0 + gy + pad * 2.0;
        let (px, py) = (24.0, ui.h - FOOTER_H);
        let pw = ui.w - 48.0;
        ui.rrect_grad(px, py, pw, ph + 30.0, 12.0, Color::new(0.0, 0.0, 0.0, 0.42), Color::new(0.0, 0.0, 0.0, 0.58));
        ui.rrect_line(px, py, pw, ph + 30.0, 12.0, 1.0, Color::new(1.0, 1.0, 1.0, 0.14));
        let name_w = (pw * 0.25).clamp(220.0, 320.0);
        let div_x = px + name_w;
        // long buttons like the game's (~a quarter of the screen); screens
        // with many buttons shrink them just enough to fit beside the name
        let cols = buttons.len().div_ceil(2).max(1) as f32;
        let avail = pw - name_w - pad * 2.0 - gx * (cols - 1.0);
        let bw = (avail / cols).min(292.0);
        ui.rect(div_x - 1.5, py + 4.0, 3.0, ph + 30.0, Color::new(0.92, 0.94, 0.94, 0.55));
        let mid = py + ph * 0.5;
        let running = self.job.as_ref().map(|j| j.running()).unwrap_or(false);
        let job_chip = running && self.screen() != Screen::Log;
        ui.text_sh("Tiramisu", px + name_w * 0.5, mid + if job_chip { 0.0 } else { 8.0 }, 24.0, TXT, Align::Center);
        if job_chip {
            let cw = ui.measure_with(&ui.black, "JOB RUNNING", 14.0) + 16.0;
            ui.chip(px + name_w * 0.5 - cw * 0.5, mid + 20.0, "JOB RUNNING", LIME);
        }
        // grid: column by column from the right, top cell then bottom cell
        let y_top = py + pad;
        let m = ui.mouse();
        let mut hits = Vec::new();
        for (i, b) in buttons.iter().enumerate() {
            let col = (i / 2) as f32;
            let row = (i % 2) as f32;
            let x = px + pw - pad - bw - col * (bw + gx);
            if x < div_x + pad {
                break; // never draw over the name section
            }
            let y = y_top + row * (bh + gy);
            let r = Rect::new(x, y, bw, bh);
            let accent = b.act == FAct::TunerPill(4) && self.tuner.dirty();
            let hint = if self.opts.show_hints { b.hint } else { "" };
            ui.button(b.key, x, y, bw, bh, &b.label, hint, hit(&r, m) || accent);
            hits.push((r, b.key, b.act));
        }
        self.hits.footer = hits;
    }

    /// Lime text for a selected button or row, like the game's: it blinks
    /// smoothly between full lime and almost gone (no outline on the button).
    fn lime_pulse(&self, since: f64) -> Color {
        let p = 0.5 + 0.5 * ((self.now() - since) as f32 * std::f32::consts::TAU / 1.1).cos();
        Color::new(LIME.r, LIME.g, LIME.b, 0.06 + 0.94 * p)
    }

    /// The game's "Jump to Garage?" box: a dark see-through frame with a pale
    /// lime outline, a grey inner panel holding the centred question, and wide
    /// buttons along the panel's bottom. The selected button is dark with
    /// pulsing lime text; the other is a shade darker than the panel. Behind
    /// it the screen is darkened but stays visible.
    fn draw_dialog(&mut self) {
        let now = self.now();
        let Some(dlg) = self.dialog.as_ref() else { return };
        let ui = &self.ui;
        let opened = match dlg {
            Dialog::Confirm { opened, .. } | Dialog::Input { opened, .. } | Dialog::Message { opened, .. } => *opened,
        };
        let a = smooth(opened, opened + 0.12, now);
        let mut ui2 = ui.faded(a);
        ui2.dx = 0.0;
        let ui = &ui2;
        ui.rect(0.0, 0.0, ui.w, ui.h, Color::new(0.0, 0.0, 0.0, 0.50));

        const TS: f32 = 27.0; // question text size
        const BH: f32 = 56.0; // button height
        let msg_lines: Vec<String> = match dlg {
            Dialog::Message { text, .. } => wrap(ui, text, 22.0, 720.0),
            _ => Vec::new(),
        };
        // an OK-only box hugs its text; questions get the game's wide box
        let (w, body) = match dlg {
            Dialog::Message { .. } => {
                let tw = msg_lines.iter().map(|l| ui.measure(l, 22.0)).fold(0.0, f32::max);
                ((tw + 110.0).clamp(460.0, 820.0), 56.0 + msg_lines.len() as f32 * 30.0)
            }
            Dialog::Confirm { text, .. } => ((ui.measure(text, TS) + 140.0).clamp(640.0, 900.0), 124.0),
            Dialog::Input { .. } => (760.0, 160.0),
        };
        let top = 60.0; // the dark band above the inner panel
        let (m, gap) = (12.0, 10.0);
        let h = top + body + BH + gap + m + 4.0;
        let x = ui.w * 0.5 - w * 0.5;
        let y = ui.h * 0.5 - h * 0.5 - 30.0 + (1.0 - ease_out(a)) * 12.0;
        ui.rrect(x, y, w, h, 12.0, Color::new(0.0, 0.0, 0.0, 0.55));
        ui.rrect_line(x, y, w, h, 12.0, 2.5, FRAME);
        let (ix, iy, iw) = (x + m, y + top, w - 2.0 * m);
        let ih = y + h - m - iy;
        ui.rrect(ix, iy, iw, ih, 10.0, PANEL);
        let by = iy + ih - gap - BH + 4.0;
        let mid = iy + (by - iy) * 0.5; // vertical centre of the text area

        let (yes_label, no_label, yes_sel) = match dlg {
            Dialog::Confirm { text, yes, .. } => {
                let t = ui.fit(text, TS, iw - 60.0);
                ui.text_sh(&t, x + w * 0.5, mid + 10.0, TS, TXT, Align::Center);
                ("OK", Some("Cancel"), *yes)
            }
            Dialog::Input { prompt, placeholder, text, .. } => {
                ui.text_sh(prompt, x + w * 0.5, iy + 46.0, 26.0, TXT, Align::Center);
                let (fx, fy, fw) = (ix + 30.0, iy + 66.0, iw - 60.0);
                ui.rrect(fx, fy, fw, 46.0, 8.0, Color::new(0.03, 0.035, 0.04, 0.9));
                ui.rrect_line(fx, fy, fw, 46.0, 8.0, 1.5, LIME_DIM);
                if text.is_empty() {
                    ui.text_with(&ui.mono, placeholder, fx + 14.0, fy + 31.0, 20.0, Color::new(0.5, 0.53, 0.55, 1.0), Align::Left);
                } else {
                    ui.text_with(&ui.mono, text, fx + 14.0, fy + 31.0, 20.0, TXT, Align::Left);
                }
                if (now * 2.0).fract() < 0.55 {
                    let cx = fx + 14.0 + ui.measure_with(&ui.mono, text, 20.0) + 1.0;
                    ui.rect(cx, fy + 11.0, 2.0, 25.0, LIME);
                }
                ("OK", Some("Cancel"), true)
            }
            Dialog::Message { .. } => {
                // short notes are centred like the game's; long help text reads better left-aligned
                let n = msg_lines.len() as f32;
                let y0 = mid - (n - 1.0) * 15.0 + 8.0;
                for (i, l) in msg_lines.iter().enumerate() {
                    if msg_lines.len() <= 2 {
                        ui.text_sh(l, x + w * 0.5, y0 + i as f32 * 30.0, 22.0, TXT, Align::Center);
                    } else {
                        ui.text_sh(l, ix + 30.0, y0 + i as f32 * 30.0, 22.0, TXT, Align::Left);
                    }
                }
                ("OK", None, true)
            }
        };
        let lime = self.lime_pulse(opened);
        let mut btns: Vec<(&str, Rect, &str, bool)> = Vec::new();
        match no_label {
            Some(no) => {
                let bw = (iw - 3.0 * gap) * 0.5;
                btns.push(("dlg_yes", Rect::new(ix + gap, by, bw, BH), yes_label, yes_sel));
                btns.push(("dlg_no", Rect::new(ix + iw - gap - bw, by, bw, BH), no, !yes_sel));
            }
            None => {
                let bw = (iw * 0.5).max(220.0);
                btns.push(("dlg_yes", Rect::new(ix + iw * 0.5 - bw * 0.5, by, bw, BH), yes_label, true));
            }
        }
        self.hits.dlg_no = None;
        for (key, r, label, sel) in btns {
            // the highlight glides in and out over 250 ms
            let g = ui.glow(key, sel || self.hovered(&r));
            let fill = Color::new(BTN.r + (BTN_SEL.r - BTN.r) * g, BTN.g + (BTN_SEL.g - BTN.g) * g, BTN.b + (BTN_SEL.b - BTN.b) * g, BTN.a);
            ui.rrect(r.x, r.y, r.w, r.h, 10.0, fill);
            let base = Color::new(0.86, 0.87, 0.88, 1.0);
            let col = Color::new(base.r + (lime.r - base.r) * g, base.g + (lime.g - base.g) * g, base.b + (lime.b - base.b) * g, 1.0 + (lime.a - 1.0) * g);
            ui.text_sh(label, r.x + r.w * 0.5, r.y + r.h * 0.5 + 9.0, 26.0, col, Align::Center);
            if key == "dlg_yes" {
                self.hits.dlg_yes = Some(r);
            } else {
                self.hits.dlg_no = Some(r);
            }
        }
    }

    fn draw_toast(&mut self) {
        let now = self.now();
        let Some((text, t0, col)) = self.toast.clone() else { return };
        let age = now - t0;
        if age > 3.2 {
            self.toast = None;
            return;
        }
        let a = smooth(0.0, 0.15, age) * (1.0 - smooth(2.6, 3.2, age));
        let ui = self.ui.faded(a);
        let w = ui.measure(&text, 22.0) + 60.0;
        let x = ui.w * 0.5 - w * 0.5;
        let y = ui.h - FOOTER_H - 52.0 + (1.0 - ease_out(a)) * 10.0;
        ui.rrect(x, y, w, 40.0, 20.0, Color::new(0.03, 0.035, 0.04, 0.85));
        ui.rrect_line(x, y, w, 40.0, 20.0, 1.5, col);
        ui.text_sh(&text, x + w * 0.5, y + 28.0, 22.0, col, Align::Center);
    }
}

/// A material that writes alpha = 1 and leaves colour alone. Drawn over the
/// whole window at the end of each frame: translucent panels otherwise leave
/// the framebuffer's alpha below 1, and a compositor that honours an RGBA
/// window would then let the desktop show through (screenshots looked washed
/// out for the same reason).
fn alpha_seal() -> Option<Material> {
    const VS: &str = "#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
uniform mat4 Model;
uniform mat4 Projection;
void main() { gl_Position = Projection * Model * vec4(position, 1.0); }";
    const FS: &str = "#version 100
precision lowp float;
void main() { gl_FragColor = vec4(1.0, 1.0, 1.0, 1.0); }";
    let pipeline_params = macroquad::miniquad::PipelineParams { color_write: (false, false, false, true), ..Default::default() };
    load_material(macroquad::miniquad::ShaderSource::Glsl { vertex: VS, fragment: FS }, MaterialParams { pipeline_params, ..Default::default() }).ok()
}

/// Greedy word wrap to `max_w` virtual units.
fn wrap(ui: &Ui, text: &str, size: f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let try_line = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if ui.measure(&try_line, size) > max_w && !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                cur = try_line;
            }
        }
        lines.push(cur);
    }
    lines
}

/// Key press with auto-repeat after 0.36 s, every 75 ms.
fn key_repeat(rep: &mut Vec<(KeyCode, f64, f64)>, k: KeyCode, now: f64) -> bool {
    if is_key_pressed(k) {
        rep.retain(|(kk, _, _)| *kk != k);
        rep.push((k, now, now));
        return true;
    }
    if is_key_down(k) {
        if let Some(e) = rep.iter_mut().find(|(kk, _, _)| *kk == k) {
            if now - e.1 > 0.36 && now - e.2 > 0.075 {
                e.2 = now;
                return true;
            }
        }
    } else {
        rep.retain(|(kk, _, _)| *kk != k);
    }
    false
}

/// Fonts are cheap handles (Arc), so a Ui can be copied for a differently faded pass.
fn clone_ui(ui: &Ui) -> Ui {
    ui.faded(ui.alpha)
}

#[macroquad::main(window_conf)]
async fn main() {
    let font = load_ttf_font_from_bytes(FONT).expect("UI font");
    let black = load_ttf_font_from_bytes(FONT_BLACK).expect("logo font");
    let mono = load_ttf_font_from_bytes(FONT_MONO).expect("mono font");
    let tools = catalog::catalog();
    let installed = tools.iter().map(|t| runner::which(t.needs)).collect();
    let n_tools = tools.len();
    let opts = Options::load();
    if opts.fullscreen {
        set_fullscreen(true);
    }
    prevent_quit();
    let seal = alpha_seal();
    let mut app = App {
        ui: Ui::new(font, black, mono),
        tools,
        installed,
        stack: vec![Screen::Main],
        main_strip: StripState::new(),
        tool_strips: (0..n_tools).map(|_| StripState::new()).collect(),
        pending_nav: None,
        enter_at: get_time() + 0.35,
        dir: 1.0,
        dialog: None,
        job: None,
        log_scroll: 0.0,
        log_follow: true,
        log_input: String::new(),
        log_seen_version: 0,
        dyno: dyno::Dyno::new(),
        tuner: tuner::Tuner::new(),
        opts,
        opt_sel: 0,
        sfx: Sfx::new().await,
        scene: scene::Scene::new(),
        grab_last: None,
        dim: Spring::new(0.0),
        toast: None,
        picker: None,
        hits: Hits::default(),
        chars: Vec::new(),
        rep: Vec::new(),
        wheel_acc: 0.0,
        quit: false,
        shot: None,
        demo: std::env::var("GARAGE_DEMO").ok().map(Demo::new),
        warm_k: -1.0,
        opt_dot: Spring::new(0.0),
        opt_top: Spring::new(0.0),
    };
    if let Some(d) = app.demo.as_ref() {
        let _ = std::fs::create_dir_all(&d.dir);
        app.opts.sound = false;
        // GitHub screenshots: clean, like the game (all four switches off)
        app.opts.show_count = false;
        app.opts.show_desc = false;
        app.opts.show_tags = false;
        app.opts.show_hints = false;
    }

    loop {
        let dt = get_frame_time().min(0.05);
        app.ui.begin_frame();
        app.prewarm();
        let now = app.now();
        app.chars.clear();
        while let Some(c) = get_char_pressed() {
            app.chars.push(c);
        }

        // background work
        if let Some(job) = app.job.as_mut() {
            if job.poll(now) {
                let ok = matches!(job.state, JobState::Exited(0)) || job.stopped_by_user();
                app.fx(if ok { Fx::Done } else { Fx::Error });
                let heading_to_log = matches!(app.pending_nav, Some((_, Nav::Push(Screen::Log))));
                if app.screen() != Screen::Log && !heading_to_log {
                    let t = app.job.as_ref().unwrap().spec.title.clone();
                    app.job_done_dialog(&t, ok);
                }
            }
        }
        let tev = app.tuner.poll();
        app.tuner_events(tev);
        if let Some((rx, p)) = app.picker.take() {
            match rx.try_recv() {
                Ok(Some(files)) => app.proceed(Pending { files: Some(files), ..p }),
                Ok(None) => app.toast("No file picked", TXT_DIM),
                Err(std::sync::mpsc::TryRecvError::Empty) => app.picker = Some((rx, p)),
                Err(_) => {}
            }
        }
        if let Some((t0, nav)) = app.pending_nav {
            if now - t0 >= app.times().0 {
                app.pending_nav = None;
                app.apply_nav(nav);
            }
        }
        if is_quit_requested() {
            app.ask_quit();
        }

        app.demo_tick();
        app.frame_input();
        app.update_strips(dt);
        app.draw(dt);
        if let Some(m) = seal.as_ref() {
            gl_use_material(m);
            draw_rectangle(0.0, 0.0, screen_width(), screen_height(), WHITE);
            gl_use_default_material();
        }
        if let Some(path) = app.shot.take() {
            get_screen_data().export_png(&path);
        }

        if app.quit {
            app.job = None;
            app.save_opts();
            break;
        }
        next_frame().await;
    }
}
