//! The terminal tab's text model: PTY output in, plain lines out, plus the
//! key-to-bytes mapping for typing. A line-oriented subset of a VT100:
//! carriage return, backspace, tab, erase-in-line and clear-screen; other
//! escape sequences (colors, titles, cursor moves) are dropped.

/// Lines kept for display.
pub const MAX_LINES: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Parse {
    #[default]
    None,
    /// After ESC.
    Start,
    /// Inside `ESC [ … final`.
    Csi,
    /// Inside `ESC ] … BEL` (or `ESC \`).
    Osc,
    OscEsc,
}

#[derive(Debug, Clone)]
pub struct Screen {
    lines: Vec<Vec<char>>,
    col: usize,
    esc: Parse,
    params: String,
    /// Bytes of an incomplete UTF-8 sequence from the last chunk.
    pending: Vec<u8>,
}

impl Default for Screen {
    fn default() -> Self {
        Self {
            lines: vec![Vec::new()],
            col: 0,
            esc: Parse::None,
            params: String::new(),
            pending: Vec::new(),
        }
    }
}

impl Screen {
    /// Feeds a chunk of output.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(bytes);
        let (text, rest) = match std::str::from_utf8(&buf) {
            Ok(s) => (s.to_owned(), Vec::new()),
            Err(e) if e.error_len().is_none() => {
                let cut = e.valid_up_to();
                (
                    String::from_utf8_lossy(&buf[..cut]).into_owned(),
                    buf[cut..].to_vec(),
                )
            }
            Err(_) => (String::from_utf8_lossy(&buf).into_owned(), Vec::new()),
        };
        self.pending = rest;
        for c in text.chars() {
            self.put(c);
        }
    }

    fn line(&mut self) -> &mut Vec<char> {
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        let last = self.lines.len() - 1;
        &mut self.lines[last]
    }

    fn put(&mut self, c: char) {
        match self.esc {
            Parse::Start => {
                self.esc = match c {
                    '[' => {
                        self.params.clear();
                        Parse::Csi
                    }
                    ']' => Parse::Osc,
                    _ => Parse::None,
                };
                return;
            }
            Parse::Csi => {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    self.esc = Parse::None;
                    self.csi(c);
                } else {
                    self.params.push(c);
                }
                return;
            }
            Parse::Osc => {
                self.esc = match c {
                    '\u{7}' => Parse::None,
                    '\u{1b}' => Parse::OscEsc,
                    _ => Parse::Osc,
                };
                return;
            }
            Parse::OscEsc => {
                self.esc = Parse::None;
                return;
            }
            Parse::None => {}
        }
        match c {
            '\u{1b}' => self.esc = Parse::Start,
            '\r' => self.col = 0,
            '\n' => {
                self.lines.push(Vec::new());
                self.col = 0;
                if self.lines.len() > MAX_LINES {
                    let over = self.lines.len() - MAX_LINES;
                    self.lines.drain(..over);
                }
            }
            '\u{8}' => self.col = self.col.saturating_sub(1),
            '\t' => {
                let next = (self.col / 8 + 1) * 8;
                while self.line().len() < next {
                    self.line().push(' ');
                }
                self.col = next;
            }
            c if c.is_control() => {}
            c => {
                let col = self.col;
                let line = self.line();
                while line.len() < col {
                    line.push(' ');
                }
                if col < line.len() {
                    line[col] = c;
                } else {
                    line.push(c);
                }
                self.col += 1;
            }
        }
    }

    fn csi(&mut self, fin: char) {
        let n = self
            .params
            .trim_start_matches('?')
            .parse::<usize>()
            .unwrap_or(0);
        match fin {
            // Erase in line: 0 (default) to the end, 1 to the start, 2 all.
            'K' => {
                let col = self.col;
                let line = self.line();
                match n {
                    1 => line.iter_mut().take(col + 1).for_each(|c| *c = ' '),
                    2 => line.clear(),
                    _ => line.truncate(col),
                }
            }
            'J' if n >= 2 => {
                self.lines = vec![Vec::new()];
                self.col = 0;
            }
            'C' => self.col += n.max(1),
            'D' => self.col = self.col.saturating_sub(n.max(1)),
            _ => {}
        }
    }

    /// Everything kept, as text.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.extend(l.iter());
        }
        out
    }
}

/// Bytes a key press types into the shell; `None` leaves the key to the
/// app (Cmd shortcuts, bare modifiers).
#[must_use]
pub fn key_bytes(key: &str, ctrl: bool, alt: bool, meta: bool) -> Option<Vec<u8>> {
    if meta {
        return None;
    }
    let seq: &[u8] = match key {
        "Enter" => b"\r",
        "Backspace" => b"\x7f",
        "Tab" => b"\t",
        "Escape" => b"\x1b",
        "ArrowUp" => b"\x1b[A",
        "ArrowDown" => b"\x1b[B",
        "ArrowRight" => b"\x1b[C",
        "ArrowLeft" => b"\x1b[D",
        "Home" => b"\x1b[H",
        "End" => b"\x1b[F",
        "Delete" => b"\x1b[3~",
        _ => {
            let mut chars = key.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            if ctrl {
                return c
                    .is_ascii_alphabetic()
                    .then(|| vec![(c.to_ascii_lowercase() as u8) & 0x1f]);
            }
            let mut out = Vec::new();
            if alt {
                out.push(0x1b);
            }
            let mut b = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            return Some(out);
        }
    };
    (!ctrl || key == "Enter").then(|| seq.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(chunks: &[&[u8]]) -> String {
        let mut s = Screen::default();
        for c in chunks {
            s.feed(c);
        }
        s.text()
    }

    #[test]
    fn plain_output_and_line_editing() {
        assert_eq!(screen(&[b"$ echo hi\r\nhi\r\n$ "]), "$ echo hi\nhi\n$ ");
        assert_eq!(screen(&[b"abc\x08\x08X"]), "aXc");
        assert_eq!(screen(&[b"progress 10%\rprogress 99%"]), "progress 99%");
        assert_eq!(screen(&[b"hello\r\x1b[Kbye"]), "bye");
        assert_eq!(screen(&[b"a\tb"]), "a       b");
    }

    #[test]
    fn escapes_are_dropped() {
        assert_eq!(
            screen(&[b"\x1b[1;32mgreen\x1b[0m \x1b]0;title\x07ok"]),
            "green ok"
        );
        assert_eq!(screen(&[b"\x1b[?1034hsh-3.2$ "]), "sh-3.2$ ");
        assert_eq!(screen(&[b"junk\x1b[2Jclean"]), "clean");
        assert_eq!(screen(&[b"\x1b]2;t\x1b\\x"]), "x");
    }

    #[test]
    fn utf8_split_across_chunks() {
        let bytes = "Tiếng Việt".as_bytes();
        assert_eq!(screen(&[&bytes[..3], &bytes[3..]]), "Tiếng Việt");
    }

    #[test]
    fn scrollback_is_bounded() {
        let mut s = Screen::default();
        for _ in 0..(MAX_LINES + 50) {
            s.feed(b"x\n");
        }
        assert_eq!(s.text().split('\n').count(), MAX_LINES);
    }

    #[test]
    fn keys() {
        assert_eq!(key_bytes("a", false, false, false), Some(b"a".to_vec()));
        assert_eq!(
            key_bytes("Enter", false, false, false),
            Some(b"\r".to_vec())
        );
        assert_eq!(key_bytes("c", true, false, false), Some(vec![3]));
        assert_eq!(key_bytes("D", true, false, false), Some(vec![4]));
        assert_eq!(
            key_bytes("ArrowUp", false, false, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes("ê", false, false, false),
            Some("ê".as_bytes().to_vec())
        );
        assert_eq!(key_bytes("b", false, true, false), Some(b"\x1bb".to_vec()));
        assert_eq!(
            key_bytes("w", false, false, true),
            None,
            "Cmd shortcuts stay with the app"
        );
        assert_eq!(key_bytes("Shift", false, false, false), None);
        assert_eq!(key_bytes("1", true, false, false), None);
    }
}
