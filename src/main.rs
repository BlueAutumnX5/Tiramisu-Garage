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
const STRIP_Y: f32 = 94.0;
const STRIP_H: f32 = 150.0;

fn window_conf() -> Conf {
    Conf {
        window_title: "Tiramisu Garage".to_owned(),
        window_width: 1280,
        window_height: 720,
        window_resizable: true,
        sample_count: 4,
        high_dpi: false,
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

/// Where a strip's first item sits and how far it should be scrolled.
/// Which item index sits at the centre of the strip. Like the game's
/// three-icon carousel: the selection box stays put in the middle and the
/// icons scroll under it; only on the first and last item does the scroll
/// stop and the box walk to the side.
fn strip_scroll_target(n: usize, sel: usize) -> f32 {
    if n <= 3 { (n as f32 - 1.0) * 0.5 } else { (sel as f32).clamp(1.0, n as f32 - 2.0) }
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
}

enum Dialog {
    Confirm { text: String, yes: bool, action: DAction, opened: f64 },
    Input { prompt: String, placeholder: String, text: String, allow_empty: bool, pending: Pending, opened: f64 },
    Message { text: String, opened: f64 },
}

#[derive(Default)]
struct Hits {
    items: Vec<(Rect, usize)>,
    prev: Option<Rect>,
    next: Option<Rect>,
    back: Option<Rect>,
    quit: Option<Rect>,
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
    Quit,
}

struct Demo {
    dir: String,
    t0: f64,
    i: usize,
    steps: Vec<(f64, DStep)>,
}

impl Demo {
    fn new(dir: String) -> Demo {
        use DStep::*;
        let steps = vec![
            (1.0, Shot("01-main")),
            (1.2, Right),
            (1.27, Shot("02-box-walks-to-centre")),
            (1.7, Right),
            (1.76, Shot("03-icons-slide-box-stays")),
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
            (30.9, Quit),
        ];
        Demo { dir, t0: -1.0, i: 0, steps }
    }
}

const OPT_ROWS: [&str; 8] = ["UI Sounds", "Sound Volume", "Turntable", "Animations", "Fullscreen", "Default Phone", "Log Text Size", "Restore Defaults"];

impl App {
    fn now(&self) -> f64 {
        self.ui.t
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
        let until: f64 = std::env::var("GARAGE_DEMO_UNTIL").ok().and_then(|v| v.parse().ok()).unwrap_or(f64::MAX);
        if el > until {
            self.quit = true;
        }
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
                DStep::Quit => self.quit = true,
            }
        }
    }

    fn screen(&self) -> Screen {
        *self.stack.last().unwrap()
    }

    fn fx(&self, fx: Fx) {
        self.sfx.play(fx, self.opts.sound, self.opts.volume);
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
            st.scroll.update(dt, 17.0);
            st.boxpos.update(dt, 17.0);
            st.dot.update(dt, 14.0);
        }
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
        if self.clicked(&self.hits.quit.clone()) {
            self.ask_quit();
            return;
        }
        if self.clicked(&self.hits.back.clone()) {
            self.back();
            return;
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
        if self.rep_pressed(KeyCode::Up) {
            self.opt_sel = (self.opt_sel + n - 1) % n;
            self.fx(Fx::Tick);
        }
        if self.rep_pressed(KeyCode::Down) {
            self.opt_sel = (self.opt_sel + 1) % n;
            self.fx(Fx::Tick);
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
                } else if i == 7 {
                    d = 1;
                }
            }
        }
        if (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter)) && self.opt_sel == 7 {
            d = 1;
        }
        if d != 0 {
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
        let (w, h) = (ui.w - 48.0, ui.h - 96.0 - 78.0);
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
        ui.glow(dx, ty + 4.5, 12.0, Color::new(1.0, 1.0, 1.0, 0.45));
        ui.disc(dx, ty + 4.5, 4.2, TXT);

        // the strip
        ui.rrect_grad(24.0, STRIP_Y, w - 48.0, STRIP_H, 14.0, Color::new(0.62, 0.65, 0.65, 0.42), Color::new(0.45, 0.48, 0.48, 0.40));
        ui.rrect_line(24.0, STRIP_Y, w - 48.0, STRIP_H, 14.0, 1.0, Color::new(1.0, 1.0, 1.0, 0.12));
        let cy = STRIP_Y + STRIP_H * 0.5;
        let lp = Rect::new(24.0 + 40.0 - 22.0, cy - 22.0, 44.0, 44.0);
        let rp = Rect::new(w - 24.0 - 40.0 - 22.0, cy - 22.0, 44.0, 44.0);
        ui.circle_arrow(lp.x + 22.0, cy, 21.0, -1, self.hovered(&lp));
        ui.circle_arrow(rp.x + 22.0, cy, 21.0, 1, self.hovered(&rp));

        let cxm = w * 0.5;
        let (x0, x1) = (24.0 + 60.0, w - 24.0 - 60.0);
        let since = (now - st.changed_at) as f32;
        let _ = n;

        // selection box: slides, its lime rim pulses, and it flashes on change
        let bx = cxm + st.boxpos.value * SLOT;
        let (bw, bh) = (178.0, 128.0);
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
            let size = if selected { 92.0 } else { 76.0 } * pop * (1.0 - 0.14 * smooth(1.0, 2.0, d.abs() as f64));
            let base = if it.dim { 0.32 } else if selected { 1.0 } else { 0.80 };
            let mut col = Color::new(0.95, 0.96, 0.96, base * fade);
            if selected && !it.dim {
                let k = 0.30 * rim;
                col = Color::new(col.r + (LIME.r - col.r) * k, col.g + (LIME.g - col.g) * k, col.b + (LIME.b - col.b) * k, col.a);
            }
            ui.icon(it.glyph, x, cy + 2.0, size, col);
            hits.push((Rect::new(x - SLOT * 0.5, STRIP_Y, SLOT, STRIP_H), i));
        }

        // label under the strip, in lime, under the selected item
        if let Some(it) = items.get(st.sel) {
            let lx = bx.clamp(160.0, w - 160.0);
            ui.text_sh(&it.label, lx, STRIP_Y + STRIP_H + 40.0, 32.0, Color::new(LIME.r, LIME.g, LIME.b, hl.max(0.25)), Align::Center);
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
        ui.text_sh(tool.blurb, ui.w * 0.5, y, 22.0, TXT, Align::Center);
        let info = if !self.installed[i] {
            format!("{} is not installed", tool.needs)
        } else if tool.cmds.len() == 1 {
            String::new()
        } else {
            format!("{} commands inside", tool.cmds.len())
        };
        if !info.is_empty() {
            ui.text_sh(&info, ui.w * 0.5, y + 28.0, 17.0, if self.installed[i] { TXT_DIM } else { RED }, Align::Center);
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
            ui.circle_arrow(lx, 42.0, 15.0, -1, self.hovered(&lp));
            ui.circle_arrow(rx, 42.0, 15.0, 1, self.hovered(&rp));
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
        let desc = ui.fit(cmd.desc, 22.0, ui.w - 120.0);
        ui.text_sh(&desc, ui.w * 0.5, y, 22.0, TXT, Align::Center);
        let tags = catalog::tags(cmd);
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
            ui.glow(dot_x, 48.0, 16.0, Color::new(LIME.r, LIME.g, LIME.b, 0.5 * p));
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
        ui.circle_arrow(sbx, iy + 15.0, 14.0, -2, self.hovered(&up));
        ui.circle_arrow(sbx, iy + ih - 15.0, 14.0, 2, self.hovered(&dn));
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
        let (w, h) = (ui.w - 120.0, ui.h - 100.0 - 92.0);
        ui.rrect_grad(x, y, w, h, 16.0, Color::new(0.78, 0.80, 0.80, 0.40), Color::new(0.62, 0.64, 0.64, 0.38));
        let o = &self.opts;
        let row_h = 52.0;
        let vx = x + w * 0.58; // value column
        let mut rows = Vec::new();
        let (mut rp, mut rn) = (None, None);
        for (i, name) in OPT_ROWS.iter().enumerate() {
            let ry = y + 22.0 + i as f32 * row_h;
            let sel = i == self.opt_sel;
            let rr = Rect::new(x + 18.0, ry, w - 36.0 - 40.0, row_h - 6.0);
            if sel {
                ui.rrect(rr.x, rr.y, rr.w, rr.h, 9.0, Color::new(0.14, 0.15, 0.15, 0.88));
            }
            ui.text_sh(name, rr.x + 24.0, ry + 33.0, 27.0, if sel { LIME_DIM } else { TXT }, Align::Left);
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
                let lp = Rect::new(vx + 10.0 - 15.0, ry + 8.0, 30.0, 30.0);
                let np = Rect::new(rr.x + rr.w - 34.0 - 15.0, ry + 8.0, 30.0, 30.0);
                ui.circle_arrow(vx + 10.0, ry + 23.0, 15.0, -1, self.hovered(&lp));
                ui.circle_arrow(rr.x + rr.w - 34.0, ry + 23.0, 15.0, 1, self.hovered(&np));
                rp = Some(lp);
                rn = Some(np);
            }
            rows.push((rr, i));
        }
        // decorative scrollbar, like the original list screens
        let sbx = x + w - 28.0;
        ui.circle_arrow(sbx, y + 30.0, 14.0, -2, false);
        ui.circle_arrow(sbx, y + h - 30.0, 14.0, 2, false);
        ui.rrect(sbx - 3.0, y + 52.0, 6.0, h - 104.0, 3.0, Color::new(0.0, 0.0, 0.0, 0.35));
        let f = self.opt_sel as f32 / (OPT_ROWS.len() - 1) as f32;
        ui.disc(sbx, y + 58.0 + (h - 116.0) * f, 6.0, TXT);
        ui.text_sh(
            "Saved to ~/.config/tiramisu-garage/options.conf as you change them.",
            x + 24.0,
            y + h - 20.0,
            17.0,
            TXT_DIM,
            Align::Left,
        );
        self.hits.rows = rows;
        self.hits.row_prev = rp;
        self.hits.row_next = rn;
    }

    fn draw_footer(&mut self) {
        let ui = &self.ui;
        let fy = ui.h - 40.0;
        ui.rect(0.0, ui.h - 70.0, ui.w, 70.0, Color::new(0.0, 0.0, 0.0, 0.25));
        ui.text_sh("TIRAMISU", 30.0, fy + 8.0, 20.0, TXT, Align::Left);
        let mut x = 140.0;
        let screen = self.screen();
        let running = self.job.as_ref().map(|j| j.running()).unwrap_or(false);
        let prompts: Vec<(Vec<(&str, Option<char>)>, &str)> = if self.dialog.is_some() {
            vec![(vec![("", Some(ico::ARROW_LR))], "Choose"), (vec![("", Some(ico::RETURN))], "Confirm"), (vec![("ESC", None)], "Cancel")]
        } else {
            match screen {
                Screen::Main => vec![(vec![("", Some(ico::ARROW_LR))], "Browse"), (vec![("", Some(ico::RETURN))], "Select"), (vec![("ESC", None)], "Quit")],
                Screen::Tool(ti) => {
                    let mut v = vec![(vec![("", Some(ico::ARROW_LR))], "Browse"), (vec![("", Some(ico::RETURN))], "Run"), (vec![("ESC", None)], "Back")];
                    if self.tools[ti].phone_picker {
                        v.push((vec![("", Some(ico::ARROW_UD))], "Phone"));
                    }
                    v
                }
                Screen::Log if running => vec![(vec![("", Some(ico::RETURN))], "Send answer"), (vec![("PGUP", None), ("PGDN", None)], "Scroll"), (vec![("ESC", None)], "Stop")],
                Screen::Log => vec![(vec![("R", None)], "Run again"), (vec![("C", None)], "Copy log"), (vec![("PGUP", None), ("PGDN", None)], "Scroll"), (vec![("ESC", None)], "Back")],
                Screen::Dyno => vec![(vec![("", Some(ico::RETURN))], "Fan Curve Tuner"), (vec![("ESC", None)], "Back")],
                Screen::Tuner => vec![
                    (vec![("", Some(ico::ARROW_LR))], "Column"),
                    (vec![("", Some(ico::ARROW_UD))], "Speed"),
                    (vec![(",", None), (".", None)], "Threshold"),
                    (vec![("TAB", None)], "CPU/GPU"),
                    (vec![("H", None)], "Help"),
                ],
                Screen::Options => vec![(vec![("", Some(ico::ARROW_UD))], "Choose"), (vec![("", Some(ico::ARROW_LR))], "Change"), (vec![("ESC", None)], "Back")],
            }
        };
        for (caps, cap) in &prompts {
            x += ui.prompt(x, fy, caps, cap) + 26.0;
        }
        if self.job.is_some() && !matches!(screen, Screen::Log) && self.dialog.is_none() {
            let (label, col) = if running { ("JOB RUNNING", LIME) } else { ("LAST LOG", TXT_DIM) };
            x += ui.prompt(x, fy, &[("L", None)], "") - 6.0;
            ui.chip(x, fy, label, col);
        }
        // pills on the right
        let pw = 150.0;
        let quit = ui.pill(ui.w - 30.0 - pw, fy - 15.0, pw, "Quit Garage", self.hovered(&Rect::new(ui.w - 30.0 - pw, fy - 15.0, pw, 30.0)));
        self.hits.quit = Some(quit);
        if self.stack.len() > 1 {
            let br = Rect::new(ui.w - 30.0 - pw * 2.0 - 12.0, fy - 15.0, pw, 30.0);
            let back = ui.pill(br.x, br.y, pw, "Back", self.hovered(&br));
            self.hits.back = Some(back);
        }
    }

    fn draw_dialog(&mut self) {
        let now = self.now();
        let Some(dlg) = self.dialog.as_ref() else { return };
        let ui = &self.ui;
        let opened = match dlg {
            Dialog::Confirm { opened, .. } | Dialog::Input { opened, .. } | Dialog::Message { opened, .. } => *opened,
        };
        let a = smooth(opened, opened + 0.12, now);
        let mut ui2 = Ui { alpha: a, ..clone_ui(ui) };
        ui2.dx = 0.0;
        let ui = &ui2;
        ui.rect(0.0, 0.0, ui.w, ui.h, Color::new(0.0, 0.0, 0.0, 0.35));
        // a Message wraps its text; the box grows to fit
        let msg_lines: Vec<String> = match dlg {
            Dialog::Message { text, .. } => wrap(ui, text, 22.0, 760.0),
            _ => Vec::new(),
        };
        let (w, h) = match dlg {
            Dialog::Input { .. } => (720.0, 236.0),
            Dialog::Message { .. } => (820.0, 130.0 + msg_lines.len() as f32 * 30.0),
            _ => (720.0, 196.0),
        };
        let x = ui.w * 0.5 - w * 0.5;
        let y = ui.h * 0.5 - h * 0.5 - 30.0 + (1.0 - ease_out(a)) * 12.0;
        ui.rrect_grad(x, y, w, h, 14.0, Color::new(0.70, 0.72, 0.72, 0.78), Color::new(0.52, 0.54, 0.54, 0.78));
        ui.rrect_line(x, y, w, h, 14.0, 2.0, LIME);
        let (yes_label, no_label, yes_sel) = match dlg {
            Dialog::Confirm { text, yes, .. } => {
                let t = ui.fit(text, 27.0, w - 60.0);
                ui.text_sh(&t, x + w * 0.5, y + 70.0, 27.0, TXT, Align::Center);
                ("Yes", "No", *yes)
            }
            Dialog::Input { prompt, placeholder, text, .. } => {
                ui.text_sh(prompt, x + w * 0.5, y + 52.0, 26.0, TXT, Align::Center);
                let (fx, fy, fw) = (x + 40.0, y + 72.0, w - 80.0);
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
                ("OK", "Cancel", true)
            }
            Dialog::Message { .. } => {
                for (i, l) in msg_lines.iter().enumerate() {
                    ui.text_sh(l, x + 30.0, y + 46.0 + i as f32 * 30.0, 22.0, TXT, Align::Left);
                }
                // one centred OK button
                let r = Rect::new(x + w * 0.5 - 110.0, y + h - 64.0, 220.0, 40.0);
                ui.rrect(r.x, r.y, r.w, r.h, 8.0, Color::new(0.10, 0.11, 0.11, 0.92));
                ui.rrect_line(r.x, r.y, r.w, r.h, 8.0, 1.5, Color::new(LIME.r, LIME.g, LIME.b, 0.8));
                ui.text_sh("OK", r.x + r.w * 0.5, r.y + 28.0, 24.0, LIME, Align::Center);
                self.hits.dlg_yes = Some(r);
                self.hits.dlg_no = None;
                return;
            }
        };
        let bw = (w - 120.0) * 0.5;
        let by = y + h - 64.0;
        let yr = Rect::new(x + 40.0, by, bw, 40.0);
        let nr = Rect::new(x + w - 40.0 - bw, by, bw, 40.0);
        for (r, label, sel) in [(yr, yes_label, yes_sel), (nr, no_label, !yes_sel)] {
            let hot = self.hovered(&r);
            ui.rrect(r.x, r.y, r.w, r.h, 8.0, if sel || hot { Color::new(0.10, 0.11, 0.11, 0.92) } else { Color::new(0.30, 0.32, 0.32, 0.75) });
            if sel {
                ui.rrect_line(r.x, r.y, r.w, r.h, 8.0, 1.5, Color::new(LIME.r, LIME.g, LIME.b, 0.8));
            }
            ui.text_sh(label, r.x + r.w * 0.5, r.y + 28.0, 24.0, if sel { LIME } else { TXT }, Align::Center);
        }
        self.hits.dlg_yes = Some(yr);
        self.hits.dlg_no = Some(nr);
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
        let ui = Ui { alpha: a, ..clone_ui(&self.ui) };
        let w = ui.measure(&text, 22.0) + 60.0;
        let x = ui.w * 0.5 - w * 0.5;
        let y = ui.h - 124.0 + (1.0 - ease_out(a)) * 10.0;
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
    Ui { font: ui.font.clone(), black: ui.black.clone(), mono: ui.mono.clone(), k: ui.k, w: ui.w, h: ui.h, t: ui.t, alpha: ui.alpha, dx: ui.dx }
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
    };
    if let Some(d) = app.demo.as_ref() {
        let _ = std::fs::create_dir_all(&d.dir);
        app.opts.sound = false;
    }

    loop {
        let dt = get_frame_time().min(0.05);
        app.ui.begin_frame();
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
                    app.toast(if ok { format!("{t}: finished (L to view)") } else { format!("{t}: failed (L to view)") }, if ok { LIME } else { RED });
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
