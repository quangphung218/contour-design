//! "Brand font files missing" (track C2-chat-home, the original's
//! `MissingBrandFontsBanner`): an editable design system without any font
//! file previews its type with substitutes. Pure, tested natively.

/// Whether `name` is a font file (by extension, or under a `fonts/` folder).
#[must_use]
pub fn is_font(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".otf", ".ttf", ".woff", ".woff2"]
        .iter()
        .any(|e| lower.ends_with(e))
        || lower.contains("fonts/")
}

/// The banner shows for an editable system with no font file.
#[must_use]
pub fn missing<'a>(editable: bool, names: impl IntoIterator<Item = &'a str>) -> bool {
    editable && !names.into_iter().any(is_font)
}

/// `localStorage` key of "Keep substitutes" for one system.
#[must_use]
pub fn dismiss_key(id: &str) -> String {
    format!("od:font-banner-dismissed:{id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_are_found_by_extension_or_folder() {
        assert!(is_font("Brand-Bold.WOFF2") && is_font("assets/fonts/brand.css"));
        assert!(!is_font("logo.svg"));
        assert!(missing(true, ["logo.svg", "hero.png"]));
        assert!(!missing(true, ["logo.svg", "Inter.ttf"]));
        assert!(
            !missing(false, ["logo.svg"]),
            "read-only systems never warn"
        );
        assert_eq!(dismiss_key("acme"), "od:font-banner-dismissed:acme");
    }
}
