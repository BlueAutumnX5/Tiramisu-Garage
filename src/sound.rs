//! Menu sounds, synthesised at start-up (no sound files shipped).
//!
//! Each effect is rendered to a 44.1 kHz 16-bit stereo WAV in memory and
//! handed to macroquad's mixer: a short tick for browsing, a pneumatic
//! "shunk" for accept, a rising blip for back, a filtered-noise whoosh for
//! screen changes, a buzz for errors and a two-tone chime when a job ends.

use macroquad::audio::{PlaySoundParams, Sound, load_sound_from_bytes, play_sound};

const RATE: u32 = 44_100;

#[derive(Clone, Copy)]
pub enum Fx {
    Tick,
    Accept,
    Back,
    Whoosh,
    Error,
    Done,
}

pub struct Sfx {
    tick: Option<Sound>,
    accept: Option<Sound>,
    back: Option<Sound>,
    whoosh: Option<Sound>,
    error: Option<Sound>,
    done: Option<Sound>,
}

/// Tiny deterministic noise source (xorshift).
struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn wav(samples: &[f32]) -> Vec<u8> {
    let n = samples.len() as u32;
    let data_len = n * 4; // stereo, 16-bit
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&2u16.to_le_bytes()); // channels
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 4).to_le_bytes());
    v.extend_from_slice(&4u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let i = (s.clamp(-1.0, 1.0) * 32000.0) as i16;
        v.extend_from_slice(&i.to_le_bytes());
        v.extend_from_slice(&i.to_le_bytes());
    }
    v
}

fn render(secs: f32, mut f: impl FnMut(f32, &mut Noise) -> f32) -> Vec<u8> {
    let n = (secs * RATE as f32) as usize;
    let mut noise = Noise(0x9E37_79B9);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        // 3 ms fade at both ends: no clicks
        let edge = (t / 0.003).min((secs - t) / 0.003).clamp(0.0, 1.0);
        out.push(f(t, &mut noise) * edge);
    }
    wav(&out)
}

fn tau() -> f32 {
    std::f32::consts::TAU
}

fn gen_tick() -> Vec<u8> {
    render(0.045, |t, n| {
        let env = (-t * 140.0).exp();
        ((tau() * 2350.0 * t).sin() * 0.55 + n.next() * 0.25) * env * 0.7
    })
}

fn gen_accept() -> Vec<u8> {
    let mut phase = 0.0f32;
    let mut lp = 0.0f32;
    render(0.17, move |t, n| {
        let f = 820.0 * (-t * 9.0).exp() + 190.0;
        phase += f / RATE as f32;
        let sq = if (phase * tau()).sin() > 0.0 { 1.0 } else { -1.0 };
        lp += (n.next() - lp) * 0.25;
        let body = sq * 0.32 * (-t * 22.0).exp();
        let hiss = lp * 0.6 * (-t * 30.0).exp();
        (body + hiss) * 0.75
    })
}

fn gen_back() -> Vec<u8> {
    let mut phase = 0.0f32;
    render(0.12, move |t, _| {
        let f = 330.0 + 900.0 * (t / 0.12);
        phase += f / RATE as f32;
        (phase * tau()).sin() * 0.45 * (-t * 18.0).exp()
    })
}

fn gen_whoosh() -> Vec<u8> {
    let (mut lp1, mut lp2) = (0.0f32, 0.0f32);
    render(0.30, move |t, n| {
        let x = t / 0.30;
        let swell = (x * std::f32::consts::PI).sin().powf(1.6);
        let cut = 0.03 + 0.22 * x; // the band sweeps upward
        lp1 += (n.next() - lp1) * cut;
        lp2 += (lp1 - lp2) * cut;
        (lp1 - lp2) * 2.6 * swell
    })
}

fn gen_error() -> Vec<u8> {
    render(0.20, |t, _| {
        let saw = ((t * 110.0).fract() * 2.0 - 1.0) * 0.35 + ((t * 116.0).fract() * 2.0 - 1.0) * 0.25;
        saw * (1.0 - t / 0.2)
    })
}

fn gen_done() -> Vec<u8> {
    render(0.42, |t, _| {
        let a = (tau() * 880.0 * t).sin() * (-t * 9.0).exp();
        let b = if t > 0.11 { (tau() * 1318.5 * (t - 0.11)).sin() * (-(t - 0.11) * 7.0).exp() } else { 0.0 };
        (a + b) * 0.32
    })
}

impl Sfx {
    pub async fn new() -> Sfx {
        async fn load(bytes: Vec<u8>) -> Option<Sound> {
            load_sound_from_bytes(&bytes).await.ok()
        }
        Sfx {
            tick: load(gen_tick()).await,
            accept: load(gen_accept()).await,
            back: load(gen_back()).await,
            whoosh: load(gen_whoosh()).await,
            error: load(gen_error()).await,
            done: load(gen_done()).await,
        }
    }

    pub fn play(&self, fx: Fx, enabled: bool, volume: u8) {
        if !enabled || volume == 0 {
            return;
        }
        let s = match fx {
            Fx::Tick => &self.tick,
            Fx::Accept => &self.accept,
            Fx::Back => &self.back,
            Fx::Whoosh => &self.whoosh,
            Fx::Error => &self.error,
            Fx::Done => &self.done,
        };
        if let Some(s) = s {
            play_sound(s, PlaySoundParams { looped: false, volume: volume as f32 / 10.0 });
        }
    }
}
