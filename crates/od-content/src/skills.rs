//! Skill and design-template registry: one folder per entry with a
//! `SKILL.md`. Normalization ports the original `skills.ts` (spec 3.4).

use crate::frontmatter;
use crate::text::{js_string, sorted_dir_entries};
use od_contracts::{
    ContentSource, CritiquePolicy, Fidelity, Platform, SkillInfo, SkillMode, Surface,
};
use rayon::prelude::*;
use regex::Regex;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Deprecated ids forwarded to their current id after a rename.
pub const SKILL_ID_ALIASES: [(&str, &str); 3] = [
    ("editorial-collage", "open-design-landing"),
    ("editorial-collage-deck", "open-design-landing-deck"),
    ("taste-skill", "design-taste-frontend"),
];

/// Folder, relative to a project, where a run stages its active skills.
pub const SKILLS_CWD_ALIAS: &str = ".od-skills";

const EXAMPLE_PROMPT_MAX_CHARS: usize = 320;

/// A loaded skill: public metadata, prompt body and folder.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillEntry {
    pub info: SkillInfo,
    /// `SKILL.md` body, prefixed with the skill-root preamble when the
    /// folder ships side files.
    pub body: String,
    pub dir: PathBuf,
}

/// Rewrites a deprecated id to its canonical form.
#[must_use]
pub fn resolve_skill_id(id: &str) -> &str {
    SKILL_ID_ALIASES
        .iter()
        .find(|(old, _)| *old == id)
        .map_or(id, |(_, new)| new)
}

/// Scans `roots` in priority order (first wins on id collisions). The first
/// root is the user root when `first_is_user` is set.
#[must_use]
pub fn list_skills(roots: &[PathBuf], first_is_user: bool) -> Vec<SkillEntry> {
    // Parse every folder in parallel, then apply first-wins in scan order.
    let per_root: Vec<(ContentSource, Vec<Vec<SkillEntry>>)> = roots
        .iter()
        .enumerate()
        .map(|(idx, root)| {
            let source = if idx == 0 && first_is_user {
                ContentSource::User
            } else {
                ContentSource::BuiltIn
            };
            let dirs = sorted_dir_entries(root);
            let parsed = dirs
                .par_iter()
                .map(|(name, path)| load_skill_dir(name, path, source).unwrap_or_default())
                .collect();
            (source, parsed)
        })
        .collect();

    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (_, groups) in per_root {
        for group in groups {
            let mut entries = group.into_iter();
            let Some(parent) = entries.next() else {
                continue;
            };
            if !seen.insert(parent.info.id.clone()) {
                continue;
            }
            out.push(parent);
            for derived in entries {
                if seen.insert(derived.info.id.clone()) {
                    out.push(derived);
                }
            }
        }
    }
    let rank = |e: &SkillEntry| e.info.featured.unwrap_or(f64::INFINITY);
    out.sort_by(|a, b| {
        rank(a)
            .partial_cmp(&rank(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// Loads one folder: the parent entry followed by its derived examples.
/// `None` when there is no readable `SKILL.md` (discovery, not validation).
fn load_skill_dir(dir_name: &str, dir: &Path, source: ContentSource) -> Option<Vec<SkillEntry>> {
    let skill_path = dir.join("SKILL.md");
    if !std::fs::metadata(&skill_path).ok()?.is_file() {
        return None;
    }
    let raw = std::fs::read_to_string(&skill_path).ok()?;
    let (data, body) = frontmatter::parse(&raw).ok()?;
    let od = data.get("od").filter(|v| v.is_object());
    let od_get = |key: &str| od.and_then(|o| o.get(key));
    let od_get2 = |a: &str, b: &str| od_get(a).and_then(|v| v.get(b));

    let parent_id = match data.get("name") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => dir_name.to_owned(),
    };
    let description_value = data.get("description");
    let description_text = description_value.map(js_string).unwrap_or_default();
    let mode = normalize_mode(od_get("mode"), body, &description_text);
    let surface = normalize_surface(od_get("surface"), mode);
    let platform = normalize_platform(od_get("platform"), mode, body, &description_text);
    let scenario = normalize_scenario(od_get("scenario"), body, &description_text);
    let category = normalize_category(od_get("category"));
    let design_system_required = od_get2("design_system", "requires")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let upstream = od_get("upstream")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let preview_type = od_get2("preview", "type")
        .and_then(Value::as_str)
        .unwrap_or("html")
        .to_owned();
    let description = description_value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let display_name = localized_from_fields(data.get("en_name"), data.get("zh_name"));
    let description_i18n =
        localized_from_fields(data.get("en_description"), data.get("zh_description"));
    let example_prompt_i18n = localized_from_record(od_get("example_prompt_i18n"));
    let triggers = match data.get("triggers") {
        Some(Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    };
    let fidelity = normalize_fidelity(od_get("fidelity"));
    let speaker_notes = normalize_bool_hint(od_get("speaker_notes"));
    let animations = normalize_bool_hint(od_get("animations"));
    let example_prompt = derive_prompt(od_get("example_prompt"), description_value);
    let critique_policy = normalize_critique_policy(od_get2("critique", "policy"));
    let full_body = body_with_root_preamble(body, dir);
    let examples = collect_derived_examples(dir);

    let parent = SkillInfo {
        id: parent_id.clone(),
        name: parent_id.clone(),
        display_name,
        description: description.clone(),
        description_i18n: description_i18n.clone(),
        triggers: triggers.clone(),
        mode,
        surface,
        source,
        craft_requires: normalize_craft_requires(od_get2("craft", "requires")),
        platform,
        scenario: scenario.clone(),
        category: category.clone(),
        preview_type: preview_type.clone(),
        design_system_required,
        default_for: normalize_default_for(od_get("default_for")),
        upstream: upstream.clone(),
        featured: normalize_featured(od_get("featured")),
        fidelity,
        speaker_notes,
        animations,
        example_prompt: example_prompt.clone(),
        example_prompt_i18n: example_prompt_i18n.clone(),
        aggregates_examples: !examples.is_empty(),
        critique_policy,
    };
    let mut out = Vec::with_capacity(1 + examples.len());
    for key in examples {
        out.push(SkillEntry {
            info: SkillInfo {
                id: format!("{parent_id}:{key}"),
                name: humanize_example_name(&key),
                display_name: None,
                description: description.clone(),
                description_i18n: description_i18n.clone(),
                triggers: triggers.clone(),
                mode,
                surface,
                source,
                craft_requires: Vec::new(),
                platform,
                scenario: scenario.clone(),
                category: category.clone(),
                preview_type: preview_type.clone(),
                design_system_required,
                default_for: Vec::new(),
                upstream: upstream.clone(),
                featured: None,
                fidelity,
                speaker_notes,
                animations,
                example_prompt: example_prompt.clone(),
                example_prompt_i18n: example_prompt_i18n.clone(),
                aggregates_examples: false,
                critique_policy,
            },
            body: full_body.clone(),
            dir: dir.to_path_buf(),
        });
    }
    out.insert(
        0,
        SkillEntry {
            info: parent,
            body: full_body,
            dir: dir.to_path_buf(),
        },
    );
    Some(out)
}

/// Example keys usable in a `<parent>:<key>` id.
#[must_use]
pub fn is_safe_example_key(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with('.')
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `examples/<key>.html` files, sorted by key.
fn collect_derived_examples(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir.join("examples")) else {
        return Vec::new();
    };
    let mut keys: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let lower = name.to_lowercase();
            lower
                .ends_with(".html")
                .then(|| name[..name.len() - ".html".len()].to_owned())
        })
        .filter(|key| is_safe_example_key(key))
        .collect();
    keys.sort_by(|a, b| {
        a.to_lowercase()
            .cmp(&b.to_lowercase())
            .then_with(|| a.cmp(b))
    });
    keys
}

fn humanize_example_name(key: &str) -> String {
    key.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Splits `<parent>:<key>` into its halves.
#[must_use]
pub fn split_derived_id(id: &str) -> Option<(&str, &str)> {
    let (parent, child) = id.split_once(':')?;
    (!parent.is_empty() && is_safe_example_key(child)).then_some((parent, child))
}

/// `<folder>-<first 10 hex of sha256(abs dir)>`, the staged copy's folder.
#[must_use]
pub fn skill_cwd_alias_segment(dir: &Path) -> String {
    let folder = dir
        .file_name()
        .map_or_else(|| "skill".to_owned(), |n| n.to_string_lossy().into_owned());
    let abs = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let normalized = abs.to_string_lossy().replace('\\', "/");
    let digest = hex::encode(Sha256::digest(normalized.as_bytes()));
    format!("{folder}-{}", &digest[..10])
}

static SIDE_FILE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?-u:\b)(?:assets|references)/[A-Za-z0-9._-]+(?-u:\b)").expect("static regex")
});
static EXAMPLE_HTML_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)example\.html(?-u:\b)").expect("static regex"));

fn dir_has_attachments(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        name != "skill.md"
            && (e.file_type().is_ok_and(|t| t.is_dir())
                || [".md", ".html", ".css", ".js", ".json", ".txt"]
                    .iter()
                    .any(|ext| name.ends_with(ext)))
    })
}

/// Tells the agent where side files live: the staged copy under the
/// project (`.od-skills/<alias>/`) first, the absolute folder as fallback.
#[must_use]
pub fn body_with_root_preamble(body: &str, dir: &Path) -> String {
    if !dir_has_attachments(dir) {
        return body.to_owned();
    }
    let mut referenced: Vec<String> = SIDE_FILE_RE
        .find_iter(body)
        .map(|m| m.as_str().to_owned())
        .collect();
    if EXAMPLE_HTML_RE.is_match(body) {
        referenced.push("example.html".to_owned());
    }
    referenced.sort();
    referenced.dedup();
    let rel_root = format!("{SKILLS_CWD_ALIAS}/{}", skill_cwd_alias_segment(dir));
    let dir_str = dir.to_string_lossy();
    let mut lines = vec![
        format!("> **Skill root (relative to project):** `{rel_root}/`"),
        format!("> **Skill root (absolute fallback):** `{dir_str}`"),
        ">".to_owned(),
        "> This skill ships side files alongside `SKILL.md`. When the workflow".to_owned(),
    ];
    if let Some(first) = referenced.first() {
        lines.push(format!(
            "> below references side files such as `{first}`, prefer the\n> relative form rooted at the first path above — e.g. open `{rel_root}/{first}`."
        ));
        lines.push("> If that path is not reachable from your working directory, fall".to_owned());
        lines.push(format!(
            "> back to the absolute path: `{}`.",
            dir.join(first).to_string_lossy()
        ));
    } else {
        lines.push(
            "> below references side files, prefer the relative form rooted at the\n> first path above."
                .to_owned(),
        );
        lines.push("> If that path is not reachable from your working directory, fall".to_owned());
        lines.push("> back to the absolute skill root above.".to_owned());
    }
    lines.push("> Either form resolves to the same file; the relative form keeps you".to_owned());
    lines.push("> inside the project working directory, which is preferred.".to_owned());
    if !referenced.is_empty() {
        let list = referenced
            .iter()
            .map(|f| format!("`{f}`"))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(">".to_owned());
        lines.push(format!("> Known side files in this skill: {list}."));
    }
    let mut out = lines.join("\n");
    out.push_str("\n\n");
    out.push_str(body);
    out
}

fn slug_ok(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Trimmed, lowercased, deduplicated craft slugs.
#[must_use]
pub fn normalize_craft_slugs<'a>(values: impl IntoIterator<Item = &'a Value>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter_map(Value::as_str)
        .map(|s| s.trim().to_lowercase())
        .filter(|s| slug_ok(s) && seen.insert(s.clone()))
        .collect()
}

fn normalize_craft_requires(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => normalize_craft_slugs(items),
        _ => Vec::new(),
    }
}

fn is_falsy(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::String(s) => s.is_empty(),
        Value::Number(n) => n.as_f64() == Some(0.0),
        _ => false,
    }
}

fn normalize_default_for(value: Option<&Value>) -> Vec<String> {
    match value {
        None => Vec::new(),
        Some(v) if is_falsy(v) => Vec::new(),
        Some(Value::Array(items)) => items.iter().map(js_string).collect(),
        Some(v) => vec![js_string(v)],
    }
}

fn normalize_fidelity(value: Option<&Value>) -> Option<Fidelity> {
    match value.and_then(Value::as_str) {
        Some("wireframe") => Some(Fidelity::Wireframe),
        Some("high-fidelity") => Some(Fidelity::HighFidelity),
        _ => None,
    }
}

fn normalize_bool_hint(value: Option<&Value>) -> Option<bool> {
    match value? {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.trim().to_lowercase().as_str() {
            "true" | "yes" | "1" => Some(true),
            "false" | "no" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn localized_from_fields(
    en: Option<&Value>,
    zh: Option<&Value>,
) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (key, value) in [("en", en), ("zh-CN", zh)] {
        if let Some(s) = value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            out.insert(key.to_owned(), s.to_owned());
        }
    }
    (!out.is_empty()).then_some(out)
}

fn localized_from_record(value: Option<&Value>) -> Option<BTreeMap<String, String>> {
    let map: &Map<String, Value> = value?.as_object()?;
    let out: BTreeMap<String, String> = map
        .iter()
        .filter_map(|(k, v)| {
            let s = v.as_str()?.trim();
            (!s.is_empty()).then(|| (k.clone(), s.to_owned()))
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

fn normalize_critique_policy(value: Option<&Value>) -> Option<CritiquePolicy> {
    match value?.as_str()?.trim().to_lowercase().as_str() {
        "required" => Some(CritiquePolicy::Required),
        "opt-in" => Some(CritiquePolicy::OptIn),
        "opt-out" => Some(CritiquePolicy::OptOut),
        _ => None,
    }
}

fn normalize_featured(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Bool(true) => Some(1.0),
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) if !s.trim().is_empty() => {
            s.trim().parse::<f64>().ok().filter(|f| f.is_finite())
        }
        _ => None,
    }
}

static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("static regex"));
static SENTENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A.+?[.!?。！？](?:\s|\z)").expect("static regex"));

/// `od.example_prompt`, else the first sentence of the description
/// (at most 320 characters).
fn derive_prompt(explicit: Option<&Value>, description: Option<&Value>) -> String {
    if let Some(s) = explicit
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return s.to_owned();
    }
    let desc = description
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if desc.is_empty() {
        return String::new();
    }
    let collapsed = WS_RE.replace_all(desc, " ");
    let collapsed = collapsed.trim();
    let first = SENTENCE_RE
        .find(collapsed)
        .map(|m| m.as_str().trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(collapsed);
    first.chars().take(EXAMPLE_PROMPT_MAX_CHARS).collect()
}

macro_rules! lazy_re {
    ($name:ident, $re:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($re).expect("static regex"));
    };
}

lazy_re!(
    MODE_IMAGE,
    r"(?-u:\b)image|poster|illustration|photography|图片|海报|插画"
);
lazy_re!(
    MODE_VIDEO,
    r"(?-u:\b)video|motion|shortform|animation|视频|动效|短片"
);
lazy_re!(
    MODE_AUDIO,
    r"(?-u:\b)audio|music|jingle|tts|sound|音频|音乐|配音|音效"
);
lazy_re!(MODE_DECK, r"(?-u:\b)ppt|deck|slide|presentation|幻灯|投影");
lazy_re!(
    MODE_DS,
    r"(?-u:\b)design[- ]system|(?-u:\b)design\.md|(?-u:\b)design tokens"
);
lazy_re!(MODE_TEMPLATE, r"(?-u:\b)template(?-u:\b)");
lazy_re!(PLATFORM_MOBILE, r"mobile|phone|ios|android|手机|移动端");

fn haystack(description: &str, body: &str) -> String {
    format!("{description}\n{body}").to_lowercase()
}

/// Regex inference used when `od.mode` is missing or invalid, checked in
/// the order image, video, audio, deck, design-system, template.
#[must_use]
pub fn infer_mode(body: &str, description: &str) -> SkillMode {
    let hay = haystack(description, body);
    let rules: [(&Regex, SkillMode); 6] = [
        (&MODE_IMAGE, SkillMode::Image),
        (&MODE_VIDEO, SkillMode::Video),
        (&MODE_AUDIO, SkillMode::Audio),
        (&MODE_DECK, SkillMode::Deck),
        (&MODE_DS, SkillMode::DesignSystem),
        (&MODE_TEMPLATE, SkillMode::Template),
    ];
    rules
        .iter()
        .find(|(re, _)| re.is_match(&hay))
        .map_or(SkillMode::Prototype, |(_, mode)| *mode)
}

fn normalize_mode(value: Option<&Value>, body: &str, description: &str) -> SkillMode {
    value
        .and_then(Value::as_str)
        .and_then(SkillMode::parse)
        .unwrap_or_else(|| infer_mode(body, description))
}

fn normalize_surface(value: Option<&Value>, mode: SkillMode) -> Surface {
    if let Some(s) = value
        .and_then(Value::as_str)
        .and_then(|v| Surface::parse(&v.trim().to_lowercase()))
    {
        return s;
    }
    match mode {
        SkillMode::Image => Surface::Image,
        SkillMode::Video => Surface::Video,
        SkillMode::Audio => Surface::Audio,
        _ => Surface::Web,
    }
}

fn normalize_platform(
    value: Option<&Value>,
    mode: SkillMode,
    body: &str,
    description: &str,
) -> Option<Platform> {
    match value.and_then(Value::as_str) {
        Some("desktop") => return Some(Platform::Desktop),
        Some("mobile") => return Some(Platform::Mobile),
        _ => {}
    }
    if mode != SkillMode::Prototype {
        return None;
    }
    if PLATFORM_MOBILE.is_match(&haystack(description, body)) {
        Some(Platform::Mobile)
    } else {
        Some(Platform::Desktop)
    }
}

lazy_re!(SC_FINANCE, r"finance|invoice|expense|budget|p&l|revenue");
lazy_re!(
    SC_HR,
    r"(?-u:\b)hr(?-u:\b)|onboarding|payroll|employee|人事"
);
lazy_re!(SC_MARKETING, r"marketing|campaign|brand|landing");
lazy_re!(
    SC_ENGINEERING,
    r"runbook|incident|deploy|engineering|sre|api"
);
lazy_re!(SC_PRODUCT, r"spec|prd|roadmap|product manager|product team");
lazy_re!(SC_DESIGN, r"design system|moodboard|mockup|ui kit");
lazy_re!(SC_SALES, r"sales|quote|proposal|lead");
lazy_re!(SC_OPERATIONS, r"operations|ops|logistics|inventory");

fn normalize_scenario(value: Option<&Value>, body: &str, description: &str) -> String {
    if let Some(v) = value
        .and_then(Value::as_str)
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
    {
        return v;
    }
    let hay = haystack(description, body);
    let rules: [(&Regex, &str); 8] = [
        (&SC_FINANCE, "finance"),
        (&SC_HR, "hr"),
        (&SC_MARKETING, "marketing"),
        (&SC_ENGINEERING, "engineering"),
        (&SC_PRODUCT, "product"),
        (&SC_DESIGN, "design"),
        (&SC_SALES, "sales"),
        (&SC_OPERATIONS, "operations"),
    ];
    rules
        .iter()
        .find(|(re, _)| re.is_match(&hay))
        .map_or("general", |(_, s)| s)
        .to_owned()
}

fn normalize_category(value: Option<&Value>) -> Option<String> {
    let slug = value?.as_str()?.trim().to_lowercase();
    slug_ok(&slug).then(|| slug.chars().take(64).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mode_inference_order() {
        assert_eq!(infer_mode("make a poster", ""), SkillMode::Image);
        assert_eq!(infer_mode("a pitch deck video", ""), SkillMode::Video);
        assert_eq!(infer_mode("", "Slides for the board"), SkillMode::Deck);
        assert_eq!(infer_mode("write DESIGN.md", ""), SkillMode::DesignSystem);
        assert_eq!(infer_mode("a template", ""), SkillMode::Template);
        assert_eq!(infer_mode("templates", ""), SkillMode::Prototype);
        assert_eq!(infer_mode("landing page", ""), SkillMode::Prototype);
        // ASCII word boundary: CJK before `image` still counts as a boundary.
        assert_eq!(infer_mode("做image", ""), SkillMode::Image);
    }

    #[test]
    fn example_prompt_first_sentence() {
        let d = json!("Admin dashboard in one file.  Second sentence.");
        assert_eq!(
            derive_prompt(None, Some(&d)),
            "Admin dashboard in one file."
        );
        let explicit = json!("  Do this  ");
        assert_eq!(derive_prompt(Some(&explicit), Some(&d)), "Do this");
        let long = json!("x".repeat(400));
        assert_eq!(derive_prompt(None, Some(&long)).chars().count(), 320);
    }

    #[test]
    fn aliases_and_derived_ids() {
        assert_eq!(resolve_skill_id("taste-skill"), "design-taste-frontend");
        assert_eq!(resolve_skill_id("other"), "other");
        assert_eq!(split_derived_id("live:stock-a"), Some(("live", "stock-a")));
        assert_eq!(split_derived_id("live:../x"), None);
        assert_eq!(
            humanize_example_name("stock_portfolio-LIVE"),
            "Stock Portfolio Live"
        );
    }

    #[test]
    fn user_root_shadows_builtin_and_examples_derive() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("user");
        let builtin = tmp.path().join("builtin");
        for (root, desc) in [(&user, "user copy"), (&builtin, "built-in copy")] {
            let dir = root.join("demo");
            std::fs::create_dir_all(dir.join("examples")).unwrap();
            std::fs::write(
                dir.join("SKILL.md"),
                format!("---\nname: demo\ndescription: {desc}\nod:\n  mode: deck\n---\nbody"),
            )
            .unwrap();
            std::fs::write(dir.join("examples/b-two.html"), "<p>").unwrap();
            std::fs::write(dir.join("examples/a-one.html"), "<p>").unwrap();
        }
        let list = list_skills(&[user, builtin], true);
        let ids: Vec<_> = list.iter().map(|e| e.info.id.as_str()).collect();
        assert_eq!(ids, ["demo", "demo:a-one", "demo:b-two"]);
        assert_eq!(list[0].info.source, ContentSource::User);
        assert_eq!(list[0].info.description, "user copy");
        assert!(list[0].info.aggregates_examples);
        assert!(list[0].body.contains("Skill root"));
    }
}
