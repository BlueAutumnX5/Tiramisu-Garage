//! MSI Thin 15 (board MS-16R8) EC fan curves, ported from fan_curve_tuner.py.
//!
//! The EC holds two curve tables. Table 1 follows CPU temperature, table 2
//! GPU temperature, and the one physical fan runs at whichever asks for more.
//!
//!     0x6a..0x6f  table 1 temperature thresholds  (6, °C, ascending)
//!     0x72..0x78  table 1 speed steps             (7, percent)
//!     0x82..0x87  table 2 temperature thresholds  (6)
//!     0x8a..0x90  table 2 speed steps             (7)
//!     0xd4        fan mode (0x0d auto, 0x8d advanced)
//!
//! speed[0] applies below temp[0], speed[n] between temp[n-1] and temp[n],
//! speed[6] above temp[5].
//!
//! All EC access goes through MControlCenter's root helper on the system bus
//! (`mcontrolcenter.helper`, the path its own GUI uses), via `busctl`, so no
//! password and no extra crates. Apply keeps the three places that hold the
//! curve in step: the EC, ~/.config/MControlCenter.conf, and the boot fallback
//! /etc/default/msi-ec-profile (only touched when it differs, via pkexec).
//! Never poll the EC fast: the keyboard sits on the same controller.

use std::path::PathBuf;
use std::process::Command;

pub const T1: usize = 0x6a;
pub const S1: usize = 0x72;
pub const T2: usize = 0x82;
pub const S2: usize = 0x8a;
pub const FAN_MODE: usize = 0xd4;
pub const FAN_MODE_ADVANCED: u8 = 0x8d;

pub const N_TEMPS: usize = 6;
pub const N_SPEEDS: usize = 7;
pub const TEMP_MIN: i32 = 20;
pub const TEMP_MAX: i32 = 100;
/// The EC accepts 0-150, but 100 % is already the fan's 6000 RPM ceiling
/// (steady-state sweep 2026-10-20: 50 % = 2944, 85 % = 5052, 100 % and up = 6000).
pub const SPEED_MAX: i32 = 100;
pub const RPM_PER_PCT: i32 = 60;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Curve {
    pub t: [i32; N_TEMPS],
    pub s: [i32; N_SPEEDS],
}

impl Curve {
    pub const fn new(t: [i32; N_TEMPS], s: [i32; N_SPEEDS]) -> Curve {
        Curve { t, s }
    }
    /// Speeds above SPEED_MAX cut to it; a curve that needed this shows as unsaved.
    pub fn clamped(mut self) -> Curve {
        for v in self.s.iter_mut() {
            *v = (*v).min(SPEED_MAX);
        }
        self
    }
    pub fn ascending(&self) -> bool {
        self.t.windows(2).all(|w| w[0] < w[1])
    }
}

/// MSI factory curve for MS-16R8 (both tables). Source: raw EC dump of an
/// untouched Thin GF63 12VE in BeardOverflow/msi-ec#690; it also matched this
/// laptop's never-edited table 2.
pub const FACTORY: Curve = Curve::new([50, 60, 65, 70, 75, 80], [0, 40, 50, 60, 70, 80, 85]);

pub const PRESETS: [(&str, Curve); 4] = [
    ("MSI factory", FACTORY),
    ("Quiet (old curve)", Curve::new([52, 60, 66, 71, 76, 80], [0, 0, 30, 50, 70, 90, 100])),
    ("Aggressive", Curve::new([50, 55, 62, 68, 74, 78], [0, 40, 55, 70, 90, 100, 100])),
    ("Maximum", Curve::new([40, 50, 58, 64, 70, 75], [40, 55, 70, 85, 100, 100, 100])),
];

fn busctl(args: &[&str]) -> Result<String, String> {
    let out = Command::new("busctl")
        .args(["--system", "--timeout=5", "call", "mcontrolcenter.helper", "/", "dmitry_s93.MControlCenter"])
        .args(args)
        .output()
        .map_err(|e| format!("busctl: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "MControlCenter's helper did not answer ({}). Is mcontrolcenter installed?",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Parse busctl's `ay 256 0 128 …` reply.
pub fn parse_ay(text: &str) -> Result<Vec<u8>, String> {
    let mut it = text.split_whitespace();
    if it.next() != Some("ay") {
        return Err(format!("unexpected reply: {}", text.chars().take(40).collect::<String>()));
    }
    let n: usize = it.next().and_then(|v| v.parse().ok()).ok_or("no length in reply")?;
    let bytes: Vec<u8> = it.filter_map(|v| v.parse().ok()).collect();
    if bytes.len() != n || n < 0x100 {
        return Err(format!("short EC dump: {} of {n} bytes", bytes.len()));
    }
    Ok(bytes)
}

/// One call reads all 256 bytes. Only on open, Apply and Revert.
pub fn dump() -> Result<Vec<u8>, String> {
    parse_ay(&busctl(&["getData"])?)
}

pub fn put(off: usize, val: i32) -> Result<(), String> {
    busctl(&["putValue", "ii", &off.to_string(), &val.to_string()]).map(|_| ())
}

fn curve_at(d: &[u8], t: usize, s: usize) -> Curve {
    let mut c = Curve::new([0; N_TEMPS], [0; N_SPEEDS]);
    for i in 0..N_TEMPS {
        c.t[i] = d[t + i] as i32;
    }
    for i in 0..N_SPEEDS {
        c.s[i] = d[s + i] as i32;
    }
    c
}

pub fn read_curves() -> Result<(Curve, Curve, u8), String> {
    let d = dump()?;
    Ok((curve_at(&d, T1, S1), curve_at(&d, T2, S2), d[FAN_MODE]))
}

/// Write only the bytes that differ, then the fan mode, then read back.
/// Returns the offsets the EC did not take.
pub fn write_curves(c1: &Curve, c2: &Curve) -> Result<Vec<String>, String> {
    let cur = dump()?;
    let mut wanted: Vec<(usize, i32)> = Vec::new();
    for (base, vals) in [(T1, &c1.t[..]), (S1, &c1.s[..]), (T2, &c2.t[..]), (S2, &c2.s[..])] {
        for (i, v) in vals.iter().enumerate() {
            wanted.push((base + i, *v));
        }
    }
    for &(off, v) in &wanted {
        if cur[off] as i32 != v {
            put(off, v)?;
        }
    }
    // curve first, then the mode: the same order msi-ec-profile uses
    if cur[FAN_MODE] != FAN_MODE_ADVANCED {
        put(FAN_MODE, FAN_MODE_ADVANCED as i32)?;
    }
    let after = dump()?;
    let mut bad: Vec<String> = wanted.iter().filter(|(o, v)| after[*o] as i32 != *v).map(|(o, _)| format!("{o:#04x}")).collect();
    if after[FAN_MODE] != FAN_MODE_ADVANCED {
        bad.push("fan_mode".into());
    }
    Ok(bad)
}

pub fn force_advanced() -> Result<(), String> {
    put(FAN_MODE, FAN_MODE_ADVANCED as i32)
}

fn join(v: &[i32]) -> String {
    v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("|")
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub fn mcc_conf_path() -> PathBuf {
    home().join(".config/MControlCenter.conf")
}

/// Rewrite the [Settings] curve keys of MControlCenter.conf, keeping everything else.
pub fn mcc_conf_text(old: &str, c1: &Curve, c2: &Curve) -> String {
    let keys = [
        ("fan1TempSettings", join(&c1.t)),
        ("fan1SpeedSettings", join(&c1.s)),
        ("fan2TempSettings", join(&c2.t)),
        ("fan2SpeedSettings", join(&c2.s)),
        ("fanModeAdvanced", "true".to_string()),
    ];
    let mut lines: Vec<String> = old.lines().map(String::from).collect();
    let mut seen = [false; 5];
    let mut section = String::new();
    for line in lines.iter_mut() {
        let tr = line.trim();
        if tr.starts_with('[') && tr.ends_with(']') {
            section = tr[1..tr.len() - 1].to_string();
            continue;
        }
        if section == "Settings" {
            if let Some((k, _)) = line.split_once('=') {
                let k = k.trim().to_string();
                if let Some(i) = keys.iter().position(|(kk, _)| *kk == k) {
                    *line = format!("{}={}", keys[i].0, keys[i].1);
                    seen[i] = true;
                }
            }
        }
    }
    if seen.iter().any(|s| !s) {
        if !lines.iter().any(|l| l.trim() == "[Settings]") {
            lines.push("[Settings]".into());
        }
        let idx = lines.iter().position(|l| l.trim() == "[Settings]").unwrap() + 1;
        for (i, (k, v)) in keys.iter().enumerate().rev() {
            if !seen[i] {
                lines.insert(idx, format!("{k}={v}"));
            }
        }
    }
    lines.join("\n") + "\n"
}

pub fn write_mcc_conf(c1: &Curve, c2: &Curve) -> std::io::Result<()> {
    let p = mcc_conf_path();
    let old = std::fs::read_to_string(&p).unwrap_or_else(|_| "[Settings]\n".into());
    let tmp = p.with_extension("conf.tmp");
    std::fs::write(&tmp, mcc_conf_text(&old, c1, c2))?;
    std::fs::rename(tmp, p)
}

pub const ETC_DEFAULT: &str = "/etc/default/msi-ec-profile";

pub fn etc_default_wanted(c1: &Curve, c2: &Curve, mirror: bool) -> Vec<(String, String)> {
    vec![
        ("FAN2_MIRRORS_FAN1".into(), if mirror { "yes" } else { "no" }.into()),
        ("FAN1_TEMPS".into(), format!("\"{}\"", join(&c1.t))),
        ("FAN1_SPEEDS".into(), format!("\"{}\"", join(&c1.s))),
        ("FAN2_TEMPS".into(), format!("\"{}\"", join(&c2.t))),
        ("FAN2_SPEEDS".into(), format!("\"{}\"", join(&c2.s))),
    ]
}

/// Does /etc/default/msi-ec-profile disagree? (false when it does not exist:
/// nothing to keep in sync on a machine without the service)
pub fn etc_default_needs_update(text: Option<&str>, wanted: &[(String, String)]) -> bool {
    let Some(text) = text else { return false };
    let have: Vec<(&str, &str)> = text
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
        .collect();
    wanted.iter().any(|(k, v)| have.iter().rev().find(|(hk, _)| hk == k).map(|(_, hv)| *hv) != Some(v.as_str()))
}

/// One pkexec call rewrites only the keys we own (KDE's polkit dialog asks).
pub fn write_etc_default(wanted: &[(String, String)]) -> Result<(), String> {
    let script: String = wanted.iter().map(|(k, v)| format!("s/^{k}=.*/{k}={v}/;")).collect();
    let out = Command::new("pkexec").args(["sed", "-i", "-e", &script, ETC_DEFAULT]).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if e.is_empty() { "cancelled".into() } else { e })
    }
}

/// What happened during Apply, for the message afterwards.
#[derive(Debug, Default)]
pub struct ApplyReport {
    pub conf_error: Option<String>,
    pub etc_note: Option<String>,
}

/// The whole Apply, meant to run on a worker thread (pkexec waits for the user).
pub fn apply(c1: Curve, c2: Curve, mirror: bool) -> Result<ApplyReport, String> {
    for (name, c) in [("CPU", &c1), ("GPU", &c2)] {
        if !c.ascending() {
            return Err(format!("{name} curve: temperatures must go strictly upwards."));
        }
    }
    let bad = write_curves(&c1, &c2)?;
    if !bad.is_empty() {
        return Err(format!("The EC did not take these bytes: {}", bad.join(", ")));
    }
    let mut rep = ApplyReport::default();
    if let Err(e) = write_mcc_conf(&c1, &c2) {
        rep.conf_error = Some(format!("EC updated, but MControlCenter.conf could not be written: {e}"));
    }
    let wanted = etc_default_wanted(&c1, &c2, mirror);
    let text = std::fs::read_to_string(ETC_DEFAULT).ok();
    if etc_default_needs_update(text.as_deref(), &wanted) {
        if let Err(e) = write_etc_default(&wanted) {
            rep.etc_note = Some(format!(
                "The boot fallback in {ETC_DEFAULT} was not updated ({e}). Your curve still applies at boot; that file only matters if MControlCenter.conf is missing."
            ));
        }
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dump() {
        let mut s = String::from("ay 256");
        for i in 0..256 {
            s.push_str(&format!(" {}", i % 200));
        }
        let d = parse_ay(&s).unwrap();
        assert_eq!(d.len(), 256);
        assert_eq!(d[0x6a], 0x6a);
        assert!(parse_ay("ay 3 1 2 3").is_err());
        assert!(parse_ay("garbage").is_err());
    }

    #[test]
    fn conf_rewrite_keeps_other_keys() {
        let old = "[MainWindow]\ngeometry=@ByteArray(x)\n\n[Settings]\nbatteryThreshold=80\nfan1SpeedSettings=1|2|3|4|5|6|7\nfan1TempSettings=1|2|3|4|5|6\n";
        let c = Curve::new([50, 56, 62, 68, 74, 78], [0, 30, 50, 70, 90, 100, 100]);
        let new = mcc_conf_text(old, &c, &FACTORY);
        assert!(new.contains("geometry=@ByteArray(x)"));
        assert!(new.contains("batteryThreshold=80"));
        assert!(new.contains("fan1SpeedSettings=0|30|50|70|90|100|100"));
        assert!(new.contains("fan1TempSettings=50|56|62|68|74|78"));
        assert!(new.contains("fan2TempSettings=50|60|65|70|75|80"));
        assert!(new.contains("fanModeAdvanced=true"));
        assert_eq!(new.matches("fan1TempSettings").count(), 1);
        // inserted keys land inside [Settings], not after [MainWindow]
        let settings_at = new.find("[Settings]").unwrap();
        assert!(new.find("fan2SpeedSettings").unwrap() > settings_at);
    }

    #[test]
    fn etc_default_detection() {
        let c = Curve::new([50, 56, 62, 68, 74, 78], [0, 30, 50, 70, 90, 100, 150]);
        let wanted = etc_default_wanted(&c, &c, true);
        let same = "SHIFT_MODE=turbo\nFAN2_MIRRORS_FAN1=yes\nFAN1_TEMPS=\"50|56|62|68|74|78\"\nFAN1_SPEEDS=\"0|30|50|70|90|100|150\"\nFAN2_TEMPS=\"50|56|62|68|74|78\"\nFAN2_SPEEDS=\"0|30|50|70|90|100|150\"\n";
        assert!(!etc_default_needs_update(Some(same), &wanted));
        assert!(etc_default_needs_update(Some(&same.replace("150", "100")), &wanted));
        assert!(!etc_default_needs_update(None, &wanted));
    }

    #[test]
    fn presets_are_valid() {
        for (_, c) in PRESETS {
            assert!(c.ascending());
            assert!(c.s.iter().all(|&v| (0..=SPEED_MAX).contains(&v)));
        }
    }
}
