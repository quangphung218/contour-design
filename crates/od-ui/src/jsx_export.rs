//! Export ▸ Export as JSX (track C2-chat-home, the original's
//! `exportAsJsx`): a React component file is downloaded as its own source,
//! `.tsx` kept, under a safe name. Pure, tested natively.

/// Whether `path` is a React component source.
#[must_use]
pub fn is_component(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".jsx") || lower.ends_with(".tsx")
}

/// The download name: the file's stem made safe (the original's
/// `safeFilename`: runs of other characters become `-`, trimmed, at most
/// 60 characters, `component` when nothing is left) plus `.tsx` for a
/// TypeScript source, else `.jsx`.
#[must_use]
pub fn download_name(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) if e.eq_ignore_ascii_case("tsx") => (s, ".tsx"),
        Some((s, e)) if e.eq_ignore_ascii_case("jsx") => (s, ".jsx"),
        _ => (base, ".jsx"),
    };
    let mut safe = String::with_capacity(stem.len());
    let mut dash = false;
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
            safe.push(c);
            dash = false;
        } else if !dash {
            safe.push('-');
            dash = true;
        }
    }
    let safe: String = safe.trim_matches('-').chars().take(60).collect();
    let safe = if safe.is_empty() {
        "component".to_owned()
    } else {
        safe
    };
    format!("{safe}{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_keep_their_extension_under_a_safe_name() {
        assert!(is_component("src/Card.TSX") && is_component("a.jsx"));
        assert!(!is_component("index.html") && !is_component("app.js"));
        assert_eq!(download_name("src/Pricing Card.tsx"), "Pricing-Card.tsx");
        assert_eq!(download_name("ui/hero.jsx"), "hero.jsx");
        assert_eq!(download_name("ui/✨.jsx"), "component.jsx");
        let long = format!("{}.jsx", "a".repeat(80));
        assert_eq!(download_name(&long).len(), 60 + ".jsx".len());
    }
}
