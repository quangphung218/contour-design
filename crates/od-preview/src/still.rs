//! Still pages: project covers and template thumbnails show a page with its
//! `<script>` elements taken out. The thumbnail frame is a sandbox that
//! runs no script anyway, but it logs a console error for each one it
//! blocks (UI-020). The page keeps its own URL and response headers (the
//! no-network CSP), so its relative assets load as before.

/// `html` without its `<script>` elements (an unclosed one runs to the end).
/// `<scripts>` and `<script-x>` are other elements and stay.
#[must_use]
pub fn without_scripts(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find("<script") {
        let open = at + found;
        let next = lower.as_bytes().get(open + 7).copied();
        if !matches!(
            next,
            Some(b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
        ) {
            out.push_str(&html[at..open + 7]);
            at = open + 7;
            continue;
        }
        out.push_str(&html[at..open]);
        at = match lower[open..].find("</script") {
            Some(close) => {
                let end = open + close;
                lower[end..].find('>').map_or(html.len(), |gt| end + gt + 1)
            }
            None => html.len(),
        };
    }
    out.push_str(&html[at..]);
    out
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
}
