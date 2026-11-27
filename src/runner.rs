//! Running the workshop scripts.
//!
//! Log jobs go through util-linux `script`, which gives the child a real
//! pseudo-terminal. That matters because the scripts only colour their output
//! when `[ -t 1 ]`, Python tools stop block-buffering, and a `read -r` prompt
//! behaves exactly as in Konsole: whatever is typed into the garage's input
//! line is written to the pty, so the script sees a normal keyboard answer.

use crate::term::Term;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phone {
    Auto,
    Vivo,
    Redmi,
}

impl Phone {
    pub fn from_index(i: u8) -> Phone {
        match i {
            1 => Phone::Vivo,
            2 => Phone::Redmi,
            _ => Phone::Auto,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Phone::Auto => "Any Phone",
            Phone::Vivo => "vivo 1910",
            Phone::Redmi => "Redmi Note 8 Pro",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JobState {
    Running,
    Exited(i32),
    Failed(String),
}

/// Everything needed to start (or restart) a log job.
#[derive(Clone, Debug)]
pub struct Spec {
    pub title: String,
    pub glyph: char,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub phone: Option<Phone>,
    pub cols: u16,
}

pub struct Job {
    pub spec: Spec,
    pub term: Arc<Mutex<Term>>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    pub started: f64,
    pub ended: Option<f64>,
    pub state: JobState,
    stop_at: Option<f64>,
    killed: u8,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// PATH with ~/.local/bin and /usr/local/bin in front: a launcher started from
/// the Plasma menu does not necessarily inherit the login shell's PATH.
pub fn search_path() -> String {
    let mut parts: Vec<String> = vec![home().join(".local/bin").to_string_lossy().into(), "/usr/local/bin".into()];
    if let Ok(p) = std::env::var("PATH") {
        for d in p.split(':') {
            if !d.is_empty() && !parts.iter().any(|x| x == d) {
                parts.push(d.to_string());
            }
        }
    }
    for d in ["/usr/bin", "/bin"] {
        if !parts.iter().any(|x| x == d) {
            parts.push(d.to_string());
        }
    }
    parts.join(":")
}

/// Is `prog` an executable on our PATH?
pub fn which(prog: &str) -> bool {
    if prog.is_empty() {
        return true;
    }
    let ok = |p: &std::path::Path| p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false);
    if prog.contains('/') {
        return ok(std::path::Path::new(prog));
    }
    search_path().split(':').any(|d| ok(&std::path::Path::new(d).join(prog)))
}

/// Single-quote for bash, only when needed.
pub fn shq(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=+@%,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// Bash snippet that resolves the picked phone to an adb serial in $S.
fn phone_snippet(p: Phone) -> &'static str {
    match p {
        Phone::Auto => "",
        Phone::Vivo => "S=$(adb devices -l 2>/dev/null | awk -v want=\"$GARAGE_VIVO_SERIAL\" '$2==\"device\" && ((want != \"\" && $1 == want) || /model:vivo/) {print $1; exit}')",
        Phone::Redmi => "S=$(adb devices -l 2>/dev/null | awk '$2==\"device\" && /device:begonia/ {print $1; exit}')",
    }
}

fn base_env(cmd: &mut Command, extra: &[(String, String)]) {
    cmd.env("PATH", search_path())
        .env("TERM", "xterm-256color")
        .env("SHELL", "/bin/bash")
        .env("PYTHONUNBUFFERED", "1")
        .env("FORCE_COLOR", "1")
        .env("SYSTEMD_PAGER", "cat")
        .env("PAGER", "cat");
    for (k, v) in extra {
        cmd.env(k, v);
    }
}

impl Job {
    pub fn start(spec: Spec, now: f64) -> Job {
        let term = Arc::new(Mutex::new(Term::new()));
        let mut job = Job { spec, term, child: None, stdin: None, started: now, ended: None, state: JobState::Running, stop_at: None, killed: 0 };
        job.launch();
        job
    }

    fn launch(&mut self) {
        let s = &self.spec;
        let quoted: Vec<String> = s.argv.iter().map(|a| shq(a)).collect();
        let shown = quoted.join(" ");
        let body = match s.phone {
            Some(p) if p != Phone::Auto && !quoted.is_empty() => {
                let rest = quoted[1..].join(" ");
                format!(
                    "{snip}; if [ -n \"$S\" ]; then printf '\\e[36m› using %s\\e[0m\\n' \"$S\"; exec {prog} -s \"$S\" {rest}; else printf '\\e[33m› no {name} visible to adb, running on whatever is connected\\e[0m\\n'; exec {shown}; fi",
                    snip = phone_snippet(p),
                    prog = quoted[0],
                    name = p.label(),
                )
            }
            _ => format!("exec {shown}"),
        };
        let line = format!("stty cols {} rows 40 2>/dev/null; {}", s.cols, body);
        {
            let mut t = self.term.lock().unwrap();
            t.note(&format!("$ {shown}"), [0x8c, 0x95, 0x99]);
        }
        let mut cmd = Command::new("script");
        cmd.args(["-qfec", &line, "/dev/null"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).current_dir(home()).process_group(0);
        base_env(&mut cmd, &s.env);
        match cmd.spawn() {
            Ok(mut child) => {
                for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>)]
                    .into_iter()
                    .flatten()
                {
                    let term = self.term.clone();
                    std::thread::spawn(move || {
                        let mut pipe = pipe;
                        let mut buf = [0u8; 8192];
                        loop {
                            match pipe.read(&mut buf) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => term.lock().unwrap().feed(&buf[..n]),
                            }
                        }
                    });
                }
                self.stdin = child.stdin.take();
                self.child = Some(child);
            }
            Err(e) => {
                self.state = JobState::Failed(format!("could not start: {e}"));
                self.term.lock().unwrap().note(&format!("could not start `script`: {e}"), [0xff, 0x5c, 0x3d]);
            }
        }
    }

    pub fn stopped_by_user(&self) -> bool {
        self.stop_at.is_some()
    }

    pub fn running(&self) -> bool {
        self.state == JobState::Running
    }

    /// Call every frame. Returns true on the frame the job finishes.
    pub fn poll(&mut self, now: f64) -> bool {
        if !self.running() {
            return false;
        }
        let Some(child) = self.child.as_mut() else {
            self.ended = Some(now);
            return true;
        };
        // escalate a stop request if the script ignores Ctrl+C
        if let Some(t0) = self.stop_at {
            let pid = child.id() as i32;
            if now - t0 > 1.5 && self.killed == 0 {
                let _ = Command::new("kill").args(["-TERM", &format!("-{pid}")]).status();
                self.killed = 1;
            } else if now - t0 > 4.0 && self.killed == 1 {
                let _ = Command::new("kill").args(["-KILL", &format!("-{pid}")]).status();
                self.killed = 2;
            }
        }
        match child.try_wait() {
            Ok(Some(st)) => {
                let code = st.code().unwrap_or(-1);
                self.state = JobState::Exited(code);
                self.ended = Some(now);
                self.stdin = None;
                true
            }
            Ok(None) => false,
            Err(e) => {
                self.state = JobState::Failed(e.to_string());
                self.ended = Some(now);
                true
            }
        }
    }

    /// Type a line into the job (the pty echoes it back into the log).
    pub fn send_line(&mut self, text: &str) {
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
        }
    }

    /// Ctrl+C through the pty, then TERM/KILL if it does not stop.
    pub fn stop(&mut self, now: f64) {
        if !self.running() {
            return;
        }
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = stdin.write_all(&[0x03]);
            let _ = stdin.flush();
        }
        if self.stop_at.is_none() {
            self.stop_at = Some(now);
            self.term.lock().unwrap().note("── stop requested (Ctrl+C) ──", [0xf5, 0xb8, 0x3d]);
        }
    }

    pub fn elapsed(&self, now: f64) -> f64 {
        self.ended.unwrap_or(now) - self.started
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Ok(None) = child.try_wait() {
                let _ = Command::new("kill").args(["-TERM", &format!("-{}", child.id())]).status();
            }
            // reap in the background so no zombie is left behind
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

/// Start a detached GUI program.
pub fn launch(argv: &[String], env: &[(String, String)]) -> Result<(), String> {
    let (prog, rest) = argv.split_first().ok_or("empty command")?;
    let mut cmd = Command::new(prog);
    cmd.args(rest).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).current_dir(home()).process_group(0);
    base_env(&mut cmd, env);
    cmd.env_remove("FORCE_COLOR");
    let mut child = cmd.spawn().map_err(|e| format!("{prog}: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Run a command in a Konsole window that stays open until Enter is pressed.
/// The bash script Konsole runs: banner, the command, then wait for Enter.
pub fn terminal_script(title: &str, argv: &[String]) -> String {
    let shown: Vec<String> = argv.iter().map(|a| shq(a)).collect();
    format!(
        "printf '\\e[1;32m▶ %s\\e[0m\\n\\n' {t}; {cmd}; rc=$?; echo; printf '\\e[1;32m── finished (exit %s), press Enter to close ──\\e[0m' $rc; read -r _",
        t = shq(title),
        cmd = shown.join(" ")
    )
}

pub fn in_terminal(title: &str, argv: &[String], env: &[(String, String)]) -> Result<(), String> {
    let script = terminal_script(title, argv);
    let tabtitle = format!("tabtitle={title}");
    let mut cmd = Command::new("konsole");
    cmd.args(["-p", &tabtitle, "-e", "bash", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir(home())
        .process_group(0);
    base_env(&mut cmd, env);
    cmd.env_remove("FORCE_COLOR").env_remove("PAGER").env_remove("SYSTEMD_PAGER");
    let mut child = cmd.spawn().map_err(|e| format!("konsole: {e}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// KDE file picker in a thread. None = cancelled.
pub fn pick_files(multiple: bool) -> Receiver<Option<Vec<String>>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut cmd = Command::new("kdialog");
        cmd.arg("--title").arg("Tiramisu Garage").arg("--getopenfilename").arg(home());
        if multiple {
            cmd.args(["--multiple", "--separate-output"]);
        }
        let out = cmd.stderr(Stdio::null()).output();
        let res = match out {
            Ok(o) if o.status.success() => {
                let files: Vec<String> = String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.is_empty()).map(String::from).collect();
                if files.is_empty() { None } else { Some(files) }
            }
            _ => None,
        };
        let _ = tx.send(res);
    });
    rx
}

/// Fill the {} / {*} / {files} placeholders. Returns None if an answer that
/// may not be empty is empty.
pub fn expand(argv: &[&str], text: Option<&str>, files: Option<&[String]>) -> Vec<String> {
    let mut out = Vec::new();
    for a in argv {
        match *a {
            "{}" => {
                if let Some(t) = text {
                    let t = t.trim();
                    if !t.is_empty() {
                        out.push(t.to_string());
                    }
                }
            }
            "{*}" => {
                if let Some(t) = text {
                    out.extend(t.split_whitespace().map(String::from));
                }
            }
            "{files}" => {
                if let Some(f) = files {
                    out.extend(f.iter().cloned());
                }
            }
            s => out.push(s.to_string()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(shq("check"), "check");
        assert_eq!(shq("a b"), "'a b'");
        assert_eq!(shq("it's"), r"'it'\''s'");
        assert_eq!(shq(""), "''");
    }

    #[test]
    fn terminal_script_is_valid_bash() {
        let argv: Vec<String> = ["safe-update", "it's a test", "a b"].iter().map(|s| s.to_string()).collect();
        let script = terminal_script("Safe Update · Full Guided Update", &argv);
        let st = std::process::Command::new("bash").args(["-n", "-c", &script]).status().unwrap();
        assert!(st.success(), "{script}");
        // and it really runs the command with its arguments intact (Enter supplied on stdin)
        let argv: Vec<String> = ["printf", "<%s>", "it's", "a b"].iter().map(|s| s.to_string()).collect();
        let out = std::process::Command::new("bash")
            .args(["-c", &terminal_script("t", &argv)])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                c.stdin.take().unwrap().write_all(b"\n")?;
                c.wait_with_output()
            })
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("<it's><a b>"), "{text}");
        assert!(text.contains("finished (exit 0)"), "{text}");
    }

    #[test]
    fn placeholders() {
        assert_eq!(expand(&["x", "{}"], Some("  "), None), vec!["x"]);
        assert_eq!(expand(&["x", "pair", "{*}"], Some("1.2.3.4:5 999"), None), vec!["x", "pair", "1.2.3.4:5", "999"]);
        assert_eq!(expand(&["x", "--since", "{}"], Some("2026-10-19 08:00"), None), vec!["x", "--since", "2026-10-19 08:00"]);
        let f = vec!["/a b.mp4".to_string()];
        assert_eq!(expand(&["r", "{files}"], None, Some(&f)), vec!["r", "/a b.mp4"]);
    }
}
