//! Still pages: project covers and template thumbnails show a page with its
//! `<script>` elements taken out. The thumbnail frame is a sandbox that
//! runs no script anyway, but it logs a console error for each one it
//! blocks (UI-020). The page keeps its own URL and response headers (the
//! no-network CSP), so its relative assets load as before.

/// Pages larger than this (bytes) get a blank cover: stripping one means
/// holding it whole, for every cover on screen.
pub const MAX_STILL_BYTES: u64 = 5 * 1024 * 1024;

/// What a page over [`MAX_STILL_BYTES`] shows as its cover.
pub const BLANK: &str = "<!doctype html><html><head></head><body></body></html>";

/// `html` without its `<script>` elements (an unclosed one runs to the end).
/// `<scripts>` and `<script-x>` are other elements and stay. Matches tags
/// in any case without a lowercased copy of the page.
#[must_use]
pub fn without_scripts(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while let Some(open) = find_tag(html, at, "<script") {
        let next = html.as_bytes().get(open + 7).copied();
        if !matches!(
            next,
            Some(b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
        ) {
            out.push_str(&html[at..open + 7]);
            at = open + 7;
            continue;
        }
        out.push_str(&html[at..open]);
        at = match find_tag(html, open, "</script") {
            Some(end) => html[end..].find('>').map_or(html.len(), |gt| end + gt + 1),
            None => html.len(),
        };
    }
    out.push_str(&html[at..]);
    out
}

/// Where `tag` (ASCII, starting with `<`) next starts in `html` at or after
/// `from`, in any case. A match starts at an ASCII `<`, so on a char
/// boundary.
fn find_tag(html: &str, from: usize, tag: &str) -> Option<usize> {
    let (bytes, tag) = (html.as_bytes(), tag.as_bytes());
    let mut at = from;
    while let Some(off) = bytes.get(at..)?.iter().position(|&b| b == b'<') {
        let start = at + off;
        if bytes
            .get(start..start + tag.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(tag))
        {
            return Some(start);
        }
        at = start + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::without_scripts;

    #[test]
    fn scripts_go_and_the_rest_stays() {
        let html = "<!doctype html><html><head><title>t</title><SCRIPT src=\"a.js\"></SCRIPT>\
                    <link rel=stylesheet href=s.css></head><body><h1>Hi</h1>\
                    <script type=module>if (a < b) go('</div>')</script><p>end</p></body></html>";
        let out = without_scripts(html);
        assert!(!out.to_ascii_lowercase().contains("<script"), "{out}");
        assert!(!out.contains("go("), "{out}");
        assert!(
            out.contains("<head><title>t</title><link rel=stylesheet href=s.css></head>"),
            "{out}"
        );
        assert!(
            out.contains("<h1>Hi</h1>") && out.contains("<p>end</p>"),
            "{out}"
        );
    }

    #[test]
    fn look_alikes_and_unclosed_scripts() {
        let out = without_scripts("<scripts>x</scripts><p>a</p><script>never closed");
        assert_eq!(out, "<scripts>x</scripts><p>a</p>");
    }

    #[test]
    fn any_case_and_text_around_it_stay_whole() {
        let out =
            without_scripts("<p>Ünï <b>x</b></p><ScRiPt>a</sCrIpT ><p>é</p><SCRIPT>b</SCRIPT>");
        assert_eq!(out, "<p>Ünï <b>x</b></p><p>é</p>");
    }
}
