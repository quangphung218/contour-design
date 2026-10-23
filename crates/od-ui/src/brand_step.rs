//! Onboarding ▸ Your brand (track C2-chat-home): the website a design
//! system is extracted from. Pure, tested natively.

/// The URL to extract from: `https://` added when missing; `None` unless
/// it names a host with a dot (no spaces, http(s) only).
#[must_use]
pub fn normalize_url(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() || s.contains(char::is_whitespace) {
        return None;
    }
    let url = if s.starts_with("http://") || s.starts_with("https://") {
        s.to_owned()
    } else if s.contains("://") {
        return None;
    } else {
        format!("https://{s}")
    };
    let host = url.split("://").nth(1)?.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?.split(':').next()?;
    (host.contains('.') && !host.starts_with('.') && !host.ends_with('.')).then_some(url)
}

/// A design system name from the URL's host: `www.acme-coffee.com` →
/// "Acme Coffee".
#[must_use]
pub fn brand_name(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?', '#', ':'])
        .next()
        .unwrap_or_default();
    let host = host.strip_prefix("www.").unwrap_or(host);
    let stem = host.split('.').next().unwrap_or(host);
    stem.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_get_a_scheme_and_need_a_real_host() {
        assert_eq!(
            normalize_url(" acme.com ").as_deref(),
            Some("https://acme.com")
        );
        assert_eq!(
            normalize_url("http://acme.com/x").as_deref(),
            Some("http://acme.com/x")
        );
        for bad in [
            "",
            "acme",
            "acme .com",
            "ftp://acme.com",
            ".com",
            "https://localhost",
        ] {
            assert_eq!(normalize_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn names_come_from_the_host() {
        assert_eq!(
            brand_name("https://www.acme-coffee.com/menu"),
            "Acme Coffee"
        );
        assert_eq!(brand_name("https://linear.app"), "Linear");
    }
}
