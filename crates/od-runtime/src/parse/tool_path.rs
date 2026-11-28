//! The file a tool call writes, read from its arguments while they still
//! stream (ports `tool-input-path-scanner.ts`).
//!
//! Claude streams a call's JSON arguments as `input_json_delta` fragments cut
//! at any byte, one path often across several of them:
//!
//! ```text
//! '{"file_path": "/private/tmp/claude-501'
//! '/-Users-elian-Documents-open-design/bff58f5e-18'
//! 'scratchpad/w107/cwd/alpha.html'
//! '", "content": "<!doctype html><html><body'
//! ```
//!
//! `file_path` comes first, so the path is known after a few dozen bytes
//! while `content` still has tens of kilobytes to go. Parsing the growing
//! buffer as JSON would be quadratic and could not answer before the last
//! byte; this scanner walks each new character once instead.
//!
//! It answers only with a path that is provably complete (its closing quote
//! arrived). It stays silent when the key is missing, nested, not a string,
//! longer than [`MAX_PATH_CHARS`], when the tool does not write a file, or
//! after [`SCAN_BUDGET_BYTES`] without a path. A half-read path never
//! reaches the UI.

/// Tools whose arguments name the file the call writes.
const FILE_WRITE_TOOLS: [&str; 6] = [
    "Write",
    "Edit",
    "MultiEdit",
    "NotebookEdit",
    "write_file",
    "replace",
];

/// Keys that hold that file's path, most specific first. `path` is a search
/// root on `Grep`/`Glob`; those never get here, as the tool is checked first.
const PATH_KEYS: [&str; 4] = ["file_path", "notebook_path", "filePath", "path"];

/// Longest plausible path; a longer value is not a path.
const MAX_PATH_CHARS: usize = 4096;
/// Longest plausible argument key.
const MAX_KEY_CHARS: usize = 64;
/// Give up on arguments this large that named no path.
pub const SCAN_BUDGET_BYTES: usize = 1 << 18;

/// Whether `name` is a tool whose arguments name the file it writes.
#[must_use]
pub fn is_file_write_tool(name: &str) -> bool {
    FILE_WRITE_TOOLS.contains(&name)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capture {
    None,
    Key,
    Path,
}

/// Reads top-level string values of a JSON object as it arrives. Whether a
/// quote opens a key or a value is decided by the last structural character
/// (`{` or `,` opens a key), so a `content` value holding `{`, `}` or
/// `"file_path"` is never taken for structure.
#[derive(Debug)]
pub struct ToolPathScanner {
    scanned: usize,
    depth: i32,
    /// The top-level object closed, a path was found, or the budget ran out.
    done: bool,
    in_string: bool,
    capture: Capture,
    buf: String,
    buf_chars: usize,
    overflowed: bool,
    escaping: bool,
    /// Hex digits of a `\uXXXX` escape read so far, possibly across fragments.
    unicode: Option<String>,
    /// A high surrogate waiting for its low half.
    high_surrogate: Option<u16>,
    last_significant: char,
    pending_key: Option<String>,
    expect_colon: bool,
    expect_path_value: bool,
}

impl ToolPathScanner {
    /// A scanner for a call of `tool`, or `None` when that tool does not
    /// name a file (so other calls cost one lookup).
    #[must_use]
    pub fn for_tool(tool: &str) -> Option<Self> {
        is_file_write_tool(tool).then(|| Self {
            scanned: 0,
            depth: 0,
            done: false,
            in_string: false,
            capture: Capture::None,
            buf: String::new(),
            buf_chars: 0,
            overflowed: false,
            escaping: false,
            unicode: None,
            high_surrogate: None,
            last_significant: '\0',
            pending_key: None,
            expect_colon: false,
            expect_path_value: false,
        })
    }

    /// Feeds the next raw fragment. Returns the path the first time one is
    /// complete; `None` otherwise, and always after that.
    pub fn push(&mut self, fragment: &str) -> Option<String> {
        if self.done {
            return None;
        }
        self.scanned += fragment.len();
        for ch in fragment.chars() {
            if let Some(path) = self.step(ch) {
                self.done = true;
                return Some(path);
            }
            if self.done {
                return None;
            }
        }
        if self.scanned > SCAN_BUDGET_BYTES {
            self.done = true;
        }
        None
    }

    fn step(&mut self, ch: char) -> Option<String> {
        if self.in_string {
            return self.step_in_string(ch);
        }
        if ch == '"' {
            self.in_string = true;
            self.buf.clear();
            self.buf_chars = 0;
            self.overflowed = false;
            self.capture = if self.depth == 1 && matches!(self.last_significant, '{' | ',') {
                Capture::Key
            } else if self.expect_path_value {
                Capture::Path
            } else {
                Capture::None
            };
            self.expect_path_value = false;
            return None;
        }
        if ch.is_ascii_whitespace() {
            return None;
        }
        match ch {
            '{' | '[' => self.depth += 1,
            '}' | ']' => {
                self.depth -= 1;
                if self.depth <= 0 {
                    self.done = true;
                }
            }
            ':' if self.expect_colon => {
                self.expect_colon = false;
                // Only a path key at the top level opens a value we read.
                self.expect_path_value = self
                    .pending_key
                    .take()
                    .is_some_and(|k| PATH_KEYS.contains(&k.as_str()));
            }
            _ => {}
        }
        if ch != ':' {
            self.expect_colon = false;
            self.pending_key = None;
            // A path key followed by a non-string value is not a path.
            self.expect_path_value = false;
        }
        self.last_significant = ch;
        None
    }

    fn step_in_string(&mut self, ch: char) -> Option<String> {
        if let Some(hex) = self.unicode.as_mut() {
            hex.push(ch);
            if hex.len() == 4 {
                let code = u16::from_str_radix(hex, 16);
                self.unicode = None;
                match code {
                    Ok(code) => self.append_utf16(code),
                    // A malformed `\uXXXX`: not a string we can read.
                    Err(_) => self.abandon(),
                }
            }
            return None;
        }
        if self.escaping {
            self.escaping = false;
            if ch == 'u' {
                self.unicode = Some(String::with_capacity(4));
                return None;
            }
            let unescaped = match ch {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                'b' => '\u{8}',
                'f' => '\u{c}',
                other => other,
            };
            self.append(unescaped);
            return None;
        }
        match ch {
            '\\' => {
                self.escaping = true;
                None
            }
            '"' => self.close_string(),
            _ => {
                self.append(ch);
                None
            }
        }
    }

    /// The closing quote: the string is complete.
    fn close_string(&mut self) -> Option<String> {
        self.flush_surrogate();
        self.in_string = false;
        self.last_significant = '"';
        let captured = std::mem::replace(&mut self.capture, Capture::None);
        let value = std::mem::take(&mut self.buf);
        if self.overflowed {
            return None;
        }
        match captured {
            Capture::Key => {
                self.pending_key = Some(value);
                self.expect_colon = true;
                None
            }
            Capture::Path if !value.is_empty() => Some(value),
            _ => None,
        }
    }

    fn append_utf16(&mut self, code: u16) {
        match code {
            0xD800..=0xDBFF => {
                self.flush_surrogate();
                self.high_surrogate = Some(code);
            }
            0xDC00..=0xDFFF => match self.high_surrogate.take() {
                Some(high) => {
                    let ch = char::decode_utf16([high, code])
                        .next()
                        .and_then(Result::ok)
                        .unwrap_or(char::REPLACEMENT_CHARACTER);
                    self.append(ch);
                }
                None => self.append(char::REPLACEMENT_CHARACTER),
            },
            _ => {
                self.flush_surrogate();
                self.append(char::from_u32(u32::from(code)).unwrap_or(char::REPLACEMENT_CHARACTER));
            }
        }
    }

    /// A high surrogate that no low half followed stands for itself.
    fn flush_surrogate(&mut self) {
        if self.high_surrogate.take().is_some() {
            self.append(char::REPLACEMENT_CHARACTER);
        }
    }

    /// Stops reading the current string without producing anything.
    fn abandon(&mut self) {
        self.buf.clear();
        self.overflowed = true;
    }

    fn append(&mut self, ch: char) {
        if self.high_surrogate.is_some() {
            self.flush_surrogate();
        }
        if self.capture == Capture::None || self.overflowed {
            return;
        }
        let limit = if self.capture == Capture::Key {
            MAX_KEY_CHARS
        } else {
            MAX_PATH_CHARS
        };
        if self.buf_chars >= limit {
            // Too long to be what we look for: keep scanning structure.
            self.abandon();
            return;
        }
        self.buf.push(ch);
        self.buf_chars += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(tool: &str, fragments: &[&str]) -> Vec<String> {
        let mut s = ToolPathScanner::for_tool(tool).expect("a file tool");
        fragments.iter().filter_map(|f| s.push(f)).collect()
    }

    #[test]
    fn a_path_split_across_fragments_comes_once_its_quote_closes() {
        let fragments = [
            "",
            "{\"file_path\": \"/private/tmp/claude-501",
            "/-Users-elian/bff58f5e-18",
            "scratchpad/cwd/alpha.html",
            "\", \"content\": \"<!doctype html><html><body",
            "><h1>Alpha</h1></body></html>",
            "\"}",
        ];
        let mut s = ToolPathScanner::for_tool("Write").expect("Write scans");
        let found: Vec<(usize, String)> = fragments
            .iter()
            .enumerate()
            .filter_map(|(i, f)| s.push(f).map(|p| (i, p)))
            .collect();
        assert_eq!(
            found,
            [(
                4,
                "/private/tmp/claude-501/-Users-elian/bff58f5e-18scratchpad/cwd/alpha.html"
                    .to_owned()
            )]
        );
    }

    #[test]
    fn an_unclosed_path_says_nothing() {
        assert!(scan("Write", &["{\"file_path\": \"/a/b/half"]).is_empty());
    }

    #[test]
    fn only_file_writing_tools_are_scanned() {
        for tool in ["Bash", "Grep", "Glob", "Read", "TodoWrite", "Agent"] {
            assert!(ToolPathScanner::for_tool(tool).is_none(), "{tool}");
        }
        for tool in FILE_WRITE_TOOLS {
            assert!(ToolPathScanner::for_tool(tool).is_some(), "{tool}");
        }
    }

    #[test]
    fn every_path_key_is_read_and_the_first_one_wins() {
        for key in PATH_KEYS {
            let json = format!("{{\"{key}\": \"x.html\", \"file_path\": \"y.html\"}}");
            assert_eq!(scan("Edit", &[&json]), ["x.html"], "{key}");
        }
    }

    #[test]
    fn a_nested_or_non_string_path_is_not_the_target() {
        assert!(scan("Write", &["{\"meta\": {\"file_path\": \"n.html\"}}"]).is_empty());
        assert!(scan("Write", &["{\"file_path\": 3, \"content\": \"x\"}"]).is_empty());
        assert!(scan("Write", &["{\"file_path\": null}"]).is_empty());
        assert!(scan("Write", &["{\"file_path\": [\"a.html\"]}"]).is_empty());
        assert!(scan("Write", &["{\"file_path\": {\"x\": \"a.html\"}}"]).is_empty());
        assert!(scan("Write", &["{\"file_path\": \"\"}"]).is_empty());
    }

    #[test]
    fn a_path_key_inside_content_is_not_structure() {
        let json = r#"{"content": "{\"file_path\": \"fake.html\"} , \"file_path\": \"f2\"", "file_path": "real.html"}"#;
        assert_eq!(scan("Write", &[json]), ["real.html"]);
    }

    #[test]
    fn escapes_split_across_fragments_decode() {
        assert_eq!(
            scan(
                "Write",
                &[
                    "{\"file_path\": \"a\\",
                    "\"b\\u00",
                    "e9\\ud83d",
                    "\\ude00.html\"}"
                ]
            ),
            ["a\"bé😀.html"]
        );
    }

    #[test]
    fn a_malformed_unicode_escape_drops_the_value() {
        assert!(scan("Write", &["{\"file_path\": \"a\\uZZZZ.html\"}"]).is_empty());
    }

    #[test]
    fn a_path_too_long_is_not_a_path() {
        let long = "a".repeat(MAX_PATH_CHARS + 1);
        let json = format!("{{\"file_path\": \"{long}\"}}");
        assert!(scan("Write", &[&json]).is_empty());
        let fits = "a".repeat(MAX_PATH_CHARS);
        let json = format!("{{\"file_path\": \"{fits}\"}}");
        assert_eq!(scan("Write", &[&json]), [fits]);
    }

    #[test]
    fn a_late_key_still_resolves_and_the_budget_stops_the_scan() {
        let big = "x".repeat(1000);
        let json = format!("{{\"content\": \"{big}\", \"file_path\": \"late.html\"}}");
        assert_eq!(scan("Write", &[&json]), ["late.html"]);
        let huge = "x".repeat(SCAN_BUDGET_BYTES + 10);
        let mut s = ToolPathScanner::for_tool("Write").expect("Write scans");
        assert_eq!(s.push(&format!("{{\"content\": \"{huge}")), None);
        assert_eq!(s.push("\", \"file_path\": \"late.html\"}"), None);
    }

    #[test]
    fn nothing_after_the_object_closes() {
        assert!(scan("Write", &["{\"content\": \"x\"}", "{\"file_path\": \"y\"}"]).is_empty());
    }
}
