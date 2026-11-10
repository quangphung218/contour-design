//! A failed media task's error in words (QA-077). The daemon stores a
//! provider's refusal as `<provider> <status>: <body>`, the body as the
//! provider sent it (often JSON, cut at 240 characters). The Queue shows
//! the provider, the message inside the body and, for a refused key, a
//! way to Providers. Pure, tested natively.

use serde_json::Value;

/// A task error split into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    /// Provider id (`openai`), when the error came from one.
    pub provider: Option<String>,
    /// The provider's HTTP status.
    pub status: Option<u16>,
    /// The reason in words; empty when the body held none.
    pub message: String,
}

impl ProviderError {
    /// The provider refused the key (401, 403): fixed in Providers.
    #[must_use]
    pub fn key_refused(&self) -> bool {
        matches!(self.status, Some(401 | 403))
    }
}

/// Splits `raw`. An error not in the provider's shape keeps its text,
/// except a JSON or HTML body, which is reduced to its message or dropped.
#[must_use]
pub fn parse(raw: &str) -> ProviderError {
    let raw = raw.trim();
    if let Some((head, body)) = raw.split_once(": ")
        && let Some((tag, code)) = head.rsplit_once(' ')
        && let Ok(status) = code.parse::<u16>()
        && (100..600).contains(&status)
        && is_tag(tag)
    {
        return ProviderError {
            provider: Some(provider_id(tag)),
            status: Some(status),
            message: body_message(body).unwrap_or_default(),
        };
    }
    // `openai non-JSON response: <html…>` and the like.
    let message = match raw.split_once(": ") {
        Some((head, body)) if looks_structured(body) => match body_message(body) {
            Some(m) => format!("{head}: {m}"),
            None => head.to_owned(),
        },
        _ => raw.to_owned(),
    };
    ProviderError {
        provider: None,
        status: None,
        message,
    }
}

/// The daemon's tag: a lower-case provider word, then what was asked
/// (`openai`, `fal submit`, `leonardo.ai submit`, `aihubmix image (gemini)`).
fn is_tag(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || " -_.()".contains(c))
}

/// The Providers id behind a tag's first word.
fn provider_id(tag: &str) -> String {
    let word = tag.split(' ').next().unwrap_or(tag);
    match word {
        "xai" => "grok",
        "nano-banana" => "nanobanana",
        "leonardo.ai" => "leonardo",
        "custom" => "custom-image",
        w => w,
    }
    .to_owned()
}

fn looks_structured(body: &str) -> bool {
    let b = body.trim_start();
    b.starts_with('{') || b.starts_with('[') || b.starts_with('<')
}

/// The message in a provider's body: the JSON's message, else the text
/// itself when it is plain words. `None` for HTML or JSON without one.
fn body_message(body: &str) -> Option<String> {
    let body = body.trim();
    if body.starts_with('<') {
        return None;
    }
    if !(body.starts_with('{') || body.starts_with('[')) {
        return (!body.is_empty()).then(|| body.to_owned());
    }
    match serde_json::from_str::<Value>(body) {
        Ok(v) => json_message(&v),
        // Cut at 240 characters: no longer JSON, the message may still be whole.
        Err(_) => scan_message(body),
    }
    .map(|m| m.trim().to_owned())
    .filter(|m| !m.is_empty())
}

fn json_message(v: &Value) -> Option<String> {
    let text = |v: &Value| v.as_str().map(str::to_owned);
    let err = &v["error"];
    text(&err["message"])
        .or_else(|| text(err))
        .or_else(|| text(&v["message"]))
        .or_else(|| text(&v["detail"]))
        .or_else(|| text(&v["detail"]["message"]))
        .or_else(|| text(&err["detail"]))
        .or_else(|| text(&v["errors"][0]["message"]))
        .or_else(|| text(&v[0]["error"]["message"]))
}

/// The first `"message": "…"` string in a cut JSON body.
fn scan_message(body: &str) -> Option<String> {
    let at = body.find("\"message\"")? + "\"message\"".len();
    let rest = body[at..].trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' | 't' | 'r' => out.push(' '),
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    // Cut inside the message: what there is, marked as cut.
    (!out.is_empty()).then(|| format!("{out}…"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_json_body_gives_its_message() {
        let e = parse(r#"openai 500: {"error":{"message":"upstream exploded"}}"#);
        assert_eq!(e.provider.as_deref(), Some("openai"));
        assert_eq!(e.status, Some(500));
        assert_eq!(e.message, "upstream exploded");
        assert!(!e.key_refused());
    }

    #[test]
    fn a_refused_key_is_401_or_403() {
        let e = parse(
            r#"openai 401: {"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#,
        );
        assert_eq!(e.message, "Incorrect API key provided");
        assert!(e.key_refused());
        assert!(parse(r#"fal 403: {"detail":"Forbidden"}"#).key_refused());
        assert!(!parse("fal 429: slow down").key_refused());
    }

    #[test]
    fn the_daemons_multi_word_tags_name_the_provider() {
        // The tags od-media writes (`body_text`/`bytes_of` callers).
        for (raw, id) in [
            (r#"fal submit 401: {"detail":"Invalid key"}"#, "fal"),
            (r#"fal poll 403: {"detail":"Forbidden"}"#, "fal"),
            (r#"grok image 403: {"error":"bad key"}"#, "grok"),
            (r#"grok video submit 401: {"error":"bad key"}"#, "grok"),
            (r#"xai tts 401: {"error":"bad key"}"#, "grok"),
            (
                r#"openai speech 401: {"error":{"message":"bad key"}}"#,
                "openai",
            ),
            (r#"minimax image 401: {"message":"bad key"}"#, "minimax"),
            (
                r#"volcengine task create 403: {"message":"bad key"}"#,
                "volcengine",
            ),
            (
                r#"openrouter image 401: {"error":{"message":"bad key"}}"#,
                "openrouter",
            ),
            (
                r#"nano-banana image 401: {"error":{"message":"bad key"}}"#,
                "nanobanana",
            ),
            (r#"leonardo.ai submit 401: {"error":"bad key"}"#, "leonardo"),
            (r#"custom image 401: {"error":"bad key"}"#, "custom-image"),
            (
                r#"aihubmix image (gemini) 403: {"error":{"message":"bad key"}}"#,
                "aihubmix",
            ),
        ] {
            let e = parse(raw);
            assert_eq!(e.provider.as_deref(), Some(id), "{raw}");
            assert!(e.key_refused(), "{raw}");
            assert!(!e.message.contains('{'), "{raw}");
        }
        assert_eq!(
            parse(r#"fal submit 401: {"detail":"Invalid key"}"#).message,
            "Invalid key"
        );
    }

    #[test]
    fn other_body_shapes() {
        assert_eq!(
            parse(r#"fal 422: {"detail":"bad size"}"#).message,
            "bad size"
        );
        assert_eq!(
            parse(r#"elevenlabs 400: {"detail":{"status":"x","message":"voice not found"}}"#)
                .message,
            "voice not found"
        );
        assert_eq!(
            parse(r#"grok 400: {"error":"no prompt"}"#).message,
            "no prompt"
        );
        assert_eq!(
            parse(r#"x 400: {"errors":[{"message":"first"}]}"#).message,
            "first"
        );
        assert_eq!(parse("fal 429: slow down").message, "slow down");
    }

    #[test]
    fn a_body_cut_at_240_characters_still_gives_its_message() {
        let long = format!(
            r#"openai 400: {{"error":{{"message":"Your prompt was \"rejected\"","param":"{}"#,
            "p".repeat(300)
        );
        assert_eq!(parse(&long).message, r#"Your prompt was "rejected""#);
        let cut = r#"openai 400: {"error":{"message":"Your prompt was rej"#;
        assert_eq!(parse(cut).message, "Your prompt was rej…");
    }

    #[test]
    fn json_or_html_without_a_message_shows_nothing_raw() {
        assert_eq!(parse(r#"openai 500: {"error":{"code":1}}"#).message, "");
        assert_eq!(
            parse("openai 502: <html><body>Bad gateway</body></html>").message,
            ""
        );
        assert_eq!(
            parse("openai non-JSON response: <html>oops</html>").message,
            "openai non-JSON response"
        );
    }

    #[test]
    fn errors_not_from_a_provider_keep_their_text() {
        let e = parse("no credential for openai");
        assert_eq!(e.provider, None);
        assert_eq!(e.message, "no credential for openai");
        assert_eq!(parse("openai 500").message, "openai 500");
        assert_eq!(parse("timed out: 120 s").message, "timed out: 120 s");
        assert_eq!(
            parse("Model gpt 9: unknown").status,
            None,
            "not a provider tag"
        );
    }
}
