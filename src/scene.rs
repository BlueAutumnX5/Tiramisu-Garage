//! The backdrop: a dim garage with a hazard-striped turntable, and the "hero
//! car" spinning on it, which here is the laptop all these tools look after.
//!
//! Plain macroquad 3D with no lighting of its own, so every face is shaded
//! by hand from its normal against one key light. Geometry is rebuilt each
//! frame (a few thousand vertices), which also makes the turntable rotation a
//! simple transform on the CPU.

use crate::anim::Spring;
use macroquad::models::{Mesh, Vertex, draw_mesh};
use macroquad::prelude::*;

const LIGHT: Vec3 = Vec3::new(-0.45, 0.80, 0.55);

struct Mb {
    v: Vec<Vertex>,
    i: Vec<u16>,
}

impl Mb {
    fn new() -> Self {
        Mb { v: Vec::with_capacity(4096), i: Vec::with_capacity(6144) }
    }
    fn flush(&mut self) {
        if !self.i.is_empty() {
            draw_mesh(&Mesh { vertices: std::mem::take(&mut self.v), indices: std::mem::take(&mut self.i), texture: None });
        }
    }
    fn room(&mut self, nv: usize) {
        if self.v.len() + nv > 9000 || self.i.len() + nv * 2 > 4800 {
            self.flush();
        }
    }
    /// Quad with one colour per corner.
    fn quad4(&mut self, p: [Vec3; 4], c: [Color; 4]) {
        self.room(4);
        let b = self.v.len() as u16;
        for k in 0..4 {
            self.v.push(Vertex::new2(p[k], Vec2::ZERO, c[k]));
        }
        self.i.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    fn quad(&mut self, p: [Vec3; 4], c: Color) {
        self.quad4(p, [c; 4]);
    }
    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, ca: Color, cb: Color, cc: Color) {
        self.room(3);
        let base = self.v.len() as u16;
        self.v.push(Vertex::new2(a, Vec2::ZERO, ca));
        self.v.push(Vertex::new2(b, Vec2::ZERO, cb));
        self.v.push(Vertex::new2(c, Vec2::ZERO, cc));
        self.i.extend_from_slice(&[base, base + 1, base + 2]);
    }
}

fn shade(c: Color, n: Vec3) -> Color {
    let d = n.normalize_or_zero().dot(LIGHT.normalize()).max(0.0);
    let k = 0.30 + 0.85 * d;
    Color::new((c.r * k).min(1.0), (c.g * k).min(1.0), (c.b * k).min(1.0), c.a)
}

fn rgb(hex: u32) -> Color {
    Color::from_rgba((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
}

pub struct Scene {
    pub angle: f32,
    speed: Spring,
}

impl Scene {
    pub fn new() -> Self {
        Scene { angle: 0.6, speed: Spring::new(0.0) }
    }

    /// `dim` 0..1 darkens the room while a full-screen panel is open.
    pub fn draw(&mut self, dt: f32, spin: bool, t: f64, dim: f32) {
        self.speed.target = if spin { 0.30 } else { 0.0 };
        self.speed.update(dt, 2.0);
        self.angle += self.speed.value * dt;

        clear_background(Color::new(0.035, 0.038, 0.042, 1.0));
        set_camera(&Camera3D {
            position: vec3(0.0, 3.1, 10.9),
            target: vec3(0.0, 1.45, 0.0),
            up: Vec3::Y,
            fovy: 34f32.to_radians(),
            // the default near plane (0.01) wastes almost all depth precision
            // right in front of the lens and made the laptop screen z-fight
            z_near: 1.0,
            z_far: 60.0,
            ..Default::default()
        });

        let mut mb = Mb::new();
        self.room(&mut mb, t);
        self.turntable(&mut mb, t);
        self.laptop(&mut mb, t);
        mb.flush();

        set_default_camera();
        self.overlay(t, dim);
    }

    fn room(&self, mb: &mut Mb, t: f64) {
        // floor: polished concrete, darker toward the back
        let near = rgb(0x26292c);
        let far = rgb(0x0e1012);
        mb.quad4(
            [vec3(-16.0, 0.0, 7.0), vec3(16.0, 0.0, 7.0), vec3(16.0, 0.0, -10.0), vec3(-16.0, 0.0, -10.0)],
            [near, near, far, far],
        );
        // floor seams
        let seam = Color::new(0.0, 0.0, 0.0, 0.35);
        for k in -4..=4 {
            let x = k as f32 * 3.2;
            mb.quad([vec3(x - 0.02, 0.003, 7.0), vec3(x + 0.02, 0.003, 7.0), vec3(x + 0.02, 0.003, -10.0), vec3(x - 0.02, 0.003, -10.0)], seam);
        }
        // back wall
        let wall_lo = rgb(0x15181b);
        let wall_hi = rgb(0x0b0c0e);
        mb.quad4(
            [vec3(-16.0, 0.0, -10.0), vec3(16.0, 0.0, -10.0), vec3(16.0, 8.0, -10.0), vec3(-16.0, 8.0, -10.0)],
            [wall_lo, wall_lo, wall_hi, wall_hi],
        );
        // wall light strip + a slow-breathing neon sign bar
        let strip = Color::new(0.42, 0.48, 0.52, 0.55);
        mb.quad([vec3(-15.0, 0.9, -9.98), vec3(15.0, 0.9, -9.98), vec3(15.0, 0.97, -9.98), vec3(-15.0, 0.97, -9.98)], strip);
        let breathe = 0.55 + 0.25 * ((t * 1.3).sin() as f32);
        let neon = Color::new(0.30, 0.85, 0.80, breathe);
        mb.quad([vec3(-7.6, 1.55, -9.97), vec3(-4.4, 1.55, -9.97), vec3(-4.4, 1.63, -9.97), vec3(-7.6, 1.63, -9.97)], neon);
        let neon2 = Color::new(0.85, 0.25, 0.20, 0.55 + 0.2 * ((t * 0.9 + 1.0).sin() as f32));
        mb.quad([vec3(4.6, 1.35, -9.97), vec3(7.8, 1.35, -9.97), vec3(7.8, 1.42, -9.97), vec3(4.6, 1.42, -9.97)], neon2);
        // pillars
        for x in [-9.0f32, 9.0] {
            let c = rgb(0x1a1d20);
            mb.quad([vec3(x - 0.5, 0.0, -9.5), vec3(x + 0.5, 0.0, -9.5), vec3(x + 0.5, 8.0, -9.5), vec3(x - 0.5, 8.0, -9.5)], shade(c, Vec3::Z));
            mb.quad([vec3(x + 0.5, 0.0, -9.5), vec3(x + 0.5, 0.0, -10.0), vec3(x + 0.5, 8.0, -10.0), vec3(x + 0.5, 8.0, -9.5)], shade(c, Vec3::X));
            mb.quad([vec3(x - 0.5, 0.0, -10.0), vec3(x - 0.5, 0.0, -9.5), vec3(x - 0.5, 8.0, -9.5), vec3(x - 0.5, 8.0, -10.0)], shade(c, -Vec3::X));
        }
        // ceiling tube lights, receding rows
        for row in 0..5 {
            let z = 4.0 - row as f32 * 3.0;
            for col in -2..=2 {
                let x = col as f32 * 3.4;
                let c = Color::new(0.85, 0.88, 0.9, 0.95);
                mb.quad([vec3(x - 1.1, 6.2, z - 0.10), vec3(x + 1.1, 6.2, z - 0.10), vec3(x + 1.1, 6.2, z + 0.10), vec3(x - 1.1, 6.2, z + 0.10)], c);
            }
        }
    }

    fn turntable(&self, mb: &mut Mb, t: f64) {
        let (s, c) = self.angle.sin_cos();
        let rot = |p: Vec3| vec3(p.x * c - p.z * s, p.y, p.x * s + p.z * c);
        let r = 2.75f32;
        let top = 0.13f32;
        let seg = 64;
        let yellow = rgb(0xd9b22a);
        let black = rgb(0x121314);
        let deck_in = rgb(0x34373a);
        let deck_out = rgb(0x222426);
        for k in 0..seg {
            let a0 = k as f32 / seg as f32 * std::f32::consts::TAU;
            let a1 = (k + 1) as f32 / seg as f32 * std::f32::consts::TAU;
            let p = |a: f32, rr: f32, y: f32| rot(vec3(a.cos() * rr, y, a.sin() * rr));
            // deck
            mb.tri(p(0.0, 0.0, top), p(a0, 2.45, top), p(a1, 2.45, top), deck_in, deck_out, deck_out);
            // striped rim ring on top
            let stripe = if (k / 2) % 2 == 0 { yellow } else { black };
            mb.quad([p(a0, 2.45, top + 0.006), p(a0, r, top + 0.006), p(a1, r, top + 0.006), p(a1, 2.45, top + 0.006)], stripe);
            // outer wall
            let mid = (a0 + a1) * 0.5;
            let n = rot(vec3(mid.cos(), 0.0, mid.sin()));
            let wall = shade(if (k / 2) % 2 == 0 { yellow } else { black }, n);
            mb.quad([p(a0, r, 0.0), p(a1, r, 0.0), p(a1, r, top), p(a0, r, top)], wall);
        }
        // neon under-glow on the deck: soft, slowly breathing
        let pulse = 0.38 + 0.12 * ((t * 1.7).sin() as f32);
        let glow = Color::new(0.45, 0.95, 0.35, pulse);
        let edge = Color::new(0.45, 0.95, 0.35, 0.0);
        for k in 0..48 {
            let a0 = k as f32 / 48.0 * std::f32::consts::TAU;
            let a1 = (k + 1) as f32 / 48.0 * std::f32::consts::TAU;
            let e = |a: f32| rot(vec3(a.cos() * 2.3, top + 0.014, a.sin() * 1.7));
            mb.tri(rot(vec3(0.0, top + 0.014, 0.0)), e(a0), e(a1), glow, edge, edge);
        }
    }

    fn laptop(&self, mb: &mut Mb, t: f64) {
        let (s, c) = self.angle.sin_cos();
        let rot = |p: Vec3| vec3(p.x * c - p.z * s, p.y, p.x * s + p.z * c);
        let body = rgb(0x2b2f34);
        let (w, d) = (3.3f32, 2.25f32);
        let y0 = 0.16f32;
        let th = 0.13f32;

        // a box from an origin and three edge vectors, every face shaded
        let mut boxed = |o: Vec3, e1: Vec3, e2: Vec3, e3: Vec3, col: Color| {
            let p = |a: f32, b: f32, cc: f32| rot(o + e1 * a + e2 * b + e3 * cc);
            let faces: [([(f32, f32, f32); 4], Vec3); 6] = [
                ([(0., 0., 1.), (1., 0., 1.), (1., 1., 1.), (0., 1., 1.)], e3),
                ([(0., 0., 0.), (0., 1., 0.), (1., 1., 0.), (1., 0., 0.)], -e3),
                ([(0., 0., 0.), (1., 0., 0.), (1., 0., 1.), (0., 0., 1.)], -e2),
                ([(0., 1., 0.), (0., 1., 1.), (1., 1., 1.), (1., 1., 0.)], e2),
                ([(0., 0., 0.), (0., 0., 1.), (0., 1., 1.), (0., 1., 0.)], -e1),
                ([(1., 0., 0.), (1., 1., 0.), (1., 1., 1.), (1., 0., 1.)], e1),
            ];
            for (q, n) in faces {
                let pts = q.map(|(a, b, cc)| p(a, b, cc));
                mb.quad(pts, shade(col, rot(n)));
            }
        };

        // base: x across, z depth (front = +z), y up
        let o = vec3(-w * 0.5, y0, -d * 0.5);
        boxed(o, vec3(w, 0.0, 0.0), vec3(0.0, 0.0, d), vec3(0.0, th, 0.0), body);

        // lid, hinged at the back edge, opened ~112 degrees
        let phi = 112f32.to_radians();
        let dir = vec3(0.0, phi.sin(), phi.cos());
        let nrm = vec3(0.0, phi.cos(), -phi.sin());
        let hinge = vec3(-w * 0.5, y0 + th, -d * 0.5);
        boxed(hinge, vec3(w, 0.0, 0.0), dir * d, nrm * 0.085, rgb(0x30353b));

        drop(boxed);

        // keyboard deck, keys, touchpad
        let ytop = y0 + th + 0.008;
        let deck = rgb(0x17191c);
        let q = |x0: f32, z0: f32, x1: f32, z1: f32, y: f32| [rot(vec3(x0, y, z0)), rot(vec3(x1, y, z0)), rot(vec3(x1, y, z1)), rot(vec3(x0, y, z1))];
        mb.quad(q(-1.45, -0.98, 1.45, 0.05, ytop), shade(deck, Vec3::Y));
        let key = rgb(0x3a3f45);
        let rows = 6;
        let cols = 15;
        for r in 0..rows {
            for k in 0..cols {
                let kx0 = -1.40 + k as f32 * (2.80 / cols as f32);
                let kz0 = -0.94 + r as f32 * (0.96 / rows as f32);
                let kw = 2.80 / cols as f32 - 0.035;
                let kd = 0.96 / rows as f32 - 0.035;
                let wide = if r == rows - 1 && (5..10).contains(&k) { 1 } else { 0 };
                if wide == 1 && k != 5 {
                    continue; // space bar
                }
                let kw = if wide == 1 { kw * 5.0 + 0.035 * 4.0 } else { kw };
                mb.quad(q(kx0, kz0, kx0 + kw, kz0 + kd, ytop + 0.016), shade(key, Vec3::Y));
            }
        }
        // keyboard backlight bleed (red, like a gaming board)
        let bleed = Color::new(0.9, 0.15, 0.1, 0.10 + 0.04 * ((t * 2.1).sin() as f32));
        mb.quad(q(-1.45, -0.98, 1.45, 0.05, ytop + 0.008), bleed);
        mb.quad(q(-0.55, 0.22, 0.55, 0.92, ytop), shade(rgb(0x23272b), Vec3::Y));

        // screen on the inside of the lid
        let layer = |a: f32, b: f32, lift: f32| rot(hinge + vec3(w * a, 0.0, 0.0) + dir * (d * b) - nrm * lift);
        let face = |a: f32, b: f32| layer(a, b, 0.008);
        let bez = 0.05;
        let scr = [face(bez, 0.08), face(1.0 - bez, 0.08), face(1.0 - bez, 0.95), face(bez, 0.95)];
        let s_lo = rgb(0x0b1a2a);
        let s_hi = rgb(0x1d3f5e);
        mb.quad4(scr, [s_lo, s_lo, s_hi, s_hi]);
        // a tiny garage menu on the tiny screen
        let bar = Color::new(0.55, 0.58, 0.58, 0.55);
        let bar_l = |a: f32, b: f32| layer(a, b, 0.018);
        mb.quad([bar_l(0.1, 0.62), bar_l(0.9, 0.62), bar_l(0.9, 0.78), bar_l(0.1, 0.78)], bar);
        let lime = Color::new(0.78, 0.89, 0.32, 0.6 + 0.4 * (((t * 3.5).sin() as f32) * 0.5 + 0.5));
        let sel = 0.1 + 0.27 * (((t * 0.5).floor() as i64).rem_euclid(3) as f32);
        let top_l = |a: f32, b: f32| layer(a, b, 0.028);
        mb.quad([top_l(sel, 0.63), top_l(sel + 0.26, 0.63), top_l(sel + 0.26, 0.77), top_l(sel, 0.77)], lime);
        mb.quad([bar_l(0.06, 0.86), bar_l(0.3, 0.86), bar_l(0.3, 0.9), bar_l(0.06, 0.9)], Color::new(0.78, 0.89, 0.32, 0.9));
        // a thin lime light line along the front edge
        let fy = y0 + th * 0.5;
        mb.quad(
            [rot(vec3(-1.2, fy - 0.012, d * 0.5 + 0.004)), rot(vec3(1.2, fy - 0.012, d * 0.5 + 0.004)), rot(vec3(1.2, fy + 0.012, d * 0.5 + 0.004)), rot(vec3(-1.2, fy + 0.012, d * 0.5 + 0.004))],
            Color::new(0.78, 0.89, 0.32, 0.85),
        );
    }

    /// 2D finishing: light beams from the ceiling, vignette, dimming.
    fn overlay(&self, t: f64, dim: f32) {
        let (w, h) = (screen_width(), screen_height());
        let beam = |x_top: f32, x_bot: f32, spread: f32, a: f32| {
            let c0 = Color::new(0.85, 0.9, 1.0, a);
            let c1 = Color::new(0.85, 0.9, 1.0, 0.0);
            let v = vec![
                Vertex::new(x_top - spread * 0.15, -10.0, 0.0, 0.0, 0.0, c0),
                Vertex::new(x_top + spread * 0.15, -10.0, 0.0, 0.0, 0.0, c0),
                Vertex::new(x_bot + spread, h * 0.95, 0.0, 0.0, 0.0, c1),
                Vertex::new(x_bot - spread, h * 0.95, 0.0, 0.0, 0.0, c1),
            ];
            draw_mesh(&Mesh { vertices: v, indices: vec![0, 1, 2, 0, 2, 3], texture: None });
        };
        let flick = 0.055 + 0.01 * ((t * 7.0).sin() as f32 * (t * 2.3).cos() as f32);
        beam(w * 0.20, w * 0.36, w * 0.16, flick);
        beam(w * 0.82, w * 0.66, w * 0.16, flick * 0.9);

        // vignette: four gradient edges
        let dark = Color::new(0.0, 0.0, 0.0, 0.55);
        let clear = Color::new(0.0, 0.0, 0.0, 0.0);
        let grad = |pts: [Vec2; 4], cols: [Color; 4]| {
            let v = (0..4).map(|k| Vertex::new(pts[k].x, pts[k].y, 0.0, 0.0, 0.0, cols[k])).collect();
            draw_mesh(&Mesh { vertices: v, indices: vec![0, 1, 2, 0, 2, 3], texture: None });
        };
        let e = w.min(h) * 0.28;
        grad([vec2(0.0, 0.0), vec2(e, 0.0), vec2(e, h), vec2(0.0, h)], [dark, clear, clear, dark]);
        grad([vec2(w - e, 0.0), vec2(w, 0.0), vec2(w, h), vec2(w - e, h)], [clear, dark, dark, clear]);
        grad([vec2(0.0, h - e), vec2(w, h - e), vec2(w, h), vec2(0.0, h)], [clear, clear, dark, dark]);
        if dim > 0.0 {
            draw_rectangle(0.0, 0.0, w, h, Color::new(0.0, 0.0, 0.0, 0.45 * dim));
        }
        let _ = mix;
    }
}
