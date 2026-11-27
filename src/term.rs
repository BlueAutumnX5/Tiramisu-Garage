//! A small line-oriented terminal model for job output.
//!
//! The scripts run under a pseudo-terminal (see runner.rs), so they emit what
//! they would to Konsole: SGR colours, `\r` progress lines, and in the case of
//! `rescue-status -w` a full redraw with cursor-home + clear-to-end. This
//! keeps just enough terminal state (row, column, style) to show all of that
//! correctly in the log panel, without being a full VT100.

#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub struct Style {
    /// resolved RGB, None = default text colour
    pub fg: Option<[u8; 3]>,
    pub bold: bool,
    pub dim: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub ch: char,
    pub st: Style,
}

enum Parse {
    Normal,
    Esc,
    Csi(String),
    Osc,
    OscEsc,
    Charset,
}

pub struct Term {
    pub lines: Vec<Vec<Cell>>,
    row: usize,
    col: usize,
    screen_top: usize,
    st: Style,
    state: Parse,
    pending: Vec<u8>,
    /// bumped on every change, so the UI can tell when to auto-scroll
    pub version: u64,
    max_lines: usize,
}

/// The 16 ANSI colours, retuned to read well on the garage's dark glass.
const PALETTE: [[u8; 3]; 16] = [
    [0x6b, 0x73, 0x77], // black -> slate (still visible)
    [0xff, 0x5c, 0x3d], // red
    [0xc6, 0xe3, 0x51], // green -> the menu lime
    [0xf5, 0xb8, 0x3d], // yellow -> amber
    [0x6a, 0xa7, 0xff], // blue
    [0xe0, 0x7a, 0xd8], // magenta
    [0x54, 0xb3, 0xb0], // cyan -> teal
    [0xe6, 0xe9, 0xea], // white
    [0x8c, 0x95, 0x99],
    [0xff, 0x7d, 0x63],
    [0xd8, 0xf2, 0x6e],
    [0xff, 0xd0, 0x66],
    [0x8f, 0xbf, 0xff],
    [0xf0, 0x9a, 0xe8],
    [0x7f, 0xd6, 0xd2],
    [0xff, 0xff, 0xff],
];

fn xterm256(n: u8) -> [u8; 3] {
    match n {
        0..=15 => PALETTE[n as usize],
        16..=231 => {
            let n = n - 16;
            let lv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            [lv(n / 36), lv((n / 6) % 6), lv(n % 6)]
        }
        _ => {
            let g = 8 + (n - 232) * 10;
            [g, g, g]
        }
    }
}

impl Term {
    pub fn new() -> Self {
        Term {
            lines: vec![Vec::new()],
            row: 0,
            col: 0,
            screen_top: 0,
            st: Style::default(),
            state: Parse::Normal,
            pending: Vec::new(),
            version: 0,
            max_lines: 6000,
        }
    }

    /// Append a line in a given colour (used for the garage's own notes).
    pub fn note(&mut self, text: &str, rgb: [u8; 3]) {
        if !self.lines[self.row].is_empty() || self.col > 0 {
            self.newline();
        }
        let saved = self.st;
        self.st = Style { fg: Some(rgb), bold: true, dim: false };
        for ch in text.chars() {
            self.put(ch);
        }
        self.st = saved;
        self.newline();
        self.version += 1;
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let buf = std::mem::take(&mut self.pending);
        let mut i = 0;
        while i < buf.len() {
            match std::str::from_utf8(&buf[i..]) {
                Ok(s) => {
                    for ch in s.chars() {
                        self.ch(ch);
                    }
                    i = buf.len();
                }
                Err(e) => {
                    let good = e.valid_up_to();
                    let s = unsafe { std::str::from_utf8_unchecked(&buf[i..i + good]) };
                    for ch in s.chars() {
                        self.ch(ch);
                    }
                    i += good;
                    match e.error_len() {
                        Some(n) => {
                            self.ch('\u{FFFD}');
                            i += n;
                        }
                        None => {
                            // incomplete sequence at the end: keep for next time
                            self.pending.extend_from_slice(&buf[i..]);
                            i = buf.len();
                        }
                    }
                }
            }
        }
        self.version += 1;
    }

    fn ch(&mut self, ch: char) {
        match std::mem::replace(&mut self.state, Parse::Normal) {
            Parse::Normal => match ch {
                '\x1b' => self.state = Parse::Esc,
                '\r' => self.col = 0,
                '\n' => self.newline(),
                '\x08' => self.col = self.col.saturating_sub(1),
                '\t' => {
                    let next = (self.col / 8 + 1) * 8;
                    while self.col < next {
                        self.put(' ');
                    }
                }
                c if (c as u32) < 0x20 || c == '\x7f' => {}
                c => self.put(c),
            },
            Parse::Esc => match ch {
                '[' => self.state = Parse::Csi(String::new()),
                ']' => self.state = Parse::Osc,
                '(' | ')' | '*' | '+' => self.state = Parse::Charset,
                _ => {}
            },
            Parse::Csi(mut p) => {
                if ('\x40'..='\x7e').contains(&ch) {
                    self.csi(&p, ch);
                } else {
                    p.push(ch);
                    self.state = Parse::Csi(p);
                }
            }
            Parse::Osc => match ch {
                '\x07' => {}
                '\x1b' => self.state = Parse::OscEsc,
                _ => self.state = Parse::Osc,
            },
            Parse::OscEsc => {
                if ch != '\\' {
                    self.state = Parse::Osc;
                }
            }
            Parse::Charset => {}
        }
    }

    fn put(&mut self, ch: char) {
        let line = &mut self.lines[self.row];
        let cell = Cell { ch, st: self.st };
        if self.col < line.len() {
            line[self.col] = cell;
        } else {
            while line.len() < self.col {
                line.push(Cell { ch: ' ', st: Style::default() });
            }
            line.push(cell);
        }
        self.col += 1;
    }

    fn newline(&mut self) {
        self.row += 1;
        self.col = 0;
        while self.lines.len() <= self.row {
            self.lines.push(Vec::new());
        }
        if self.lines.len() > self.max_lines {
            let cut = self.lines.len() - self.max_lines + 500;
            self.lines.drain(0..cut);
            self.row = self.row.saturating_sub(cut);
            self.screen_top = self.screen_top.saturating_sub(cut);
        }
    }

    fn csi(&mut self, params: &str, fin: char) {
        let private = params.starts_with('?') || params.starts_with('>');
        if private {
            return; // mode switches (cursor hide/show, bracketed paste ...)
        }
        let nums: Vec<u32> = params.split(';').map(|s| s.parse().unwrap_or(0)).collect();
        let n1 = |d: u32| nums.first().copied().filter(|&v| v > 0).unwrap_or(d) as usize;
        match fin {
            'm' => self.sgr(&nums),
            'K' => {
                let line = &mut self.lines[self.row];
                match nums.first().copied().unwrap_or(0) {
                    0 => line.truncate(self.col),
                    1 => {
                        for c in line.iter_mut().take(self.col + 1) {
                            *c = Cell { ch: ' ', st: Style::default() };
                        }
                    }
                    _ => line.clear(),
                }
            }
            'J' => match nums.first().copied().unwrap_or(0) {
                0 => {
                    self.lines[self.row].truncate(self.col);
                    self.lines.truncate(self.row + 1);
                }
                _ => {
                    self.lines = vec![Vec::new()];
                    self.row = 0;
                    self.col = 0;
                    self.screen_top = 0;
                }
            },
            'H' | 'f' => {
                let r = n1(1) - 1;
                let c = nums.get(1).copied().filter(|&v| v > 0).unwrap_or(1) as usize - 1;
                self.row = self.screen_top + r;
                self.col = c;
                while self.lines.len() <= self.row {
                    self.lines.push(Vec::new());
                }
            }
            'A' => self.row = self.row.saturating_sub(n1(1)).max(self.screen_top),
            'B' => {
                self.row += n1(1);
                while self.lines.len() <= self.row {
                    self.lines.push(Vec::new());
                }
            }
            'C' => self.col += n1(1),
            'D' => self.col = self.col.saturating_sub(n1(1)),
            'G' => self.col = n1(1) - 1,
            _ => {}
        }
    }

    fn sgr(&mut self, nums: &[u32]) {
        let mut i = 0;
        if nums.is_empty() {
            self.st = Style::default();
            return;
        }
        while i < nums.len() {
            match nums[i] {
                0 => self.st = Style::default(),
                1 => self.st.bold = true,
                2 => self.st.dim = true,
                22 => {
                    self.st.bold = false;
                    self.st.dim = false;
                }
                30..=37 => self.st.fg = Some(PALETTE[(nums[i] - 30) as usize]),
                90..=97 => self.st.fg = Some(PALETTE[(nums[i] - 90 + 8) as usize]),
                39 => self.st.fg = None,
                38 => {
                    if nums.get(i + 1) == Some(&5) {
                        if let Some(&n) = nums.get(i + 2) {
                            self.st.fg = Some(xterm256(n.min(255) as u8));
                        }
                        i += 2;
                    } else if nums.get(i + 1) == Some(&2) {
                        let g = |k| nums.get(i + k).copied().unwrap_or(0).min(255) as u8;
                        self.st.fg = Some([g(2), g(3), g(4)]);
                        i += 4;
                    }
                }
                48 => {
                    // background colours are ignored, but their arguments must be skipped
                    if nums.get(i + 1) == Some(&5) {
                        i += 2;
                    } else if nums.get(i + 1) == Some(&2) {
                        i += 4;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// Plain text of the whole buffer (for copying).
    pub fn plain(&self) -> String {
        let mut s = String::new();
        for l in &self.lines {
            s.extend(l.iter().map(|c| c.ch));
            s.push('\n');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &Term) -> Vec<String> {
        t.lines.iter().map(|l| l.iter().map(|c| c.ch).collect::<String>()).collect()
    }

    #[test]
    fn crlf_and_colour() {
        let mut t = Term::new();
        t.feed(b"\x1b[32mOK\x1b[0m done\r\nnext");
        assert_eq!(text(&t), vec!["OK done", "next"]);
        assert_eq!(t.lines[0][0].st.fg, Some(PALETTE[2]));
        assert_eq!(t.lines[0][3].st.fg, None);
    }

    #[test]
    fn carriage_return_overwrites() {
        let mut t = Term::new();
        t.feed(b"10%\r55%\r100%\n");
        assert_eq!(text(&t)[0], "100%");
    }

    #[test]
    fn home_redraw_like_rescue_status() {
        let mut t = Term::new();
        t.feed(b"\x1b[2J\x1b[Hold line one\x1b[K\nold two\x1b[K\n\x1b[J");
        t.feed(b"\x1b[Hnew\x1b[K\nnew two\x1b[K\n\x1b[J");
        assert_eq!(text(&t), vec!["new", "new two", ""]);
    }

    #[test]
    fn split_utf8() {
        let mut t = Term::new();
        let s = "─ok".as_bytes();
        t.feed(&s[..1]);
        t.feed(&s[1..]);
        assert_eq!(text(&t)[0], "─ok");
    }
}
