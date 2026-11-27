//! Every workshop script the garage knows about, and each one's sub-commands.
//!
//! To add a tool: append a `Tool` to `catalog()`. Each `Cmd` says how it runs:
//!   Run::Log       inside the garage, output streamed into the log screen
//!                  (it gets a real pseudo-terminal, so colours and y/N prompts work)
//!   Run::Terminal  in a Konsole window (anything that needs sudo or is a full TUI)
//!   Run::Launch    a GUI program, started detached
//!   Run::Native    a screen built into the garage itself
//! In `argv`, "{}" is replaced by the text typed into the Ask dialog as one
//! argument, "{*}" by that text split on spaces, and "{files}" by picked files.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Native {
    Dyno,
    Tuner,
    Options,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Run {
    Log,
    Terminal,
    Launch,
    Native(Native),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ask {
    None,
    /// prompt, placeholder, may the answer be empty?
    Text(&'static str, &'static str, bool),
    /// pick files with the KDE file dialog; true = several
    Files(bool),
}

#[derive(Clone, Debug)]
pub struct Cmd {
    pub glyph: char,
    pub label: &'static str,
    pub desc: &'static str,
    pub argv: &'static [&'static str],
    pub run: Run,
    /// Ask "Would you like to …?" first.
    pub confirm: Option<&'static str>,
    pub ask: Ask,
    /// Honour the phone picker (adds -s SERIAL).
    pub phone: bool,
    /// Extra environment for this command.
    pub env: &'static [(&'static str, &'static str)],
}

#[derive(Clone, Debug)]
pub struct Tool {
    pub glyph: char,
    pub name: &'static str,
    pub blurb: &'static str,
    /// Program that must exist for this tool to work (checked at start-up).
    pub needs: &'static str,
    pub cmds: Vec<Cmd>,
    /// Show the phone picker in this tool's header.
    pub phone_picker: bool,
}

const fn c(glyph: char, label: &'static str, desc: &'static str, argv: &'static [&'static str], run: Run) -> Cmd {
    Cmd { glyph, label, desc, argv, run, confirm: None, ask: Ask::None, phone: false, env: &[] }
}

impl Cmd {
    const fn confirm(mut self, q: &'static str) -> Self {
        self.confirm = Some(q);
        self
    }
    const fn ask(mut self, a: Ask) -> Self {
        self.ask = a;
        self
    }
    const fn phone(mut self) -> Self {
        self.phone = true;
        self
    }
    const fn env(mut self, e: &'static [(&'static str, &'static str)]) -> Self {
        self.env = e;
        self
    }
}

// Material Design glyphs (Nerd Font code points), named for readability.
mod g {
    pub const UPDATE: char = '\u{F06B0}';
    pub const ARCH: char = '\u{F08C7}';
    pub const CHECK_CIRCLE: char = '\u{F05E0}';
    pub const SHIELD_CHECK: char = '\u{F0565}';
    pub const CHIP: char = '\u{F061A}';
    pub const RESTART: char = '\u{F0709}';
    pub const FILE_SEARCH: char = '\u{F0C7C}';
    pub const LIFEBUOY: char = '\u{F087E}';
    pub const CLIPBOARD: char = '\u{F014D}';
    pub const TEXT_BOX: char = '\u{F021A}';
    pub const SPEEDOMETER: char = '\u{F04C5}';
    pub const FAN: char = '\u{F0210}';
    pub const TUNE: char = '\u{F066A}';
    pub const FAN_AUTO: char = '\u{F171D}';
    pub const FIRE: char = '\u{F0238}';
    pub const CPU: char = '\u{F0EE0}';
    pub const ENGINE: char = '\u{F01FA}';
    pub const CELL_COG: char = '\u{F0951}';
    pub const CELL: char = '\u{F011C}';
    pub const CELL_CHECK: char = '\u{F17FD}';
    pub const CELL_LINK: char = '\u{F0121}';
    pub const CELL_WIRELESS: char = '\u{F0815}';
    pub const WIFI: char = '\u{F05A9}';
    pub const WIFI_COG: char = '\u{F16BE}';
    pub const KEY: char = '\u{F0306}';
    pub const BUG: char = '\u{F00E4}';
    pub const DATABASE: char = '\u{F01BC}';
    pub const BATTERY_HEART: char = '\u{F1211}';
    pub const BATTERY_CHECK: char = '\u{F17E2}';
    pub const HEADPHONES: char = '\u{F02CB}';
    pub const CAMERA: char = '\u{F0100}';
    pub const ARCHIVE: char = '\u{F003C}';
    pub const BROOM: char = '\u{F0665}'; // spray can
    pub const ALERT: char = '\u{F0026}';
    pub const STETHOSCOPE: char = '\u{F04D9}';
    pub const MEDICAL_BAG: char = '\u{F06EF}';
    pub const POWER: char = '\u{F0425}';
    pub const SWAP: char = '\u{F04E1}';
    pub const FLASH: char = '\u{F0241}';
    pub const MONITOR: char = '\u{F0379}';
    pub const ACCESS_POINT: char = '\u{F0003}';
    pub const ROUTER: char = '\u{F0469}';
    pub const CLOSE_CIRCLE: char = '\u{F0159}';
    pub const INFO: char = '\u{F02FC}';
    pub const HARDDISK: char = '\u{F02CA}';
    pub const PULSE: char = '\u{F0430}';
    pub const ROBOT: char = '\u{F06A9}';
    pub const CHAT: char = '\u{F0B79}';
    pub const SERVER: char = '\u{F048B}';
    pub const TIMER: char = '\u{F13AB}';
    pub const MOVIE: char = '\u{F0FCE}';
    pub const MUSIC: char = '\u{F0384}';
    pub const SPEAKER: char = '\u{F0D38}';
    pub const PALETTE: char = '\u{F03D8}';
    pub const PALETTE_ADV: char = '\u{F03D9}';
    pub const BRUSH: char = '\u{F00E3}';
    pub const FORMAT_PAINT: char = '\u{F027C}';
    pub const WALLPAPER: char = '\u{F0E09}';
    pub const LIST: char = '\u{F0279}';
    pub const COG: char = '\u{F0493}';
    pub const PLAY: char = '\u{F040A}';
    pub const STOP: char = '\u{F04DB}';
}

pub fn catalog() -> Vec<Tool> {
    use Ask::*;
    use Run::*;
    vec![
        Tool {
            glyph: g::UPDATE,
            name: "Safe Update",
            blurb: "Guarded arch-update with pre-flight and post-checks",
            needs: "safe-update",
            phone_picker: false,
            cmds: vec![
                c(g::SHIELD_CHECK, "Pre-Flight Check", "Is it safe to start updating? Changes nothing.", &["safe-update", "check"], Log),
                c(g::ARCH, "Full Guided Update", "check, then arch-update (logged), then post-check. Opens Konsole: it needs your password.", &["safe-update"], Terminal)
                    .confirm("Start the full guided update in Konsole?"),
                c(g::CHECK_CIRCLE, "Post-Update Check", "Explain the last update log, DKMS matrix, .pacnew files, reboot verdict.", &["safe-update", "post"], Log),
                c(g::CHIP, "Fix DKMS", "Rebuild any DKMS module missing for an installed kernel. Opens Konsole (root).", &["safe-update", "fix-dkms"], Terminal)
                    .confirm("Rebuild missing DKMS modules in Konsole?"),
                c(g::RESTART, "After Reboot", "Health check after booting into the updated system.", &["safe-update", "after-reboot"], Log),
                c(g::FILE_SEARCH, "Explain Log", "Translate the errors in today's update log into plain words.", &["safe-update", "explain"], Log),
                c(g::LIFEBUOY, "Rescue Guide", "What to do if the laptop won't boot or the desktop is broken.", &["safe-update", "rescue"], Log)
                    .env(&[("PAGER", "cat")]),
                c(g::TEXT_BOX, "Prompt Answers", "The cheat sheet: what to answer at each arch-update prompt.", &["safe-update", "cheat"], Log),
                c(g::CLIPBOARD, "Report", "The last reports as one block, ready to paste into a chat.", &["safe-update", "report"], Log),
            ],
        },
        Tool {
            glyph: g::SPEEDOMETER,
            name: "Performance",
            blurb: "MSI Thin 15 fan, EC and CPU tuning",
            needs: "busctl",
            phone_picker: false,
            cmds: vec![
                c(g::SPEEDOMETER, "Live Dyno", "Fan RPM, temperatures and EC modes, live (read-only, 1 Hz).", &[], Native(self::Native::Dyno)),
                c(g::TUNE, "Fan Curve Tuner", "Edit both EC fan curves on the ECU-tuning chart; Apply syncs the EC, MControlCenter.conf and /etc/default.", &[], Native(self::Native::Tuner)),
                c(g::FAN, "MControlCenter", "MSI's control centre app.", &["mcontrolcenter"], Launch),
                c(g::FIRE, "Fan-Kick Status", "The Cooler Boost kicker: is it running, and what has it done lately?", &["systemctl", "status", "fan-kick.service", "--no-pager", "-n", "20"], Log),
                c(g::CPU, "CPU Tuning Status", "Boot-time CPU tuning: unit status plus the live governor and EPP.", &["bash", "-c", "systemctl status cpu-tuning.service cpu-tuning.timer --no-pager -n 6; echo; for f in scaling_governor energy_performance_preference scaling_max_freq; do printf '%-34s %s\\n' \"$f\" \"$(cat /sys/devices/system/cpu/cpu0/cpufreq/$f)\"; done; printf '%-34s %s\\n' no_turbo \"$(cat /sys/devices/system/cpu/intel_pstate/no_turbo)\""], Log),
                c(g::FAN_AUTO, "EC Profile Status", "msi-ec-profile: last run, plus the live shift/fan/boost modes.", &["bash", "-c", "systemctl status msi-ec-profile.service --no-pager -n 8; echo; cd /sys/devices/platform/msi-ec && for f in shift_mode fan_mode cooler_boost super_battery fw_version; do printf '%-14s %s\\n' \"$f\" \"$(cat $f)\"; done"], Log),
                c(g::ENGINE, "Re-apply EC Profile", "Restart msi-ec-profile.service: shift mode, fan mode and the saved curve.", &["pkexec", "systemctl", "restart", "msi-ec-profile.service"], Log)
                    .confirm("Re-apply the EC profile now? (asks for your password)"),
            ],
        },
        Tool {
            glyph: g::CELL_COG,
            name: "Phone Doctor",
            blurb: "Read-only health checks and evidence capture over adb",
            needs: "phone-doctor",
            phone_picker: true,
            cmds: vec![
                c(g::STETHOSCOPE, "Health Check", "Battery, storage, RAM, thermals, crashes, Wi-Fi of the connected phone.", &["phone-doctor"], Log).phone(),
                c(g::CELL_CHECK, "Devices", "Which phones adb can see, and what to do if none.", &["phone-doctor", "devices"], Log),
                c(g::WIFI, "Wi-Fi: vivo", "Connect the vivo over Wireless debugging (remembers the address).", &["phone-doctor", "wifi", "vivo"], Log),
                c(g::WIFI, "Wi-Fi: Redmi", "Connect the Redmi over Wireless debugging.", &["phone-doctor", "wifi", "redmi"], Log),
                c(g::WIFI_COG, "Wi-Fi: Other", "Connect any phone over Wi-Fi by address.", &["phone-doctor", "wifi", "{}"], Log)
                    .ask(Text("Phone address", "192.168.1.50:5555", false)),
                c(g::KEY, "Pair", "First-time Wi-Fi pairing (Pair device with pairing code).", &["phone-doctor", "pair", "{*}"], Log)
                    .ask(Text("Pairing address and code", "192.168.1.50:37123 123456", false)),
                c(g::BUG, "Crashes", "Which apps crashed recently, with the first error lines.", &["phone-doctor", "crashes"], Log).phone(),
                c(g::DATABASE, "Storage", "What is filling the phone's storage (takes about a minute).", &["phone-doctor", "storage"], Log).phone(),
                c(g::BATTERY_HEART, "Battery", "Battery details (Redmi: the full root report).", &["phone-doctor", "battery"], Log).phone(),
                c(g::HEADPHONES, "Audio Capture", "Audio-routing snapshot. Run it right after a bad WhatsApp call.", &["phone-doctor", "audio"], Log).phone(),
                c(g::CAMERA, "Evidence Capture", "Save a full evidence bundle (logs, dumpsys) to ~/phone-reports.", &["phone-doctor", "capture", "{}"], Log)
                    .phone()
                    .ask(Text("A short note for the bundle (optional)", "e.g. lag after unlocking", true)),
                c(g::ARCHIVE, "Bug Report", "Full Android bugreport .zip, to open at ui.perfetto.dev.", &["phone-doctor", "bugreport"], Log).phone(),
                c(g::BROOM, "Clear Caches", "Empty every app cache. Photos and chats are untouched; the script asks first too.", &["phone-doctor", "clear-cache"], Log)
                    .phone()
                    .confirm("Empty all app caches on the phone?"),
            ],
        },
        Tool {
            glyph: g::BATTERY_HEART,
            name: "Battery Check",
            blurb: "Redmi Note 8 Pro battery health and charge history",
            needs: "battcheck",
            phone_picker: false,
            cmds: vec![
                c(g::BATTERY_HEART, "Full Report", "Health, charger, charge history and usage; adds a line to the trend log.", &["battcheck"], Log),
                c(g::WIFI, "Connect & Report", "Connect to the Redmi's wireless-debugging port first, then report.", &["battcheck", "{}"], Log)
                    .ask(Text("Address (short form works)", ".1.50:46007", false)),
                c(g::BATTERY_CHECK, "Quick Health", "Read-only battery health check (root).", &["phone-battery-check"], Log),
            ],
        },
        Tool {
            glyph: g::LIFEBUOY,
            name: "Begonia Rescue",
            blurb: "Fix the scrambled-data-files problem on the Redmi",
            needs: "begonia-rescue",
            phone_picker: false,
            cmds: vec![
                c(g::STETHOSCOPE, "Check", "Phone booted: read-only health and corruption signs.", &["begonia-rescue", "check"], Log),
                c(g::ALERT, "Crash", "Why is Android crash-looping? (newest system_server crash)", &["begonia-rescue", "crash"], Log),
                c(g::FILE_SEARCH, "Scan", "List garbled system files.", &["begonia-rescue", "scan"], Log),
                c(g::TIMER, "Scan Since", "Also list data files written since a moment in time.", &["begonia-rescue", "scan", "--since", "{}"], Log)
                    .ask(Text("Since when?", "2026-10-19 08:00", false)),
                c(g::MEDICAL_BAG, "Fix", "Quarantine garbled XML/ABX, restore module.prop, clean adb_keys. Nothing is deleted.", &["begonia-rescue", "fix"], Log)
                    .confirm("Quarantine the garbled files? (the script asks again)"),
                c(g::SWAP, "Swap Status", "Is the 6 GB swapfile on?", &["begonia-rescue", "swap", "status"], Log),
                c(g::SWAP, "Swap Off", "Kill switch: disable the swapfile (also swapoff now).", &["begonia-rescue", "swap", "off"], Log)
                    .confirm("Turn the swapfile off?"),
                c(g::SWAP, "Swap On", "Re-enable the swapfile at boot.", &["begonia-rescue", "swap", "on"], Log)
                    .confirm("Turn the swapfile back on?"),
                c(g::HARDDISK, "fsck", "Recovery only: unmount /data and repair f2fs.", &["begonia-rescue", "fsck"], Log)
                    .confirm("Run fsck.f2fs on /data? (recovery mode only)"),
                c(g::POWER, "Reboot Phone", "adb reboot.", &["begonia-rescue", "reboot"], Log).confirm("Reboot the Redmi?"),
            ],
        },
        Tool {
            glyph: g::CELL_LINK,
            name: "Phone Link",
            blurb: "Mirror, desktop and Wi-Fi adb for both phones",
            needs: "vivo-up",
            phone_picker: false,
            cmds: vec![
                c(g::FLASH, "vivo Up", "After the vivo boots: Shizuku, scrcpy mirror, saverguard watcher.", &["vivo-up"], Log),
                c(g::MONITOR, "Redmi Desktop", "Redmi as a desktop on a scrcpy virtual display.", &["phone-desktop"], Launch),
                c(g::MONITOR, "Redmi Freeform", "Same, with floating windows.", &["phone-desktop", "--freeform"], Launch),
                c(g::CLOSE_CIRCLE, "Dock Off", "Switch Smart Dock off if the cable was pulled mid-session.", &["phone-desktop", "--dock-off"], Log),
                c(g::CELL_WIRELESS, "Redmi Wi-Fi", "Reconnect adb over Wi-Fi (last address, else scan).", &["phone-wifi"], Log),
                c(g::WIFI_COG, "Redmi Wi-Fi To", "Connect adb to a given address.", &["phone-wifi", "{}"], Log)
                    .ask(Text("Redmi address", "192.168.1.60:41234", false)),
                c(g::CLOSE_CIRCLE, "Redmi Wi-Fi Off", "Disconnect the Wi-Fi adb link.", &["phone-wifi", "off"], Log),
            ],
        },
        Tool {
            glyph: g::ACCESS_POINT,
            name: "Hotspot",
            blurb: "5 GHz / 2.4 GHz hotspot on the AX211",
            needs: "hotspot-band",
            phone_picker: false,
            cmds: vec![
                c(g::INFO, "Status", "Is the hotspot up, on which band and channel?", &["hotspot-band", "status"], Log),
                c(g::ROUTER, "5 GHz", "Start the hotspot on 5 GHz (asks for your password).", &["hotspot-band", "5"], Log),
                c(g::ROUTER, "2.4 GHz", "Start the hotspot on 2.4 GHz (asks for your password).", &["hotspot-band", "2.4"], Log),
                c(g::CLOSE_CIRCLE, "Off", "Stop the hotspot.", &["hotspot-band", "off"], Log).confirm("Switch the hotspot off?"),
            ],
        },
        Tool {
            glyph: g::HARDDISK,
            name: "Patient Bay",
            blurb: "Progress of every customer drive job",
            needs: "rescue-status",
            phone_picker: false,
            cmds: vec![
                c(g::HARDDISK, "Snapshot", "ddrescue images, restores and verifies: one snapshot.", &["rescue-status"], Log),
                c(g::PULSE, "Live View", "Same, refreshing every 2 s. Esc stops it.", &["rescue-status", "-w"], Log),
                c(g::HARDDISK, "Bench Two", "Just the second bench: one customer's HDD restore and flash-drive backup.", &["dini-status"], Log),
            ],
        },
        Tool {
            glyph: g::ROBOT,
            name: "Local AI",
            blurb: "gpt-oss-20b on llama.cpp, tuned for this laptop",
            needs: "llama-ai",
            phone_picker: false,
            cmds: vec![
                c(g::CHAT, "Chat", "Chat in the terminal (opens Konsole).", &["llama-ai"], Terminal),
                c(g::SERVER, "Server", "OpenAI-compatible API + web UI on 127.0.0.1:8080 (opens Konsole).", &["llama-ai", "server"], Terminal),
                c(g::TIMER, "Bench", "Quick speed test (llama-bench).", &["llama-ai", "bench"], Log),
            ],
        },
        Tool {
            glyph: g::MOVIE,
            name: "Media Lab",
            blurb: "Resolve-friendly transcodes and sound themes",
            needs: "resolve-prep",
            phone_picker: false,
            cmds: vec![
                c(g::MOVIE, "Resolve Prep", "Pick videos; converts them to AV1 + Opus that free Resolve can open.", &["resolve-prep", "{files}"], Log)
                    .ask(Files(true)),
                c(g::MOVIE, "Resolve Prep DNxHR", "Same, but DNxHR + PCM: snappiest to edit, enormous files.", &["resolve-prep", "-c", "dnxhr", "{files}"], Log)
                    .ask(Files(true)),
                c(g::LIST, "Sound Themes", "List the converted Longhorn sound themes.", &["longhorn-sounds", "list"], Log),
                c(g::MUSIC, "Use Sound Theme", "Switch Plasma to one of them.", &["longhorn-sounds", "use", "{}"], Log)
                    .ask(Text("Theme name (see Sound Themes)", "", false)),
                c(g::SPEAKER, "Audio Services", "EasyEffects / output-switch user services.", &["bash", "-c", "systemctl --user --no-pager list-units --all '*audio*' '*easyeffects*' '*bass*' '*sound*' 2>/dev/null | head -30"], Log),
            ],
        },
        Tool {
            glyph: g::PALETTE,
            name: "Paint Shop",
            blurb: "The glass desktop theme and window decorations",
            needs: "glass-theme-build",
            phone_picker: false,
            cmds: vec![
                c(g::PALETTE, "Rebuild Glass Theme", "Rebuild the translucent Plasma theme from the system one.", &["glass-theme-build"], Log)
                    .confirm("Rebuild the glass Plasma theme?"),
                c(g::FORMAT_PAINT, "Rebuild Glass QML", "Rebuild the outlined-text QML overlay for plasmashell.", &["glass-qml-build"], Log)
                    .confirm("Rebuild the QML overlay?"),
                c(g::BRUSH, "Recolor Kvantum", "Map the Kvantum theme onto the current KDE colours.", &["kvantum-recolor"], Log)
                    .confirm("Recolour the Kvantum theme?"),
                c(g::LIST, "Klassy Presets", "List the Klassy window-decoration presets.", &["klassy-apply-preset", "--list"], Log),
                c(g::PALETTE_ADV, "Apply Klassy Preset", "Apply one preset to klassyrc.", &["klassy-apply-preset", "{}"], Log)
                    .ask(Text("Preset name", "Defenestrated_7", false)),
                c(g::WALLPAPER, "Login Wallpaper", "Pick an image for the login screen (asks for your password).", &["set-login-wallpaper-gui", "{files}"], Launch)
                    .ask(Files(false)),
            ],
        },
        Tool {
            glyph: g::COG,
            name: "Options",
            blurb: "Garage settings",
            needs: "",
            phone_picker: false,
            cmds: vec![c(g::COG, "Options", "", &[], Native(self::Native::Options))],
        },
    ]
}

/// Run kinds as short header tags.
pub fn tags(cmd: &Cmd) -> Vec<&'static str> {
    let mut t = Vec::new();
    match cmd.run {
        Run::Terminal => t.push("KONSOLE"),
        Run::Launch => t.push("APP"),
        Run::Native(_) => t.push("BUILT-IN"),
        Run::Log => {}
    }
    if cmd.confirm.is_some() {
        t.push("ASKS FIRST");
    }
    if cmd.phone {
        t.push("PHONE PICKER");
    }
    t
}

#[allow(dead_code)]
pub const PLAY: char = g::PLAY;
#[allow(dead_code)]
pub const STOP: char = g::STOP;
