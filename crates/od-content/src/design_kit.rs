//! Reading and scoring a design-system package for the kit view (spec 3.5
//! token layers): `tokens.css` parsed into tokens, the token-contract
//! score, a rebuilt `tokens.css` with A2 fallbacks, palettes from
//! `DESIGN.md`, packages from pasted CSS variables, and a showcase page
//! for packages without `components.html`.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

/// A1 identity tokens: every brand must bind these.
pub const A1_IDENTITY: [&str; 8] = [
    "--bg",
    "--surface",
    "--fg",
    "--muted",
    "--border",
    "--accent",
    "--font-display",
    "--font-body",
];

/// A1 structure tokens: type scale, leading, tracking, rhythm, container.
pub const A1_STRUCTURE: [&str; 13] = [
    "--text-xs",
    "--text-sm",
    "--text-base",
    "--text-lg",
    "--text-xl",
    "--text-2xl",
    "--text-3xl",
    "--text-4xl",
    "--leading-body",
    "--leading-tight",
    "--tracking-display",
    "--section-y-desktop",
    "--container-max",
];

/// One `--name: value` declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Token {
    pub name: String,
    pub value: String,
    /// color, font, type, space, radius, shadow, motion or other.
    pub group: &'static str,
}

static DECL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)(--[A-Za-z0-9_-]+)\s*:\s*([^;{}]+);").expect("static regex"));
static COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").expect("static regex"));
static HEX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3})\b").expect("static regex")
});
/// `- **Name** (`#hex`)`, `**Name** `#hex``, `| Name | #hex |`.
static NAMED_HEX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*[-*|]\s*\**([A-Za-z][^*|`(#\n]{0,40}?)\**\s*[|(:]?\s*`?(#[0-9a-fA-F]{6}|#[0-9a-fA-F]{3})\b")
        .expect("static regex")
});

fn group_of(name: &str, value: &str) -> &'static str {
    let n = name.trim_start_matches("--");
    let has = |p: &str| n.starts_with(p);
    if has("font") {
        "font"
    } else if has("text") || has("leading") || has("tracking") {
        "type"
    } else if has("space") || has("section") || has("container") || has("gap") {
        "space"
    } else if has("radius") {
        "radius"
    } else if has("elev") || has("shadow") || has("focus") {
        "shadow"
    } else if has("motion") || has("ease") || has("duration") {
        "motion"
    } else if is_color(value) {
        "color"
    } else {
        "other"
    }
}

fn is_color(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    v.starts_with('#')
        || v.starts_with("rgb")
        || v.starts_with("hsl")
        || v.starts_with("oklch")
        || v.starts_with("oklab")
        || v.starts_with("color-mix")
        || matches!(v.as_str(), "white" | "black" | "transparent")
}

/// Every custom property in `css`, comments stripped, first binding wins
/// (a later `.dark { … }` override does not replace the light value).
#[must_use]
pub fn parse_tokens(css: &str) -> Vec<Token> {
    let clean = COMMENT_RE.replace_all(css, "");
    let mut out: Vec<Token> = Vec::new();
    for c in DECL_RE.captures_iter(&clean) {
        let name = c[1].to_owned();
        if out.iter().any(|t| t.name == name) {
            continue;
        }
        let value = c[2].split_whitespace().collect::<Vec<_>>().join(" ");
        out.push(Token {
            group: group_of(&name, &value),
            name,
            value,
        });
    }
    out
}

/// The A2 tokens and their fallback values, from `_schema/defaults.css`.
#[must_use]
pub fn a2_defaults(defaults_css: &str) -> Vec<Token> {
    parse_tokens(defaults_css)
}

/// The token-contract report of one `tokens.css`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenScore {
    /// 0–100.
    pub score: u32,
    /// excellent (≥ 80), usable (≥ 60), needs-review (≥ 40), needs-rebuild.
    pub grade: &'static str,
    /// Share of A1 tokens bound to a real value (not `var(...)`).
    pub a1_coverage: f64,
    /// Share of A2 tokens that fall back to the schema default.
    pub fallback_ratio: f64,
    /// Share of bound A1 tokens that only alias another token.
    pub alias_ratio: f64,
    pub missing_a1: Vec<String>,
    pub missing_a2: Vec<String>,
    pub recommend_rebuild: bool,
}

fn ratio(n: usize, d: usize) -> f64 {
    if d == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)] // token counts are tiny
        let r = n as f64 / d as f64;
        r
    }
}

/// Scores `tokens` the way the original token contract does:
/// `100 · (0.7 · A1 coverage + 0.2 · (1 − fallback ratio) + 0.1 · (1 − alias ratio))`.
#[must_use]
pub fn score(tokens: &[Token], a2: &[Token]) -> TokenScore {
    let get = |n: &str| tokens.iter().find(|t| t.name == n);
    let a1: Vec<&str> = A1_IDENTITY.iter().chain(&A1_STRUCTURE).copied().collect();
    let missing_a1: Vec<String> = a1
        .iter()
        .filter(|n| get(n).is_none())
        .map(|n| (*n).to_owned())
        .collect();
    let bound: Vec<&Token> = a1.iter().filter_map(|n| get(n)).collect();
    let aliases = bound
        .iter()
        .filter(|t| t.value.trim_start().starts_with("var("))
        .count();
    let a1_coverage = ratio(bound.len() - aliases, a1.len());
    let missing_a2: Vec<String> = a2
        .iter()
        .filter(|d| get(&d.name).is_none())
        .map(|d| d.name.clone())
        .collect();
    let fallback_ratio = ratio(missing_a2.len(), a2.len());
    let alias_ratio = ratio(aliases, bound.len());
    let raw =
        100.0 * (0.7 * a1_coverage + 0.2 * (1.0 - fallback_ratio) + 0.1 * (1.0 - alias_ratio));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0..=100
    let score = raw.round().clamp(0.0, 100.0) as u32;
    let grade = match score {
        80.. => "excellent",
        60..=79 => "usable",
        40..=59 => "needs-review",
        _ => "needs-rebuild",
    };
    let weak_identity = ["--bg", "--surface", "--fg", "--accent"]
        .iter()
        .any(|n| get(n).is_none_or(|t| t.value.trim_start().starts_with("var(")));
    TokenScore {
        score,
        grade,
        a1_coverage,
        fallback_ratio,
        alias_ratio,
        recommend_rebuild: matches!(grade, "needs-review" | "needs-rebuild")
            || a1_coverage < 0.7
            || fallback_ratio > 0.4
            || weak_identity,
        missing_a1,
        missing_a2,
    }
}

/// Values for missing A1 structure tokens (identity tokens are never
/// guessed: they carry the brand).
fn structure_fallback(name: &str) -> Option<&'static str> {
    Some(match name {
        "--text-xs" => "12px",
        "--text-sm" => "14px",
        "--text-base" => "16px",
        "--text-lg" => "18px",
        "--text-xl" => "20px",
        "--text-2xl" => "24px",
        "--text-3xl" => "32px",
        "--text-4xl" => "44px",
        "--leading-body" => "1.5",
        "--leading-tight" => "1.15",
        "--tracking-display" => "-0.01em",
        "--section-y-desktop" => "96px",
        "--container-max" => "1200px",
        "--muted" => "color-mix(in oklab, var(--fg), transparent 40%)",
        "--border" => "color-mix(in oklab, var(--fg), transparent 85%)",
        _ => return None,
    })
}

/// `css` plus one appended `:root` block binding every missing A2 token to
/// its schema default and missing structure tokens to neutral values.
/// `None` when nothing is missing.
#[must_use]
pub fn rebuild_tokens(css: &str, a2: &[Token]) -> Option<String> {
    let have = parse_tokens(css);
    let has = |n: &str| have.iter().any(|t| t.name == n);
    let mut lines: Vec<String> = Vec::new();
    for name in A1_IDENTITY.iter().chain(&A1_STRUCTURE) {
        if !has(name) {
            if let Some(v) = structure_fallback(name) {
                lines.push(format!("  {name}: {v};"));
            }
        }
    }
    for d in a2 {
        if !has(&d.name) {
            lines.push(format!("  {}: {};", d.name, d.value));
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "{}\n\n/* Token contract rebuild: schema fallbacks for missing tokens. */\n:root {{\n{}\n}}\n",
        css.trim_end(),
        lines.join("\n")
    ))
}

/// A named colour from `DESIGN.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Swatch {
    pub name: String,
    pub hex: String,
}

/// Named colours from `DESIGN.md` bullets and tables, in order, without
/// repeats; when none are named, the first bare hex values.
#[must_use]
pub fn palette_from_markdown(md: &str) -> Vec<Swatch> {
    fn push(out: &mut Vec<Swatch>, name: String, hex: &str) {
        let hex = hex.to_ascii_lowercase();
        if out.len() < 24 && !out.iter().any(|s| s.hex == hex) {
            out.push(Swatch { name, hex });
        }
    }
    let mut out: Vec<Swatch> = Vec::new();
    for c in NAMED_HEX_RE.captures_iter(md) {
        let name = c[1].trim().trim_end_matches(':').trim().to_owned();
        push(&mut out, name, &c[2]);
    }
    if out.is_empty() {
        for m in HEX_RE.find_iter(md).take(12) {
            push(&mut out, m.as_str().to_owned(), m.as_str());
        }
    }
    out
}

/// shadcn / Tailwind theme variable → package token.
const CSS_VAR_MAP: [(&str, &str); 12] = [
    ("--background", "--bg"),
    ("--card", "--surface"),
    ("--foreground", "--fg"),
    ("--muted-foreground", "--muted"),
    ("--border", "--border"),
    ("--primary", "--accent"),
    ("--primary-foreground", "--accent-on"),
    ("--destructive", "--danger"),
    ("--font-sans", "--font-body"),
    ("--font-heading", "--font-display"),
    ("--font-mono", "--font-mono"),
    ("--radius", "--radius-md"),
];

/// A bare shadcn HSL triple (`222.2 84% 4.9%`) as `hsl(…)`.
fn css_value(v: &str) -> String {
    let parts: Vec<&str> = v.split_whitespace().collect();
    let numeric = parts.len() == 3
        && parts[0].parse::<f64>().is_ok()
        && parts[1..].iter().all(|p| p.ends_with('%'));
    if numeric {
        format!("hsl({v})")
    } else {
        v.to_owned()
    }
}

/// `tokens.css` from pasted CSS variables (a shadcn theme, Tailwind v4
/// `@theme`, or plain `:root`): theme names are mapped, package names kept,
/// a missing display font follows the body font. `None` without any of the
/// colour identity tokens.
#[must_use]
pub fn tokens_from_css_vars(css: &str) -> Option<String> {
    fn set(out: &mut Vec<(String, String)>, name: &str, value: String) {
        if !out.iter().any(|(n, _)| n == name) {
            out.push((name.to_owned(), value));
        }
    }
    fn has(out: &[(String, String)], n: &str) -> bool {
        out.iter().any(|(k, _)| k == n)
    }
    let src = parse_tokens(css);
    let mut out: Vec<(String, String)> = Vec::new();
    for t in &src {
        if A1_IDENTITY.contains(&t.name.as_str()) || A1_STRUCTURE.contains(&t.name.as_str()) {
            set(&mut out, &t.name, css_value(&t.value));
        }
    }
    for (from, to) in CSS_VAR_MAP {
        if let Some(t) = src.iter().find(|t| t.name == from) {
            set(&mut out, to, css_value(&t.value));
        }
    }
    if !["--bg", "--fg", "--accent"].iter().all(|n| has(&out, n)) {
        return None;
    }
    if !has(&out, "--surface") {
        set(&mut out, "--surface", "var(--bg)".to_owned());
    }
    if !has(&out, "--font-body") {
        let system = "system-ui, -apple-system, \"Segoe UI\", sans-serif".to_owned();
        set(&mut out, "--font-body", system);
    }
    if !has(&out, "--font-display") {
        set(&mut out, "--font-display", "var(--font-body)".to_owned());
    }
    let body: Vec<String> = out.iter().map(|(n, v)| format!("  {n}: {v};")).collect();
    Some(format!(":root {{\n{}\n}}\n", body.join("\n")))
}

/// `tokens.css` from the palette of a pasted `DESIGN.md`: the first named
/// colours become background, foreground and accent by lightness.
#[must_use]
pub fn tokens_from_palette(palette: &[Swatch]) -> Option<String> {
    let lum = |hex: &str| -> Option<f64> {
        let h = hex.trim_start_matches('#');
        let h = if h.len() == 3 {
            h.chars().flat_map(|c| [c, c]).collect::<String>()
        } else {
            h.get(..6)?.to_owned()
        };
        let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(f64::from);
        Some(0.2126 * v(0)? + 0.7152 * v(2)? + 0.0722 * v(4)?)
    };
    let mut lit: Vec<(f64, &Swatch)> = palette
        .iter()
        .filter_map(|s| lum(&s.hex).map(|l| (l, s)))
        .collect();
    if lit.len() < 2 {
        return None;
    }
    lit.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let bg = lit[0].1.hex.clone();
    let fg = lit[lit.len() - 1].1.hex.clone();
    // The accent: the first named colour that is neither extreme.
    let accent = palette
        .iter()
        .find(|s| s.hex != bg && s.hex != fg)
        .map_or_else(|| fg.clone(), |s| s.hex.clone());
    let surface = lit.get(1).map_or_else(|| bg.clone(), |s| s.1.hex.clone());
    Some(format!(
        ":root {{\n  --bg: {bg};\n  --surface: {surface};\n  --fg: {fg};\n  --accent: {accent};\n  \
         --font-display: system-ui, -apple-system, \"Segoe UI\", sans-serif;\n  \
         --font-body: system-ui, -apple-system, \"Segoe UI\", sans-serif;\n}}\n"
    ))
}

/// A `DESIGN.md` for a package imported from tokens only.
#[must_use]
pub fn design_md_from_tokens(name: &str, tokens: &[Token]) -> String {
    let get = |n: &str| {
        tokens
            .iter()
            .find(|t| t.name == n)
            .map_or("not set", |t| t.value.as_str())
    };
    let colors: Vec<String> = tokens
        .iter()
        .filter(|t| t.group == "color")
        .map(|t| format!("- **{}** `{}`", t.name.trim_start_matches("--"), t.value))
        .collect();
    format!(
        "# {name}\n\n> Category: Custom\n\nImported from CSS variables.\n\n\
         ## 1. Visual Theme & Atmosphere\n\nDefined by the imported theme variables.\n\n\
         ## 2. Color\n\n{colors}\n\n\
         ## 3. Typography\n\n- Display: {display}\n- Body: {body}\n\n\
         ## 4. Spacing & Grid\n\nUse the `--space-*` scale.\n\n\
         ## 5. Layout & Composition\n\nContent sits on `--bg`; cards on `--surface`.\n\n\
         ## 6. Components\n\nButtons use `--accent` with `--accent-on` text; radius `{radius}`.\n\n\
         ## 7. Motion & Interaction\n\nShort transitions with `--motion-fast`.\n\n\
         ## 8. Voice & Brand\n\nPlain, direct copy.\n\n\
         ## 9. Anti-patterns\n\nNo colours outside the tokens.\n",
        colors = colors.join("\n"),
        display = get("--font-display"),
        body = get("--font-body"),
        radius = get("--radius-md"),
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A product-page showcase built from the tokens, for packages that ship
/// no `components.html`: palette, type scale, buttons, a card, a form.
#[must_use]
pub fn showcase_html(title: &str, tokens_css: &str) -> String {
    let tokens = parse_tokens(tokens_css);
    let colors: String = tokens
        .iter()
        .filter(|t| t.group == "color")
        .take(16)
        .map(|t| {
            format!(
                "<figure><span style=\"background:var({n})\"></span><figcaption>{n}<br><code>{v}</code></figcaption></figure>",
                n = esc(&t.name),
                v = esc(&t.value)
            )
        })
        .collect();
    let scale: String = [
        "--text-4xl",
        "--text-2xl",
        "--text-xl",
        "--text-base",
        "--text-sm",
    ]
    .iter()
    .map(|n| format!("<p style=\"font-size:var({n},16px)\">{n} The quick brown fox</p>"))
    .collect();
    let title = esc(title);
    let mut css = String::new();
    for t in &tokens {
        css.push_str(&format!("{}:{};", t.name, t.value));
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title><style>\
         :root{{{css}}}\
         body{{margin:0;background:var(--bg,#fff);color:var(--fg,#111);font-family:var(--font-body,system-ui);line-height:var(--leading-body,1.5)}}\
         main{{max-width:var(--container-max,1100px);margin:0 auto;padding:48px 32px}}\
         h1,h2{{font-family:var(--font-display,inherit);letter-spacing:var(--tracking-display,0)}}\
         h1{{font-size:var(--text-4xl,44px);margin:0 0 8px}}h2{{font-size:var(--text-xl,20px);margin:40px 0 16px}}\
         .lead{{color:var(--muted,inherit);font-size:var(--text-lg,18px)}}\
         .swatches{{display:grid;grid-template-columns:repeat(auto-fill,minmax(130px,1fr));gap:12px}}\
         figure{{margin:0;font-size:12px}}figure span{{display:block;height:64px;border-radius:var(--radius-md,10px);border:1px solid var(--border,#ddd)}}\
         .row{{display:flex;gap:12px;flex-wrap:wrap;align-items:center}}\
         .btn{{font:inherit;padding:10px 18px;border-radius:var(--radius-md,8px);border:1px solid var(--border,#ccc);background:var(--surface,#fff);color:var(--fg,#111)}}\
         .btn.primary{{background:var(--accent,#333);color:var(--accent-on,#fff);border-color:transparent}}\
         .card{{background:var(--surface,#fff);border:1px solid var(--border,#ddd);border-radius:var(--radius-lg,14px);padding:24px;box-shadow:var(--elev-raised,none);max-width:420px}}\
         input{{font:inherit;padding:10px 12px;border-radius:var(--radius-sm,6px);border:1px solid var(--border,#ccc);background:var(--bg,#fff);color:var(--fg,#111)}}\
         </style></head><body><main>\
         <h1>{title}</h1><p class=\"lead\">A showcase built from this system's tokens.</p>\
         <h2>Palette</h2><div class=\"swatches\">{colors}</div>\
         <h2>Type scale</h2>{scale}\
         <h2>Buttons</h2><div class=\"row\"><button class=\"btn primary\">Primary action</button><button class=\"btn\">Secondary</button></div>\
         <h2>Card</h2><div class=\"card\"><strong>Card title</strong><p class=\"lead\">Supporting text on the surface colour.</p><button class=\"btn primary\">Continue</button></div>\
         <h2>Form</h2><div class=\"row\"><input placeholder=\"Email address\"><button class=\"btn primary\">Subscribe</button></div>\
         </main></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS: &str =
        ":root { --accent-on: #fff; --space-1: 4px; --radius-sm: 8px; --motion-fast: 150ms; }";

    #[test]
    fn parses_tokens_without_comments_first_binding_wins() {
        let t = parse_tokens(
            "/* --fake: 1; */ :root { --bg: #fff; --font-body: Inter,\n  sans-serif; --space-2: 8px; }\n.dark { --bg: #000; }",
        );
        let names: Vec<&str> = t.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["--bg", "--font-body", "--space-2"]);
        assert_eq!(t[0].value, "#fff");
        assert_eq!(t[0].group, "color");
        assert_eq!(t[1].value, "Inter, sans-serif");
        assert_eq!(t[1].group, "font");
        assert_eq!(t[2].group, "space");
    }

    #[test]
    fn score_rewards_coverage_and_flags_aliases() {
        let a2 = a2_defaults(DEFAULTS);
        let mut full = String::from(":root {");
        for n in A1_IDENTITY.iter().chain(&A1_STRUCTURE) {
            full.push_str(&format!("{n}: #123456;"));
        }
        for d in &a2 {
            full.push_str(&format!("{}: {};", d.name, d.value));
        }
        full.push('}');
        let s = score(&parse_tokens(&full), &a2);
        assert_eq!(s.score, 100);
        assert_eq!(s.grade, "excellent");
        assert!(!s.recommend_rebuild);
        assert!(s.missing_a1.is_empty() && s.missing_a2.is_empty());

        let thin =
            parse_tokens(":root { --bg: #fff; --fg: #000; --surface: #eee; --accent: red; }");
        let s = score(&thin, &a2);
        // 4 of 21 A1 bound, every A2 missing, no alias: 70·4/21 + 10.
        assert_eq!(s.score, 23);
        assert_eq!(s.grade, "needs-rebuild");
        assert!(s.recommend_rebuild);
        assert_eq!(s.missing_a2.len(), 4);
        let aliased =
            parse_tokens(":root { --bg: #fff; --fg: #000; --surface: var(--bg); --accent: red; }");
        let s = score(&aliased, &a2);
        assert!((s.alias_ratio - 0.25).abs() < 1e-9);
        assert!(s.recommend_rebuild, "an aliased surface is weak identity");
    }

    #[test]
    fn rebuild_appends_only_missing_tokens() {
        let a2 = a2_defaults(DEFAULTS);
        let css = ":root { --bg: #fff; --space-1: 2px; }";
        let rebuilt = rebuild_tokens(css, &a2).unwrap();
        assert!(rebuilt.starts_with(css));
        assert!(rebuilt.contains("--accent-on: #fff;"));
        assert!(
            !rebuilt.contains("--space-1: 4px"),
            "a bound A2 token is kept"
        );
        assert!(rebuilt.contains("--text-base: 16px;"));
        assert!(!rebuilt.contains("--accent: "), "identity is never guessed");
        assert_eq!(
            rebuild_tokens(&rebuilt, &a2),
            None,
            "a second rebuild has nothing to add"
        );
    }

    #[test]
    fn palette_reads_named_bullets_and_tables() {
        let md = "## Color\n- **Rausch** (`#ff385c`): brand.\n- **Ink** `#222222`\n| Leaf | #2F6B3A |\nText #abcdef.\n";
        let p = palette_from_markdown(md);
        let got: Vec<(&str, &str)> = p
            .iter()
            .map(|s| (s.name.as_str(), s.hex.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("Rausch", "#ff385c"),
                ("Ink", "#222222"),
                ("Leaf", "#2f6b3a")
            ]
        );
        let bare = palette_from_markdown("Use #fff and #000000.");
        assert_eq!(bare.len(), 2);
    }

    #[test]
    fn shadcn_theme_maps_to_package_tokens() {
        let css = ":root { --background: 0 0% 100%; --foreground: 222.2 84% 4.9%; --primary: 221 83% 53%; --radius: 0.5rem; --border: 214 32% 91%; }\n.dark { --background: 0 0% 0%; }";
        let out = tokens_from_css_vars(css).unwrap();
        assert!(out.contains("--bg: hsl(0 0% 100%);"));
        assert!(out.contains("--fg: hsl(222.2 84% 4.9%);"));
        assert!(out.contains("--accent: hsl(221 83% 53%);"));
        assert!(out.contains("--radius-md: 0.5rem;"));
        assert!(out.contains("--surface: var(--bg);"));
        assert!(out.contains("--font-display: var(--font-body);"));
        assert!(tokens_from_css_vars(":root { --x: 1; }").is_none());
    }

    #[test]
    fn palette_tokens_pick_light_dark_and_accent() {
        let p = palette_from_markdown(
            "- **Cream** `#f6efe2`\n- **Leaf** `#2f6b3a`\n- **Ink** `#1d1b16`\n",
        );
        let css = tokens_from_palette(&p).unwrap();
        assert!(css.contains("--bg: #f6efe2;"));
        assert!(css.contains("--fg: #1d1b16;"));
        assert!(css.contains("--accent: #2f6b3a;"));
    }

    #[test]
    fn showcase_escapes_and_uses_tokens() {
        let html = showcase_html("<Tea>", ":root { --bg: #fff; --accent: #2f6b3a; }");
        assert!(html.contains("&lt;Tea&gt;"));
        assert!(html.contains("--accent:#2f6b3a;"));
        assert!(html.contains("background:var(--accent)"));
    }
}
