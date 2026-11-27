//! Garage settings, kept in ~/.config/tiramisu-garage/options.conf (key=value).

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub sound: bool,
    /// 0..=10
    pub volume: u8,
    pub spin: bool,
    /// false = quick transitions
    pub anim_full: bool,
    pub fullscreen: bool,
    /// 0 any, 1 vivo, 2 redmi
    pub phone: u8,
    /// log text size in virtual px, 12..=24
    pub log_size: u8,
    /// adb serial of the vivo, for the phone picker over USB (optional:
    /// without it the picker matches any adb device whose model says vivo)
    pub vivo_serial: String,
}

impl Default for Options {
    fn default() -> Self {
        Options { sound: true, volume: 6, spin: true, anim_full: true, fullscreen: false, phone: 0, log_size: 16, vivo_serial: String::new() }
    }
}

fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("tiramisu-garage").join("options.conf")
}

impl Options {
    pub fn load() -> Options {
        let mut o = Options::default();
        let Ok(text) = std::fs::read_to_string(path()) else { return o };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let b = v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("on");
            let n = v.parse::<u8>().ok();
            match k {
                "sound" => o.sound = b,
                "volume" => o.volume = n.unwrap_or(o.volume).min(10),
                "spin" => o.spin = b,
                "anim_full" => o.anim_full = b,
                "fullscreen" => o.fullscreen = b,
                "phone" => o.phone = n.unwrap_or(0).min(2),
                "log_size" => o.log_size = n.unwrap_or(16).clamp(12, 24),
                "vivo_serial" => o.vivo_serial = v.chars().filter(|c| c.is_ascii_alphanumeric() || ".:-_".contains(*c)).collect(),
                _ => {}
            }
        }
        o
    }

    pub fn save(&self) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = format!(
            "# Tiramisu Garage options (written by the app)\nsound={}\nvolume={}\nspin={}\nanim_full={}\nfullscreen={}\nphone={}\nlog_size={}\nvivo_serial={}\n",
            self.sound as u8, self.volume, self.spin as u8, self.anim_full as u8, self.fullscreen as u8, self.phone, self.log_size, self.vivo_serial
        );
        let _ = std::fs::write(p, text);
    }
}
