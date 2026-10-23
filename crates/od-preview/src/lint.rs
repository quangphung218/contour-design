//! Anti-slop lint for generated HTML (spec 1.2, P2). Same rules and
//! thresholds as the original app's `apps/daemon/src/lint-artifact.ts`:
//! cheap regex checks, no HTML parsing. A finding carries a snippet so the
//! agent can verify it; false positives are tolerated.

use od_contracts::{LintFinding, LintSeverity};
use regex::{Captures, Regex};
use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

/// Tailwind violet/purple and indigo: the stock AI palette.
const PURPLE_HEXES: [&str; 20] = [
    "#a855f7", "#9333ea", "#7c3aed", "#6d28d9", "#581c87", "#8b5cf6", "#a78bfa", "#c4b5fd",
    "#ddd6fe", "#ede9fe", "#6366f1", "#4f46e5", "#4338ca", "#3730a3", "#312e81", "#818cf8",
    "#a5b4fc", "#c7d2fe", "#e0e7ff", "#eef2ff",
];

/// Blue and sky stops of the blue→cyan "trust" gradient.
const TRUST_BLUE_HEXES: [&str; 13] = [
    "#3b82f6", "#2563eb", "#1d4ed8", "#1e40af", "#1e3a8a", "#60a5fa", "#93c5fd", "#bfdbfe",
    "#0ea5e9", "#0284c7", "#0369a1", "#38bdf8", "#7dd3fc",
];
const TRUST_CYAN_HEXES: [&str; 8] = [
    "#06b6d4", "#0891b2", "#0e7490", "#155e75", "#164e63", "#22d3ee", "#67e8f9", "#a5f3fc",
];

/// Even one solid use of these reads as the model's default accent.
const AI_DEFAULT_INDIGO: [&str; 7] = [
    "#6366f1", "#4f46e5", "#4338ca", "#3730a3", "#8b5cf6", "#7c3aed", "#a855f7",
];

const SLOP_EMOJI: [&str; 17] = [
    "✨", "🚀", "🎯", "⚡", "🔥", "💡", "📈", "🎨", "🛡️", "🌟", "💪", "🎉", "👋", "🙌", "✅", "⭐",
    "🏆",
];

/// Root font size assumed for `rem` (browser default, seed templates).
const ROOT_FONT_PX: f64 = 16.0;
/// `var()` resolution hops; bounds cyclic token definitions.
const VAR_RESOLVE_MAX_DEPTH: usize = 4;
/// Raw hex values tolerated outside `:root` (device chrome needs some).
const MAX_RAW_HEX: usize = 12;
/// Inline `var(--accent)` uses before the accent counts as overused.
const MAX_INLINE_ACCENT: usize = 6;
const GLOBAL_THEME_ATTRIBUTES: [&str; 3] = ["data-theme", "data-color-scheme", "data-mode"];

macro_rules! re {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).expect($pattern));
    };
}

re!(HTML_COMMENT, r"(?s)<!--.*?-->");
re!(CSS_COMMENT, r"(?s)/\*.*?\*/");
re!(STYLE_BLOCK, r"(?is)<style[^>]*>(.*?)</style>");
re!(STYLE_BLOCK_PARTS, r"(?is)(<style[^>]*>)(.*?)(</style>)");
re!(STYLE_ELEMENT, r"(?is)<style.*?</style>");
re!(GRADIENT, r"(?i)linear-gradient\([^)]*\)");
re!(
    PURPLE_KEYWORD_GRADIENT,
    r"(?i)linear-gradient\([^)]*\b(purple|violet)\b[^)]*\)"
);
re!(WORD_BLUE, r"\bblue\b");
re!(WORD_CYAN, r"\bcyan\b");
re!(
    LEFT_ACCENT_CARD,
    r"(?i)\.[a-z-]+\s*\{[^}]*border-left\s*:\s*\d+px\s+solid\s+[^;]+;[^}]*border-radius\s*:\s*[1-9]"
);
re!(
    DISPLAY_SANS,
    r#"(?i)(?:h1|h2|h3|\.h-?(?:hero|xl|lg|md))[^{}]*\{[^}]*font-family\s*:\s*["']?(?:Inter|Roboto|Arial|-apple-system|system-ui|SF\s+Pro)"#
);
re!(SCROLL_INTO_VIEW, r"\.scrollIntoView\s*\(");
re!(
    UPPERCASE_RULE,
    r"(?i)([^{}]*)\{([^{}]*text-transform\s*:\s*uppercase[^{}]*)\}"
);
re!(UPPERCASE_DECL, r"(?i)text-transform\s*:\s*uppercase");
re!(INLINE_STYLE_DQ, r#"(?is)(?:^|\s)style\s*=\s*"(.*?)""#);
re!(INLINE_STYLE_SQ, r"(?is)(?:^|\s)style\s*=\s*'(.*?)'");
re!(
    EXTERNAL_IMAGE,
    r#"(?i)<img[^>]+src=["']https?://(?:images\.unsplash\.com|placehold\.co|placekitten\.com|via\.placeholder\.com|picsum\.photos|loremflickr\.com)"#
);
re!(ROOT_RULE, r":root\s*\{[^}]*\}");
re!(HEX_COLOR, r"#[0-9a-fA-F]{3,8}\b");
re!(ACCENT_VAR, r"var\(--accent\)");
re!(SECTION_TAG, r"(?i)<section\b[^>]*>");
re!(SECTION_ANCHOR, r"data-od-id\s*=|data-screen-label\s*=");
re!(SLIDE_CLASS, r#"class\s*=\s*["'][^"']*\bslide\b"#);
re!(
    SLIDE_SECTION,
    r#"(?i)<section\s+class\s*=\s*["'][^"']*\bslide\b[^"']*["']"#
);
re!(SLIDE_THEME, r"\b(light|dark|hero\s+light|hero\s+dark)\b");
re!(HERO_DARK, r"hero\s+dark");
re!(HERO_LIGHT, r"hero\s+light");
re!(WORD_DARK, r"\bdark\b");
re!(WORD_LIGHT, r"\blight\b");
re!(CSS_RULE, r"([^{}]*)\{([^{}]*)\}");
re!(TOKEN_DECL, r"^(--[\w-]+)\s*:\s*(.+)$");
re!(TOKEN_PROP, r"^--[\w-]+\s*:");
re!(COLOR_SCHEME_DECL, r"(?i)^color-scheme\s*:");
re!(VAR_REF, r"var\(\s*(--[\w-]+)\s*(?:,\s*([^()]*))?\)");
re!(LENGTH_TRACKING, r"(?i)^(-?\d*\.?\d+)\s*(em|px|rem)\b");
re!(LENGTH_FONT, r"(?i)^(-?\d*\.?\d+)\s*(px|rem)\b");
re!(
    THEME_TAG_SELECTOR,
    r"^(?::root|html|body)(?:\[([a-zA-Z-]+)(?:[*^$|~]?=[^\]]*)?\])?$"
);
re!(
    THEME_ATTR_SELECTOR,
    r"^\[([a-zA-Z-]+)(?:[*^$|~]?=[^\]]*)?\]$"
);
re!(BARE_GLOBAL, r"^(?::root|html|body)$");

static INVENTED_METRICS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)\b10×\s+(faster|better|easier)\b",
        r"(?i)\b100×\s+(faster|better)\b",
        r"(?i)\b99\.\d+%\s+uptime\b",
        r"(?i)\bzero[- ]downtime\b",
        r"(?i)\b3×\s+more\s+(productive|efficient)\b",
    ]
    .iter()
    .map(|p| Regex::new(p).expect(p))
    .collect()
});

static FILLER: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?i)\bfeature\s+(one|two|three|1|2|3)\b",
        r"(?i)\blorem\s+ipsum\b",
        r"(?i)\bdolor\s+sit\s+amet\b",
        r"(?i)\bplaceholder\s+text\b",
        r"(?i)\bsample\s+content\b",
    ]
    .iter()
    .map(|p| Regex::new(p).expect(p))
    .collect()
});

fn finding(
    severity: LintSeverity,
    id: &str,
    message: String,
    fix: &str,
    snippet: Option<String>,
) -> LintFinding {
    LintFinding {
        severity,
        id: id.to_owned(),
        message,
        fix: fix.to_owned(),
        snippet,
    }
}

/// Collapses whitespace and caps a snippet at 200 characters.
fn clip(s: &str) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 200 {
        let mut out: String = collapsed.chars().take(197).collect();
        out.push('…');
        out
    } else {
        collapsed
    }
}

fn has(out: &[LintFinding], id: &str) -> bool {
    out.iter().any(|f| f.id == id)
}

/// Runs every check on an HTML artifact. Checks are independent; the
/// order matches the original so findings line up with its output.
#[must_use]
pub fn lint_artifact(raw_html: &str) -> Vec<LintFinding> {
    use LintSeverity::{P0, P1, P2};
    let mut out = Vec::new();
    if raw_html.is_empty() {
        return out;
    }
    // Comments often hold pedagogical examples; never lint them.
    let html = HTML_COMMENT.replace_all(raw_html, "");
    let html = html.as_ref();

    for hex in PURPLE_HEXES {
        let re = Regex::new(&format!(
            r"(?i)linear-gradient\([^)]*{}[^)]*\)",
            regex::escape(hex)
        ))
        .expect("escaped hex");
        if let Some(m) = re.find(html) {
            out.push(finding(
                P0,
                "purple-gradient",
                format!("Found a violet/purple gradient using {hex}: the anti-slop list says no."),
                "Replace the gradient with a flat surface (var(--bg) or var(--surface)) or use the active accent at a single intensity, not in a gradient.",
                Some(clip(m.as_str())),
            ));
            break;
        }
    }
    if !has(&out, "purple-gradient") {
        if let Some(c) = PURPLE_KEYWORD_GRADIENT.captures(html) {
            out.push(finding(
                P0,
                "purple-gradient",
                format!("Found a \"{}\" keyword inside a gradient: anti-slop.", &c[1]),
                "Remove the gradient or swap to a single solid color from the active design tokens.",
                Some(clip(&c[0])),
            ));
        }
    }
    if !has(&out, "purple-gradient") {
        if let Some(gradient) = blue_cyan_gradient(html) {
            out.push(finding(
                P0,
                "trust-gradient",
                "Found a blue→cyan two-stop \"trust\" gradient: the anti-slop list says no."
                    .to_owned(),
                "Replace the gradient with a flat surface (var(--bg) or var(--surface)) or use a single design-token color. Two-stop blue→cyan trust gradients are a SaaS hero cliché.",
                Some(clip(gradient)),
            ));
        }
        let scoped = strip_token_blocks(html);
        let lower = scoped.to_ascii_lowercase();
        if let Some(hex) = AI_DEFAULT_INDIGO.iter().find(|h| lower.contains(*h)) {
            out.push(finding(
                P0,
                "ai-default-indigo",
                format!("Found a default LLM accent color ({hex}): the most-reported AI design tell."),
                "Replace with var(--accent) from the active DESIGN.md. If the brief truly requires indigo, encode it as the design system's accent so it reads as intentional, not default.",
                Some((*hex).to_owned()),
            ));
        }
    }

    for emoji in SLOP_EMOJI.iter().filter(|e| html.contains(**e)) {
        let re = Regex::new(&format!(
            r#"(?i)<(?:h[1-6]|button|li|span class="[^"]*icon[^"]*")[^>]*>[^<]*{}"#,
            regex::escape(emoji)
        ))
        .expect("escaped emoji");
        if let Some(m) = re.find(html) {
            out.push(finding(
                P0,
                "emoji-icon",
                format!("Emoji \"{emoji}\" used as a UI icon: the anti-slop list says SVG monoline only."),
                "Replace with a small inline SVG icon (1.6–1.8px stroke, currentColor) or remove the icon entirely.",
                Some(clip(m.as_str())),
            ));
            break;
        }
    }

    if let Some(m) = LEFT_ACCENT_CARD.find(html) {
        out.push(finding(
            P0,
            "left-accent-card",
            "Rounded card with a coloured left border: the canonical AI-slop card pattern."
                .to_owned(),
            "Drop either the border-radius (set 0px) or the border-left. Cards use hairline borders all-round, no left accent.",
            Some(clip(m.as_str())),
        ));
    }

    if let Some(m) = DISPLAY_SANS.find(html) {
        out.push(finding(
            P0,
            "sans-display",
            "A heading rule uses Inter / Roboto / system-sans as the display face, not the serif the seed binds.".to_owned(),
            "Use `font-family: var(--font-display)` on h1/h2/h3 and let the active design system pick the serif. Override only if the active direction is \"tech / utility\" or \"modern minimal\".",
            Some(clip(m.as_str())),
        ));
    }

    if let Some(m) = INVENTED_METRICS.iter().find_map(|re| re.find(html)) {
        out.push(finding(
            P0,
            "invented-metric",
            format!(
                "Suspected invented metric: \"{}\". The anti-slop list says: no numbers without a real source.",
                m.as_str()
            ),
            "Either remove the claim or replace with a placeholder (— or a labelled stub) until the user supplies a real number.",
            Some(clip(m.as_str())),
        ));
    }

    if let Some(m) = FILLER.iter().find_map(|re| re.find(html)) {
        out.push(finding(
            P0,
            "filler-copy",
            format!(
                "Filler copy detected: \"{}\". Pages should ship with real, brief-derived copy.",
                m.as_str()
            ),
            "Replace with copy specific to the brief or delete the section entirely. An empty section is a design problem to solve with composition, not by inventing words.",
            Some(clip(m.as_str())),
        ));
    }

    if SCROLL_INTO_VIEW.is_match(html) {
        out.push(finding(
            P0,
            "scroll-into-view",
            "Element.scrollIntoView() detected: it yanks the host page across the iframe boundary."
                .to_owned(),
            "Use `scrollTo({ left, top, behavior: \"smooth\" })` on the actual scroller.",
            None,
        ));
    }

    let scopes = extract_css_tokens(html);
    'blocks: for block in STYLE_BLOCK.captures_iter(html) {
        let css = CSS_COMMENT.replace_all(&block[1], "");
        for rule in UPPERCASE_RULE.captures_iter(&css) {
            let selector = rule[1].trim();
            let body = &rule[2];
            if !has_adequate_uppercase_tracking(body, &scopes) {
                out.push(finding(
                    P1,
                    "all-caps-no-tracking",
                    format!(
                        "Selector `{}` sets text-transform: uppercase without sufficient letter-spacing (≥0.06em).",
                        selector.chars().take(60).collect::<String>()
                    ),
                    "Add `letter-spacing: 0.08em` (typical) to the same rule. ALL CAPS without tracking looks cramped.",
                    Some(clip(&format!("{selector} {{ {} }}", body.trim()))),
                ));
                break 'blocks;
            }
        }
    }
    if !has(&out, "all-caps-no-tracking") {
        let mut inline: Vec<(usize, &str)> = INLINE_STYLE_DQ
            .captures_iter(html)
            .chain(INLINE_STYLE_SQ.captures_iter(html))
            .filter_map(|c| c.get(1).map(|m| (m.start(), m.as_str())))
            .collect();
        inline.sort_by_key(|(at, _)| *at);
        if let Some((_, decl)) = inline.into_iter().find(|(_, d)| {
            UPPERCASE_DECL.is_match(d) && !has_adequate_uppercase_tracking(d, &scopes)
        }) {
            out.push(finding(
                P1,
                "all-caps-no-tracking",
                "Inline style sets text-transform: uppercase without sufficient letter-spacing (≥0.06em).".to_owned(),
                "Add `letter-spacing: 0.08em` (typical) to the same inline style. ALL CAPS without tracking looks cramped.",
                Some(clip(decl.trim())),
            ));
        }
    }

    if let Some(m) = EXTERNAL_IMAGE.find(html) {
        out.push(finding(
            P1,
            "external-image",
            "External placeholder image CDN detected: fragile, looks fake when it 404s.".to_owned(),
            "Use a local placeholder (a flat block with a label) or a project image instead.",
            Some(clip(m.as_str())),
        ));
    }

    if let Some(first) = STYLE_BLOCK.captures(html) {
        let css = ROOT_RULE.replace_all(&first[1], "");
        let hexes: Vec<&str> = HEX_COLOR.find_iter(&css).map(|m| m.as_str()).collect();
        if hexes.len() > MAX_RAW_HEX {
            out.push(finding(
                P1,
                "raw-hex",
                format!(
                    "{} raw hex values found outside :root: design tokens probably not honoured.",
                    hexes.len()
                ),
                "Move every color into the :root token block (--bg / --surface / --fg / --muted / --border / --accent) and reference via var(). Use color-mix() for derived tones.",
                Some(hexes[..6].join(" ")),
            ));
        }
    }

    let body = STYLE_ELEMENT.replace_all(html, "");
    let accents = ACCENT_VAR.find_iter(&body).count();
    if accents > MAX_INLINE_ACCENT {
        out.push(finding(
            P1,
            "accent-overuse",
            format!("var(--accent) used {accents} times inline in the body: likely overused per screen."),
            "Cap accent usage at 2 visible uses per screen (one eyebrow + one CTA, OR one accent card + one tab). Demote the rest to var(--fg) or var(--muted).",
            None,
        ));
    }

    let sections: Vec<&str> = SECTION_TAG.find_iter(html).map(|m| m.as_str()).collect();
    let tagged = sections
        .iter()
        .filter(|s| SECTION_ANCHOR.is_match(s))
        .count();
    if tagged < sections.len() {
        out.push(finding(
            P2,
            "missing-section-anchor",
            format!(
                "{} of {} <section>s lack data-od-id (or data-screen-label).",
                sections.len() - tagged,
                sections.len()
            ),
            "Add data-od-id=\"kebab-slug\" (or data-screen-label=\"01 Cover\" for slides) to every top-level <section> so comment mode can target it.",
            None,
        ));
    }

    if SLIDE_CLASS.is_match(html) {
        lint_slides(html, &mut out);
    }
    out
}

fn lint_slides(html: &str, out: &mut Vec<LintFinding>) {
    let slides: Vec<&str> = SLIDE_SECTION.find_iter(html).map(|m| m.as_str()).collect();
    let themed = slides.iter().filter(|s| SLIDE_THEME.is_match(s)).count();
    if !slides.is_empty() && themed < slides.len() {
        out.push(finding(
            LintSeverity::P0,
            "slide-theme-missing",
            format!(
                "{} of {} slides lack a theme class (light / dark / hero light / hero dark).",
                slides.len() - themed,
                slides.len()
            ),
            "Every <section class=\"slide\"> must include exactly one theme class. Audit the slide list and add light/dark/hero modifiers.",
            None,
        ));
    }
    // Dark is true, light false; untagged slides are skipped.
    let seq: Vec<bool> = slides
        .iter()
        .filter_map(|s| {
            if HERO_DARK.is_match(s) {
                Some(true)
            } else if HERO_LIGHT.is_match(s) {
                Some(false)
            } else if WORD_DARK.is_match(s) {
                Some(true)
            } else if WORD_LIGHT.is_match(s) {
                Some(false)
            } else {
                None
            }
        })
        .collect();
    if let Some(i) = seq.windows(3).position(|w| w[0] == w[1] && w[1] == w[2]) {
        out.push(finding(
            LintSeverity::P1,
            "slide-rhythm",
            format!(
                "Three same-theme slides in a row at position {}–{}: visual fatigue.",
                i + 1,
                i + 3
            ),
            "Swap the middle slide to the opposite theme (light → dark, or dark → light). For 8+ slides, mix in at least one hero light AND one hero dark.",
            None,
        ));
    }
}

/// The findings as a block for the agent's next turn, P0 first. Empty
/// when there are none.
#[must_use]
pub fn render_for_agent(findings: &[LintFinding]) -> String {
    if findings.is_empty() {
        return String::new();
    }
    let count = |s: LintSeverity| findings.iter().filter(|f| f.severity == s).count();
    let mut sorted: Vec<&LintFinding> = findings.iter().collect();
    sorted.sort_by_key(|f| f.severity);
    let mut lines = vec![
        "<artifact-lint>".to_owned(),
        "The artifact has the following anti-slop / design-token issues.".to_owned(),
        format!(
            "{} P0 (must fix), {} P1 (should fix), {} P2 (nice to have).",
            count(LintSeverity::P0),
            count(LintSeverity::P1),
            count(LintSeverity::P2)
        ),
        "Fix them in the file itself; do not write a separate explanation.".to_owned(),
        String::new(),
    ];
    for f in sorted {
        lines.push(format!("**[{:?}] {}**: {}", f.severity, f.id, f.message));
        lines.push(format!("  Fix: {}", f.fix));
        if let Some(snippet) = &f.snippet {
            lines.push(format!("  Snippet: `{snippet}`"));
        }
        lines.push(String::new());
    }
    lines.push("</artifact-lint>".to_owned());
    lines.join("\n")
}

/// The first `linear-gradient(...)` holding both a blue and a cyan stop.
fn blue_cyan_gradient(html: &str) -> Option<&str> {
    GRADIENT.find_iter(html).map(|m| m.as_str()).find(|g| {
        let lower = g.to_ascii_lowercase();
        let blue = TRUST_BLUE_HEXES.iter().any(|h| lower.contains(h)) || WORD_BLUE.is_match(&lower);
        let cyan = TRUST_CYAN_HEXES.iter().any(|h| lower.contains(h)) || WORD_CYAN.is_match(&lower);
        blue && cyan
    })
}

/// Custom properties of one global theme rule (`:root`, `html`,
/// `[data-theme=…]`…), last declaration winning.
#[derive(Debug)]
struct TokenScope {
    tokens: HashMap<String, String>,
    is_default: bool,
    theme_keys: BTreeSet<String>,
}

#[derive(Debug)]
struct Decl<'a> {
    prop: String,
    value: &'a str,
}

/// True when every theme's resolved letter-spacing meets the 0.06em
/// floor for the rule's own font size (see the original's notes: `em`
/// compares directly; `px`/`rem` against a same-rule px/rem font size;
/// an unresolvable font size fails; no font size falls back to ≥ 1px).
fn has_adequate_uppercase_tracking(body: &str, scopes: &[TokenScope]) -> bool {
    resolved_themes(scopes)
        .iter()
        .all(|theme| tracking_adequate(&resolve_vars(body, theme)))
}

fn tracking_adequate(body: &str) -> bool {
    let decls = parse_declarations(body);
    let Some(ls) = last_decl(&decls, "letter-spacing") else {
        return false;
    };
    let Some((value, unit)) = number_with_unit(&LENGTH_TRACKING, ls.value) else {
        return false;
    };
    if unit == "em" {
        return value >= 0.06;
    }
    let tracking_px = if unit == "rem" {
        value * ROOT_FONT_PX
    } else {
        value
    };
    if let Some(font_px) = font_size_px(&decls) {
        return font_px > 0.0 && tracking_px >= font_px * 0.06;
    }
    if decls.iter().any(|d| d.prop == "font-size") {
        return false;
    }
    tracking_px >= 1.0
}

fn number_with_unit(re: &Regex, value: &str) -> Option<(f64, String)> {
    let c = re.captures(value)?;
    Some((c[1].parse().ok()?, c[2].to_ascii_lowercase()))
}

fn font_size_px(decls: &[Decl<'_>]) -> Option<f64> {
    let fs = last_decl(decls, "font-size")?;
    let (value, unit) = number_with_unit(&LENGTH_FONT, fs.value)?;
    Some(if unit == "rem" {
        value * ROOT_FONT_PX
    } else {
        value
    })
}

fn last_decl<'d, 'a>(decls: &'d [Decl<'a>], prop: &str) -> Option<&'d Decl<'a>> {
    decls.iter().rev().find(|d| d.prop == prop)
}

/// `prop: value` pairs, lowercased names, custom properties skipped.
fn parse_declarations(body: &str) -> Vec<Decl<'_>> {
    body.split(';')
        .filter_map(|raw| {
            let (prop, value) = raw.split_once(':')?;
            let prop = prop.trim().to_ascii_lowercase();
            let value = value.trim();
            (!prop.is_empty() && !prop.starts_with("--") && !value.is_empty())
                .then_some(Decl { prop, value })
        })
        .collect()
}

/// One map per theme (default plus each theme selector), applying scopes
/// in source order so tokens declared together stay paired.
fn resolved_themes(scopes: &[TokenScope]) -> Vec<HashMap<String, String>> {
    let mut keys: BTreeSet<&str> = BTreeSet::from(["\0default"]);
    for scope in scopes {
        keys.extend(scope.theme_keys.iter().map(String::as_str));
    }
    keys.into_iter()
        .map(|key| {
            let mut map = HashMap::new();
            for scope in scopes {
                if scope.is_default || scope.theme_keys.contains(key) {
                    map.extend(scope.tokens.iter().map(|(k, v)| (k.clone(), v.clone())));
                }
            }
            map
        })
        .collect()
}

fn extract_css_tokens(html: &str) -> Vec<TokenScope> {
    let mut scopes = Vec::new();
    for block in STYLE_BLOCK.captures_iter(html) {
        let css = CSS_COMMENT.replace_all(&block[1], "");
        for rule in CSS_RULE.captures_iter(&css) {
            let selector = rule[1].trim();
            if !is_global_theme_scope(selector) {
                continue;
            }
            let selectors: Vec<&str> = selector
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let tokens: HashMap<String, String> = rule[2]
                .split(';')
                .map(str::trim)
                .filter_map(|d| {
                    let c = TOKEN_DECL.captures(d)?;
                    Some((c[1].to_owned(), c[2].trim().to_owned()))
                })
                .collect();
            if tokens.is_empty() {
                continue;
            }
            scopes.push(TokenScope {
                tokens,
                is_default: selectors.iter().any(|s| BARE_GLOBAL.is_match(s)),
                theme_keys: selectors
                    .iter()
                    .filter(|s| !BARE_GLOBAL.is_match(s))
                    .map(|s| (*s).to_owned())
                    .collect(),
            });
        }
    }
    scopes
}

/// Replaces `var(--x)` / `var(--x, fallback)` with token values, a few
/// hops deep.
fn resolve_vars(body: &str, tokens: &HashMap<String, String>) -> String {
    let mut out = body.to_owned();
    for _ in 0..VAR_RESOLVE_MAX_DEPTH {
        let next = VAR_REF
            .replace_all(&out, |c: &Captures<'_>| {
                if let Some(v) = tokens.get(&c[1]) {
                    v.clone()
                } else if let Some(fallback) = c.get(2) {
                    fallback.as_str().trim().to_owned()
                } else {
                    c[0].to_owned()
                }
            })
            .into_owned();
        if next == out {
            break;
        }
        out = next;
    }
    out
}

/// Drops global token blocks (only custom properties, no indigo laundered
/// through a name other than `--accent`) before the indigo scan: a design
/// system that picks indigo as its accent is intentional.
fn strip_token_blocks(html: &str) -> String {
    STYLE_BLOCK_PARTS
        .replace_all(html, |c: &Captures<'_>| {
            let css = CSS_COMMENT.replace_all(&c[2], "");
            let stripped = CSS_RULE.replace_all(&css, |r: &Captures<'_>| {
                let decls: Vec<&str> = r[2]
                    .split(';')
                    .map(str::trim)
                    .filter(|d| !d.is_empty())
                    .collect();
                let token_block = is_global_theme_scope(r[1].trim())
                    && !decls.is_empty()
                    && decls
                        .iter()
                        .all(|d| TOKEN_PROP.is_match(d) || COLOR_SCHEME_DECL.is_match(d))
                    && !decls.iter().any(|d| launders_indigo(d));
                if token_block {
                    String::new()
                } else {
                    r[0].to_owned()
                }
            });
            format!("{}{}{}", &c[1], stripped, &c[3])
        })
        .into_owned()
}

fn launders_indigo(decl: &str) -> bool {
    let Some(c) = TOKEN_DECL.captures(decl) else {
        return false;
    };
    if c[1].eq_ignore_ascii_case("--accent") {
        return false;
    }
    let value = c[2].to_ascii_lowercase();
    AI_DEFAULT_INDIGO.iter().any(|h| value.contains(h))
}

fn is_global_theme_scope(selector: &str) -> bool {
    let parts: Vec<&str> = selector
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    !parts.is_empty() && parts.iter().all(|s| is_global_theme_selector(s))
}

fn is_theme_attribute(name: &str) -> bool {
    GLOBAL_THEME_ATTRIBUTES
        .iter()
        .any(|a| a.eq_ignore_ascii_case(name))
}

fn is_global_theme_selector(s: &str) -> bool {
    if let Some(c) = THEME_TAG_SELECTOR.captures(s) {
        return c
            .get(1)
            .is_none_or(|attr| is_theme_attribute(attr.as_str()));
    }
    THEME_ATTR_SELECTOR
        .captures(s)
        .is_some_and(|c| is_theme_attribute(&c[1]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(html: &str) -> Vec<String> {
        lint_artifact(html).into_iter().map(|f| f.id).collect()
    }

    fn style(css: &str) -> String {
        format!("<style>{css}</style>")
    }

    #[test]
    fn clean_page_has_no_findings() {
        let html = "<!doctype html><style>:root{--accent:#c96442}h1{font-family:var(--font-display)}</style><section data-od-id=\"hero\"><h1>Tea</h1></section>";
        assert!(lint_artifact(html).is_empty());
        assert!(lint_artifact("").is_empty());
        assert_eq!(render_for_agent(&[]), "");
    }

    #[test]
    fn purple_gradient_hex_and_keyword() {
        let f = lint_artifact(&style(".a{background:linear-gradient(90deg,#7C3AED,#fff)}"));
        assert_eq!(f[0].id, "purple-gradient");
        assert_eq!(f[0].severity, LintSeverity::P0);
        assert!(f[0].snippet.as_deref().unwrap().contains("#7C3AED"));
        assert_eq!(
            ids(&style(".a{background:linear-gradient(red, violet)}")),
            ["purple-gradient"]
        );
        // One corrective signal: no indigo finding on top.
        assert_eq!(
            ids(&style(".a{background:linear-gradient(#6366f1,#fff)}")),
            ["purple-gradient"]
        );
    }

    #[test]
    fn trust_gradient_needs_blue_and_cyan() {
        assert_eq!(
            ids(&style(
                ".a{background:linear-gradient(90deg,#3b82f6,#06b6d4)}"
            )),
            ["trust-gradient"]
        );
        assert_eq!(
            ids(&style(".a{background:linear-gradient(90deg, blue, cyan)}")),
            ["trust-gradient"]
        );
        assert!(ids(&style(".a{background:linear-gradient(90deg,#3b82f6,#fff)}")).is_empty());
    }

    #[test]
    fn indigo_accent_allowed_only_as_the_accent_token() {
        assert!(ids(&style(":root{--accent:#6366f1}.b{color:var(--accent)}")).is_empty());
        assert!(
            ids(&style(
                "@media (prefers-color-scheme: dark){:root{--accent:#4f46e5}}"
            ))
            .is_empty()
        );
        assert_eq!(
            ids(&style(":root{--primary:#6366f1}")),
            ["ai-default-indigo"]
        );
        assert_eq!(
            ids(&style(".cta{background:#4F46E5}")),
            ["ai-default-indigo"]
        );
        assert_eq!(
            ids(&style(":root, .cta{--accent:#6366f1}")),
            ["ai-default-indigo"]
        );
        assert_eq!(
            ids(&style("[data-variant=\"x\"]{--accent:#6366f1}")),
            ["ai-default-indigo"]
        );
        assert!(ids(&style("[data-theme=\"dark\"]{--accent:#6366f1}")).is_empty());
    }

    #[test]
    fn emoji_only_in_structural_context() {
        assert_eq!(ids("<h3>🚀 Fast</h3>"), ["emoji-icon"]);
        assert_eq!(ids("<li class=x>✅ Done</li>"), ["emoji-icon"]);
        assert!(ids("<p>We love 🚀 launches</p>").is_empty());
    }

    #[test]
    fn copy_and_layout_tells() {
        assert_eq!(
            ids(&style(".card{border-left:4px solid red;border-radius:8px}")),
            ["left-accent-card"]
        );
        assert_eq!(
            ids(&style("h1{font-family:Inter,sans-serif}")),
            ["sans-display"]
        );
        assert_eq!(ids("<p>10× faster builds</p>"), ["invented-metric"]);
        assert_eq!(ids("<p>99.99% uptime</p>"), ["invented-metric"]);
        assert_eq!(ids("<p>Lorem ipsum</p>"), ["filler-copy"]);
        assert_eq!(ids("<p>Feature one</p>"), ["filler-copy"]);
        assert_eq!(
            ids("<script>el.scrollIntoView ({})</script>"),
            ["scroll-into-view"]
        );
        // Commented-out examples never fire.
        assert!(ids("<!-- Lorem ipsum <h3>🚀</h3> -->").is_empty());
    }

    #[test]
    fn uppercase_tracking() {
        let bad = style(".eyebrow{text-transform:uppercase;letter-spacing:0.02em}");
        assert_eq!(ids(&bad), ["all-caps-no-tracking"]);
        let missing = style(".eyebrow{text-transform:uppercase}");
        assert_eq!(ids(&missing), ["all-caps-no-tracking"]);
        assert!(ids(&style(".e{text-transform:uppercase;letter-spacing:.08em}")).is_empty());
        // px against a same-rule font size; last declaration wins.
        assert_eq!(
            ids(&style(
                ".d{font-size:48px;text-transform:uppercase;letter-spacing:1px}"
            )),
            ["all-caps-no-tracking"]
        );
        assert!(
            ids(&style(
                ".d{font-size:12px;text-transform:uppercase;letter-spacing:1px}"
            ))
            .is_empty()
        );
        assert_eq!(
            ids(&style(
                ".e{text-transform:uppercase;letter-spacing:.08em;letter-spacing:.02em}"
            )),
            ["all-caps-no-tracking"]
        );
        // Unresolvable font size refuses the lenient fallback.
        assert_eq!(
            ids(&style(
                ".d{font-size:2em;text-transform:uppercase;letter-spacing:2px}"
            )),
            ["all-caps-no-tracking"]
        );
        // Nested in @media: the inner rule is judged with its font size.
        assert_eq!(
            ids(&style(
                "@media (min-width:1px){.d{font-size:48px;text-transform:uppercase;letter-spacing:1px}}"
            )),
            ["all-caps-no-tracking"]
        );
        // Tokens resolve per theme; every theme must pass.
        assert!(
            ids(&style(
                ":root{--caps:0.08em}.e{text-transform:uppercase;letter-spacing:var(--caps)}"
            ))
            .is_empty()
        );
        assert_eq!(
            ids(&style(
                ":root{--caps:0.02em}[data-theme=\"dark\"]{--caps:0.08em}.e{text-transform:uppercase;letter-spacing:var(--caps)}"
            )),
            ["all-caps-no-tracking"]
        );
        assert!(
            ids(&style(
                ".e{text-transform:uppercase;letter-spacing:var(--missing, 0.1em)}"
            ))
            .is_empty()
        );
        // Commented-out rule has no effect.
        assert!(ids(&style("/* .e{text-transform:uppercase} */")).is_empty());
        // Inline styles, either quote.
        assert_eq!(
            ids("<span style=\"text-transform: uppercase\">NEW</span>"),
            ["all-caps-no-tracking"]
        );
        assert_eq!(
            ids("<span style='text-transform:uppercase'>NEW</span>"),
            ["all-caps-no-tracking"]
        );
        assert!(
            ids("<span style=\"text-transform:uppercase;letter-spacing:0.1em\">N</span>")
                .is_empty()
        );
    }

    #[test]
    fn tokens_and_accent_counts() {
        assert_eq!(
            ids("<img src=\"https://images.unsplash.com/x.jpg\">"),
            ["external-image"]
        );
        let many: String = (0..13)
            .map(|i| format!(".c{i}{{color:#a1b2c{i:x}}}"))
            .collect();
        let f = lint_artifact(&style(&many));
        assert_eq!(f[0].id, "raw-hex");
        assert!(f[0].message.starts_with("13 raw hex"));
        let ok: String = (0..12)
            .map(|i| format!(".c{i}{{color:#a1b2c{i:x}}}"))
            .collect();
        assert!(
            ids(&format!(
                "{}<style>:root{{--a:#111;--b:#222}}</style>",
                style(&ok)
            ))
            .is_empty()
        );
        let accents = "<b style=\"color:var(--accent)\"></b>".repeat(7);
        assert_eq!(ids(&accents), ["accent-overuse"]);
        assert!(ids(&"<b style=\"color:var(--accent)\"></b>".repeat(6)).is_empty());
    }

    #[test]
    fn sections_and_slides() {
        let f = lint_artifact("<section data-od-id=\"a\"></section><section></section>");
        assert_eq!(f[0].id, "missing-section-anchor");
        assert_eq!(f[0].severity, LintSeverity::P2);
        assert_eq!(
            f[0].message,
            "1 of 2 <section>s lack data-od-id (or data-screen-label)."
        );
        let deck = |classes: &[&str]| {
            classes
                .iter()
                .map(|c| format!("<section class=\"{c}\" data-screen-label=\"s\"></section>"))
                .collect::<String>()
        };
        assert_eq!(
            ids(&deck(&["slide light", "slide"])),
            ["slide-theme-missing"]
        );
        assert_eq!(
            ids(&deck(&["slide dark", "slide hero dark", "slide dark"])),
            ["slide-rhythm"]
        );
        assert!(ids(&deck(&["slide light", "slide dark", "slide light"])).is_empty());
    }

    #[test]
    fn agent_message_lists_p0_first() {
        let findings = lint_artifact(
            "<section></section><h3>🚀 Go</h3><span style=\"text-transform:uppercase\">x</span>",
        );
        let msg = render_for_agent(&findings);
        assert!(msg.starts_with("<artifact-lint>\n"));
        assert!(msg.ends_with("</artifact-lint>"));
        assert!(msg.contains("1 P0 (must fix), 1 P1 (should fix), 1 P2 (nice to have)."));
        let p0 = msg.find("[P0] emoji-icon").unwrap();
        let p1 = msg.find("[P1] all-caps-no-tracking").unwrap();
        let p2 = msg.find("[P2] missing-section-anchor").unwrap();
        assert!(p0 < p1 && p1 < p2);
        assert!(msg.contains("  Snippet: `<h3>🚀`"));
    }

    #[test]
    fn snippets_are_clipped() {
        let long = format!("<p>lorem   ipsum {}</p>", "x".repeat(400));
        assert_eq!(clip(&long).chars().count(), 198);
        assert!(clip(&long).ends_with('…'));
        assert_eq!(clip("a \n  b"), "a b");
    }
}
