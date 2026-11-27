//! Drawing kit: a virtual 720-unit-tall canvas that scales with the window,
//! plus the shapes the 2004-style front end is built from (rounded panels,
//! circle-arrow buttons, keycaps, shadowed italic text, icon glyphs).
//!
//! Everything takes *virtual* coordinates. The virtual height is 720 (or more
//! on very narrow windows); the width follows the window's aspect ratio, so a
//! wide window gets a wider strip rather than black bars.

use macroquad::models::{Mesh, Vertex, draw_mesh};
use macroquad::prelude::*;

pub const LIME: Color = Color::new(0.776, 0.890, 0.318, 1.0); // #c6e351
pub const LIME_DIM: Color = Color::new(0.69, 0.73, 0.49, 1.0);
pub const TXT: Color = Color::new(0.95, 0.96, 0.96, 1.0);
pub const TXT_DIM: Color = Color::new(0.70, 0.73, 0.74, 1.0);
pub const RED: Color = Color::new(1.0, 0.36, 0.24, 1.0);
pub const AMBER: Color = Color::new(0.96, 0.72, 0.24, 1.0);
pub const TEAL: Color = Color::new(0.33, 0.70, 0.69, 1.0);
pub const SHADOW: Color = Color::new(0.0, 0.0, 0.0, 0.62);

/// Material Design glyphs that ship inside the Nerd-Font-patched UI font.
pub mod ico {
    pub const ARROW_L: char = '\u{F004D}';
    pub const ARROW_R: char = '\u{F0054}';
    pub const ARROW_U: char = '\u{F005D}';
    pub const ARROW_D: char = '\u{F0045}';
    pub const ARROW_LR: char = '\u{F0E73}';
    pub const ARROW_UD: char = '\u{F0E79}';
    pub const RETURN: char = '\u{F0311}';
    pub const MOUSE: char = '\u{F037D}';
    pub const DOT: char = '\u{F09DE}';
    pub const CHECK: char = '\u{F012C}';
    pub const CLOSE: char = '\u{F0156}';
    pub const ALERT: char = '\u{F0028}';
    pub const CONSOLE: char = '\u{F07B7}';
    pub const LAUNCH: char = '\u{F0327}';
    pub const SHIELD: char = '\u{F0498}';
    pub const KEY: char = '\u{F0306}';
    pub const CELL: char = '\u{F011C}';
}

#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub struct Ui {
    pub font: Font,
    pub black: Font,
    pub mono: Font,
    /// screen pixels per virtual unit
    pub k: f32,
    /// virtual width / height
    pub w: f32,
    pub h: f32,
    /// seconds since start
    pub t: f64,
    /// global fade multiplier applied to every colour drawn
    pub alpha: f32,
    /// horizontal offset applied to x coordinates (used for slide transitions)
    pub dx: f32,
    /// per-widget highlight level (0 grey .. 1 lime) and when it was last stepped
    glow: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, (f32, f64)>>>,
    /// when each widget was last pressed (click or hotkey)
    presses: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, f64>>>,
}

/// How long a highlight takes to fade in or out (seconds).
pub const FADE: f32 = 0.25;

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
}

impl Ui {
    pub fn new(font: Font, black: Font, mono: Font) -> Self {
        Ui { font, black, mono, k: 1.0, w: 1280.0, h: 720.0, t: 0.0, alpha: 1.0, dx: 0.0, glow: Default::default(), presses: Default::default() }
    }

    /// A copy drawn at a different opacity that shares the widget
    /// animation state (fonts are cheap Arc handles).
    pub fn faded(&self, alpha: f32) -> Ui {
        Ui {
            font: self.font.clone(),
            black: self.black.clone(),
            mono: self.mono.clone(),
            k: self.k,
            w: self.w,
            h: self.h,
            t: self.t,
            alpha,
            dx: self.dx,
            glow: self.glow.clone(),
            presses: self.presses.clone(),
        }
    }

    pub fn begin_frame(&mut self) {
        let (sw, sh) = (screen_width(), screen_height());
        let mut k = sh / 720.0;
        if sw / k < 1024.0 {
            k = sw / 1024.0;
        }
        self.k = k.max(0.2);
        self.w = sw / self.k;
        self.h = sh / self.k;
        self.t = get_time();
        self.alpha = 1.0;
        self.dx = 0.0;
    }

    #[inline]
    pub fn px(&self, x: f32) -> f32 {
        (x + self.dx) * self.k
    }
    #[inline]
    pub fn py(&self, y: f32) -> f32 {
        y * self.k
    }
    #[inline]
    pub fn c(&self, col: Color) -> Color {
        Color::new(col.r, col.g, col.b, col.a * self.alpha)
    }

    /// Mouse position in virtual units (ignores the slide offset on purpose:
    /// hit boxes are recorded without it).
    pub fn mouse(&self) -> Vec2 {
        let (x, y) = mouse_position();
        vec2(x / self.k, y / self.k)
    }

    // ------------------------------------------------------------ shapes

    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32, col: Color) {
        draw_rectangle(self.px(x), self.py(y), w * self.k, h * self.k, self.c(col));
    }

    pub fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, th: f32, col: Color) {
        draw_line(self.px(x1), self.py(y1), self.px(x2), self.py(y2), th * self.k, self.c(col));
    }

    fn corner_segments(&self, r: f32) -> usize {
        ((r * self.k) / 2.5).clamp(3.0, 14.0) as usize
    }

    fn rr_points(&self, x: f32, y: f32, w: f32, h: f32, r: f32, seg: usize) -> Vec<Vec2> {
        let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
        let mut pts = Vec::with_capacity(seg * 4 + 4);
        let corners = [
            (x + w - r, y + r, -90.0f32),
            (x + w - r, y + h - r, 0.0),
            (x + r, y + h - r, 90.0),
            (x + r, y + r, 180.0),
        ];
        for (cx, cy, a0) in corners {
            for i in 0..=seg {
                let a = (a0 + 90.0 * i as f32 / seg as f32).to_radians();
                pts.push(vec2(self.px(cx + r * a.cos()), self.py(cy + r * a.sin())));
            }
        }
        pts
    }

    /// Filled rounded rectangle with a vertical gradient. One mesh, so a
    /// translucent fill never darkens where pieces would overlap.
    pub fn rrect_grad(&self, x: f32, y: f32, w: f32, h: f32, r: f32, top: Color, bot: Color) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let seg = self.corner_segments(r);
        let pts = self.rr_points(x, y, w, h, r, seg);
        let (y0, y1) = (self.py(y), self.py(y + h));
        let col = |py: f32| {
            let t = ((py - y0) / (y1 - y0)).clamp(0.0, 1.0);
            let c = Color::new(
                top.r + (bot.r - top.r) * t,
                top.g + (bot.g - top.g) * t,
                top.b + (bot.b - top.b) * t,
                top.a + (bot.a - top.a) * t,
            );
            self.c(c)
        };
        let cx = self.px(x + w * 0.5);
        let cy = self.py(y + h * 0.5);
        let mut vertices = Vec::with_capacity(pts.len() + 1);
        vertices.push(Vertex::new(cx, cy, 0.0, 0.0, 0.0, col(cy)));
        for p in &pts {
            vertices.push(Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, col(p.y)));
        }
        let n = pts.len() as u16;
        let mut indices = Vec::with_capacity(pts.len() * 3);
        for i in 0..n {
            indices.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % n]);
        }
        draw_mesh(&Mesh { vertices, indices, texture: None });
    }

    pub fn rrect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, col: Color) {
        self.rrect_grad(x, y, w, h, r, col, col);
    }

    /// Rounded-rectangle outline as a ring mesh (clean joints at any alpha).
    pub fn rrect_line(&self, x: f32, y: f32, w: f32, h: f32, r: f32, th: f32, col: Color) {
        let seg = self.corner_segments(r);
        let outer = self.rr_points(x, y, w, h, r, seg);
        let inner = self.rr_points(x + th, y + th, w - 2.0 * th, h - 2.0 * th, (r - th).max(0.01), seg);
        let c = self.c(col);
        let mut vertices = Vec::with_capacity(outer.len() * 2);
        for (o, i) in outer.iter().zip(inner.iter()) {
            vertices.push(Vertex::new(o.x, o.y, 0.0, 0.0, 0.0, c));
            vertices.push(Vertex::new(i.x, i.y, 0.0, 0.0, 0.0, c));
        }
        let n = outer.len() as u16;
        let mut indices = Vec::with_capacity(outer.len() * 6);
        for k in 0..n {
            let a = 2 * k;
            let b = 2 * ((k + 1) % n);
            indices.extend_from_slice(&[a, a + 1, b, b, a + 1, b + 1]);
        }
        draw_mesh(&Mesh { vertices, indices, texture: None });
    }

    /// Ring segment (annulus slice) from angle a0 to a1 in radians,
    /// 0 = +x, angles grow clockwise on screen (y down).
    pub fn arc(&self, cx: f32, cy: f32, r_in: f32, r_out: f32, a0: f32, a1: f32, col: Color) {
        self.arc_grad(cx, cy, r_in, r_out, a0, a1, col, col);
    }

    /// Ring segment whose colour runs from `c0` at a0 to `c1` at a1.
    pub fn arc_grad(&self, cx: f32, cy: f32, r_in: f32, r_out: f32, a0: f32, a1: f32, c0: Color, c1: Color) {
        let span = (a1 - a0).abs();
        let seg = ((span * r_out * self.k) / 6.0).clamp(4.0, 160.0) as usize;
        let mut vertices = Vec::with_capacity((seg + 1) * 2);
        for i in 0..=seg {
            let f = i as f32 / seg as f32;
            let a = a0 + (a1 - a0) * f;
            let (s, co) = a.sin_cos();
            let col = self.c(Color::new(
                c0.r + (c1.r - c0.r) * f,
                c0.g + (c1.g - c0.g) * f,
                c0.b + (c1.b - c0.b) * f,
                c0.a + (c1.a - c0.a) * f,
            ));
            vertices.push(Vertex::new(self.px(cx + co * r_out), self.py(cy + s * r_out), 0.0, 0.0, 0.0, col));
            vertices.push(Vertex::new(self.px(cx + co * r_in), self.py(cy + s * r_in), 0.0, 0.0, 0.0, col));
        }
        let mut indices = Vec::with_capacity(seg * 6);
        for i in 0..seg as u16 {
            let a = 2 * i;
            let b = a + 2;
            indices.extend_from_slice(&[a, a + 1, b, b, a + 1, b + 1]);
        }
        draw_mesh(&Mesh { vertices, indices, texture: None });
    }

    pub fn disc(&self, cx: f32, cy: f32, r: f32, col: Color) {
        self.arc(cx, cy, 0.0, r, 0.0, std::f32::consts::TAU, col);
    }

    /// Soft radial glow: opaque-ish centre fading to nothing at `r`.
    pub fn glow_soft(&self, cx: f32, cy: f32, r: f32, col: Color) {
        let seg = 40;
        let c = self.c(col);
        let edge = Color::new(col.r, col.g, col.b, 0.0);
        let mut vertices = vec![Vertex::new(self.px(cx), self.py(cy), 0.0, 0.0, 0.0, c)];
        for i in 0..seg {
            let a = i as f32 / seg as f32 * std::f32::consts::TAU;
            vertices.push(Vertex::new(self.px(cx + a.cos() * r), self.py(cy + a.sin() * r), 0.0, 0.0, 0.0, edge));
        }
        let mut indices = Vec::new();
        for i in 0..seg as u16 {
            indices.extend_from_slice(&[0, 1 + i, 1 + (i + 1) % seg as u16]);
        }
        draw_mesh(&Mesh { vertices, indices, texture: None });
    }

    // ------------------------------------------------------------ text

    fn fs(&self, size: f32) -> u16 {
        (size * self.k).round().clamp(4.0, 400.0) as u16
    }

    pub fn measure_with(&self, font: &Font, s: &str, size: f32) -> f32 {
        measure_text(s, Some(font), self.fs(size), 1.0).width / self.k
    }

    pub fn measure(&self, s: &str, size: f32) -> f32 {
        self.measure_with(&self.font, s, size)
    }

    /// Draw text; `y` is the baseline. Returns the width in virtual units.
    pub fn text_with(&self, font: &Font, s: &str, x: f32, y: f32, size: f32, col: Color, align: Align) -> f32 {
        let fs = self.fs(size);
        let wpx = measure_text(s, Some(font), fs, 1.0).width;
        let mut xp = self.px(x);
        match align {
            Align::Center => xp -= wpx * 0.5,
            Align::Right => xp -= wpx,
            Align::Left => {}
        }
        draw_text_ex(
            s,
            xp.round(),
            self.py(y).round(),
            TextParams { font: Some(font), font_size: fs, font_scale: 1.0, color: self.c(col), ..Default::default() },
        );
        wpx / self.k
    }

    pub fn text(&self, s: &str, x: f32, y: f32, size: f32, col: Color, align: Align) -> f32 {
        self.text_with(&self.font, s, x, y, size, col, align)
    }

    /// Text with the soft drop shadow every label in the old front end had.
    pub fn text_sh(&self, s: &str, x: f32, y: f32, size: f32, col: Color, align: Align) -> f32 {
        let o = (size * 0.06).max(1.5);
        self.text_with(&self.font, s, x + o, y + o, size, Color::new(0.0, 0.0, 0.0, 0.7 * col.a), align);
        self.text_with(&self.font, s, x, y, size, col, align)
    }

    /// Text cut to fit `max_w`, with an ellipsis.
    pub fn fit(&self, s: &str, size: f32, max_w: f32) -> String {
        if self.measure(s, size) <= max_w {
            return s.to_string();
        }
        let mut out: String = s.to_string();
        while !out.is_empty() && self.measure(&(out.clone() + "…"), size) > max_w {
            out.pop();
        }
        out + "…"
    }

    /// An icon glyph centred on (cx, cy).
    ///
    /// Icons animate in size (the selection pop, the shrink with distance),
    /// but every new pixel size makes the font engine rasterise the glyph
    /// again and grow its atlas texture: that caused 100+ ms hitches while
    /// holding an arrow key. So glyphs are rasterised only at sizes rounded
    /// up to a multiple of 8 px (cached once) and scaled down to the exact
    /// size when drawn.
    pub fn glyph(&self, ch: char, cx: f32, cy: f32, size: f32, col: Color) {
        let s = ch.to_string();
        let target = (size * self.k).max(4.0);
        let base = ((target / 8.0).ceil() * 8.0).min(400.0);
        let fs = base as u16;
        let scale = target / base;
        let d = measure_text(&s, Some(&self.font), fs, scale);
        let x = self.px(cx) - d.width * 0.5;
        let y = self.py(cy) - d.height * 0.5 + d.offset_y;
        draw_text_ex(
            &s,
            x,
            y,
            TextParams { font: Some(&self.font), font_size: fs, font_scale: scale, color: self.c(col), ..Default::default() },
        );
    }

    /// The layered icon look: a faint ghost copy behind, a drop shadow, then
    /// the glyph itself.
    pub fn icon(&self, ch: char, cx: f32, cy: f32, size: f32, col: Color) {
        let ghost = Color::new(col.r, col.g, col.b, col.a * 0.28);
        self.glyph(ch, cx - size * 0.16, cy - size * 0.09, size * 0.86, ghost);
        self.glyph(ch, cx + size * 0.04, cy + size * 0.05, size, Color::new(0.0, 0.0, 0.0, 0.55 * col.a));
        self.glyph(ch, cx, cy, size, col);
    }

    // ------------------------------------------------------------ widget animation

    /// Mark a widget as pressed now (mouse click or its hotkey).
    pub fn press(&self, key: &str) {
        self.presses.borrow_mut().insert(key.to_string(), self.t);
    }

    /// Seconds since the widget was last pressed (large if never).
    pub fn since_press(&self, key: &str) -> f32 {
        self.presses.borrow().get(key).map(|t0| (self.t - t0) as f32).unwrap_or(99.0)
    }

    /// Highlight level of a widget, gliding toward 1 while `on` (or for
    /// FADE seconds after a press) and back toward 0 otherwise, so buttons
    /// turn green and grey again smoothly instead of switching.
    pub fn glow(&self, key: &str, on: bool) -> f32 {
        let on = on || self.since_press(key) < FADE;
        let mut map = self.glow.borrow_mut();
        let e = map.entry(key.to_string()).or_insert((0.0, self.t));
        let dt = ((self.t - e.1) as f32).clamp(0.0, 0.1);
        e.1 = self.t;
        let step = dt / FADE;
        e.0 = if on { (e.0 + step).min(1.0) } else { (e.0 - step).max(0.0) };
        let x = e.0;
        x * x * (3.0 - 2.0 * x)
    }

    /// Press "pop": quick swell to 1 in 70 ms, then an eased return to 0.
    pub fn pop(&self, key: &str) -> f32 {
        let t = self.since_press(key);
        if t < 0.07 {
            let x = t / 0.07;
            1.0 - (1.0 - x) * (1.0 - x)
        } else {
            let x = ((t - 0.07) / 0.26).clamp(0.0, 1.0);
            1.0 - x * x * (3.0 - 2.0 * x)
        }
    }

    // ------------------------------------------------------------ widgets

    /// White ring with an arrow inside; `dir` = -1 left, 1 right, -2 up, 2 down.
    pub fn circle_arrow(&self, cx: f32, cy: f32, r: f32, dir: i32, hot: bool) {
        let col = if hot { LIME } else { Color::new(0.93, 0.94, 0.94, 0.92) };
        self.circle_arrow_col(cx, cy, r, dir, col);
    }

    /// An arrow button that reacts: it glides to lime on hover, and when
    /// pressed (click or arrow key) it also swells ~30 % and eases back.
    /// With nothing left in its direction (`enabled` false) it fades to a
    /// dim grey and stops reacting.
    pub fn arrow_btn(&self, key: &str, cx: f32, cy: f32, r: f32, dir: i32, hover: bool, enabled: bool) {
        let off = self.glow(&format!("{key}#off"), !enabled);
        let g = self.glow(key, hover && enabled) * (1.0 - off);
        let p = self.pop(key) * (1.0 - off);
        let base = mix(Color::new(0.93, 0.94, 0.94, 0.92), Color::new(0.50, 0.52, 0.53, 0.45), off);
        let col = mix(base, LIME, g);
        if p > 0.0 {
            self.glow_disc(cx, cy, r * 2.0, Color::new(LIME.r, LIME.g, LIME.b, 0.45 * p));
        }
        self.circle_arrow_col(cx, cy, r * (1.0 + 0.30 * p), dir, col);
    }

    fn glow_disc(&self, cx: f32, cy: f32, r: f32, col: Color) {
        self.glow_soft(cx, cy, r, col);
    }

    fn circle_arrow_col(&self, cx: f32, cy: f32, r: f32, dir: i32, col: Color) {
        self.disc(cx, cy, r, Color::new(0.0, 0.0, 0.0, 0.18));
        self.arc(cx, cy, r - 2.6, r, 0.0, std::f32::consts::TAU, col);
        let (ux, uy) = match dir {
            -1 => (-1.0, 0.0),
            1 => (1.0, 0.0),
            -2 => (0.0, -1.0),
            _ => (0.0, 1.0),
        };
        let l = r * 0.5;
        let (tx, ty) = (cx + ux * l, cy + uy * l);
        self.line(cx - ux * l, cy - uy * l, tx, ty, 2.6, col);
        let hl = r * 0.36;
        // arrow head: two strokes back from the tip
        let (px, py) = (-uy, ux);
        self.line(tx, ty, tx - ux * hl + px * hl, ty - uy * hl + py * hl, 2.6, col);
        self.line(tx, ty, tx - ux * hl - px * hl, ty - uy * hl - py * hl, 2.6, col);
    }

    /// A keyboard keycap. Content is either text or a glyph. Returns width.
    pub fn keycap(&self, x: f32, cy: f32, label: &str, glyph: Option<char>) -> f32 {
        let h = 26.0;
        let size = 15.0;
        let inner = match glyph {
            Some(_) => 16.0,
            None => self.measure_with(&self.black, label, size),
        };
        let w = (inner + 16.0).max(h);
        let y = cy - h * 0.5;
        self.rrect(x, y + 3.0, w, h, 6.0, Color::new(0.0, 0.0, 0.0, 0.55));
        self.rrect_grad(x, y, w, h, 6.0, Color::new(0.95, 0.96, 0.96, 0.96), Color::new(0.64, 0.67, 0.68, 0.96));
        self.rrect_line(x, y, w, h, 6.0, 1.2, Color::new(0.15, 0.16, 0.17, 0.9));
        let dark = Color::new(0.10, 0.11, 0.12, 1.0);
        match glyph {
            Some(g) => self.glyph(g, x + w * 0.5, cy, 18.0, dark),
            None => {
                self.text_with(&self.black, label, x + w * 0.5, cy + size * 0.36, size, dark, Align::Center);
            }
        }
        w
    }

    /// Keycap(s) followed by a caption, the way console prompts read.
    /// Returns the total width.
    pub fn prompt(&self, x: f32, cy: f32, caps: &[(&str, Option<char>)], caption: &str) -> f32 {
        let mut cx = x;
        for (label, g) in caps {
            cx += self.keycap(cx, cy, label, *g) + 4.0;
        }
        cx += 4.0;
        cx += self.text_sh(caption, cx, cy + 7.0, 20.0, TXT, Align::Left);
        cx - x
    }

    /// The game's footer button: a long, slightly rounded black bar with a
    /// grey outline and white italic label; outline and label glide to lime
    /// (FADE seconds) on hover or press. `hint` is the hotkey, shown small
    /// and dim at the left end. Returns the button's rect.
    pub fn button(&self, key: &str, x: f32, y: f32, w: f32, h: f32, label: &str, hint: &str, hover: bool) -> Rect {
        let g = self.glow(key, hover);
        let edge = mix(Color::new(0.72, 0.74, 0.75, 0.95), LIME, g);
        let txt = mix(TXT, LIME, g);
        self.rrect(x, y, w, h, 7.0, Color::new(0.02, 0.025, 0.03, 0.86));
        if g > 0.0 {
            self.rrect(x, y, w, h, 7.0, Color::new(LIME.r, LIME.g, LIME.b, 0.07 * g));
        }
        self.rrect_line(x, y, w, h, 7.0, 1.6, edge);
        let size = (h * 0.66).min(19.0);
        self.text_sh(label, x + w * 0.5, y + h * 0.5 + size * 0.36, size, txt, Align::Center);
        if !hint.is_empty() {
            self.text_with(&self.black, hint, x + 10.0, y + h * 0.5 + 4.5, 12.0, mix(Color::new(0.55, 0.58, 0.6, 0.9), LIME, g * 0.6), Align::Left);
        }
        Rect::new(x, y, w, h)
    }

    /// Small outlined tag chip, e.g. "TERMINAL". Returns width.
    pub fn chip(&self, x: f32, cy: f32, label: &str, col: Color) -> f32 {
        let size = 14.0;
        let w = self.measure_with(&self.black, label, size) + 16.0;
        let h = 20.0;
        self.rrect(x, cy - h * 0.5, w, h, 5.0, Color::new(0.0, 0.0, 0.0, 0.55));
        self.rrect_line(x, cy - h * 0.5, w, h, 5.0, 1.3, col);
        self.text_with(&self.black, label, x + w * 0.5, cy + 5.0, size, col, Align::Center);
        w
    }
}

/// Is the point inside the rect?
pub fn hit(r: &Rect, p: Vec2) -> bool {
    p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h
}
