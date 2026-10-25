//! BYOK provider presets (re-exported from `od-contracts`) and key cleanup.

pub use od_contracts::byok_presets::{PRESETS, Preset, preset, preset_for};

/// Removes what a paste drags along: zero-width characters, line breaks,
/// tabs and surrounding spaces. Returns the key and whether it changed.
#[must_use]
pub fn clean_key(raw: &str) -> (String, bool) {
    let cleaned: String = raw
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\r' | '\n' | '\t'
            )
        })
        .collect::<String>()
        .trim()
        .to_owned();
    let changed = cleaned != raw;
    (cleaned, changed)
}

/// True when Save must wait for a key: the preset needs one, none is typed
/// and no stored key applies. The daemon uses a stored key only for the base
/// URL it was saved with (`stored_base`, `None` when no key is stored).
#[must_use]
pub fn key_missing(
    preset_id: &str,
    typed: &str,
    base_url: &str,
    stored_base: Option<&str>,
) -> bool {
    let norm = |u: &str| u.trim().trim_end_matches('/').to_ascii_lowercase();
    let needs = preset(preset_id).is_none_or(|p| p.needs_key);
    let stored = stored_base.is_some_and(|b| norm(b) == norm(base_url));
    needs && typed.trim().is_empty() && !stored
}

/// A warning for keys whose shape does not fit a first-party host.
#[must_use]
pub fn key_shape_warning(preset_id: &str, key: &str) -> bool {
    match preset_id {
        "anthropic" => !key.starts_with("sk-ant-"),
        "openai" => !key.starts_with("sk-") || key.starts_with("sk-ant-"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_cleaned() {
        assert_eq!(clean_key(" sk-a\u{200b}b\n"), ("sk-ab".to_owned(), true));
        assert_eq!(clean_key("sk-ab"), ("sk-ab".to_owned(), false));
        assert!(key_shape_warning("anthropic", "sk-123"));
        assert!(!key_shape_warning("anthropic", "sk-ant-1"));
        assert!(key_shape_warning("openai", "sk-ant-1"));
    }

    #[test]
    fn save_waits_for_a_key_the_provider_needs() {
        let base = "https://api.anthropic.com";
        assert!(key_missing("anthropic", "", base, None));
        assert!(key_missing("anthropic", "   ", base, None));
        assert!(key_missing("custom", "", "https://my.proxy", None));
        assert!(!key_missing("anthropic", "sk-ant-1", base, None));
        // A stored key counts for the base URL it was saved with only.
        assert!(!key_missing(
            "anthropic",
            "",
            base,
            Some("https://api.anthropic.com/")
        ));
        assert!(key_missing(
            "anthropic",
            "",
            base,
            Some("https://other.host")
        ));
        let local = PRESETS.iter().find(|p| !p.needs_key).map(|p| p.id);
        assert!(!key_missing(
            local.unwrap(),
            "",
            "http://localhost:11434",
            None
        ));
    }
}
