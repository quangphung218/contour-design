//! One-file HTML export: walks the entry's dependency graph and embeds every
//! same-project asset as a `data:` URL (spec 1.3, 4.2).
//!
//! - HTML is rewritten with `lol_html` (streaming, no DOM).
//! - CSS `url()` and `@import` are embedded recursively; an import cycle is an
//!   error.
//! - ES modules are rewritten to `od-project:/<path>` specifiers and resolved
//!   through one generated import map, so module cycles keep working.
//! - Remote (`http:`, `https:`, `//`) references stay external and are listed
//!   in a JSON manifest inside the output.
//!
//! Limits (spec 4.2): entry 2 MiB, 2,500 graph nodes, 50 MiB per asset,
//! 100 MiB output. Exceeding one is [`StandaloneError::LimitExceeded`], which
//! the HTTP layer maps to 413.

use base64::Engine;
use lol_html::html_content::{ContentType, Element};
use lol_html::{RewriteStrSettings, element, rewrite_str, text};
use regex::Regex;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

pub const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_GRAPH_NODES: usize = 2_500;
pub const MAX_ASSET_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: u64 = 100 * 1024 * 1024;

/// Specifier prefix for embedded project modules, mapped by the import map.
pub const PROJECT_MODULE_PREFIX: &str = "od-project:/";
/// `id` of the JSON manifest listing external dependencies.
pub const MANIFEST_ID: &str = "od-standalone-manifest";

const PLACEHOLDER: &str = "<!--od-standalone-head-7f3a-->";
/// Nested HTML (iframes) deeper than this is left external-free but unembedded.
const MAX_HTML_DEPTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub entry_bytes: u64,
    pub graph_nodes: usize,
    pub asset_bytes: u64,
    pub output_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            entry_bytes: MAX_ENTRY_BYTES,
            graph_nodes: MAX_GRAPH_NODES,
            asset_bytes: MAX_ASSET_BYTES,
            output_bytes: MAX_OUTPUT_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub limits: Limits,
    /// Leave missing local references untouched instead of failing. Used when
    /// the bundle only feeds the renderer, where a broken image beats no
    /// export at all.
    pub lenient: bool,
}

/// What [`AssetSource::stat`] reports for one project file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetMeta {
    pub mime: String,
    pub size: u64,
}

/// Read access to project files by normalized project path (`a/b.css`).
pub trait AssetSource {
    /// `Ok(None)` when the file does not exist.
    ///
    /// # Errors
    /// I/O or path-safety errors.
    fn stat(&self, path: &str) -> std::io::Result<Option<AssetMeta>>;

    /// # Errors
    /// I/O errors.
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>>;
}

#[derive(Debug, thiserror::Error)]
pub enum StandaloneError {
    #[error("{limit} {actual} exceeds the limit of {max}")]
    LimitExceeded {
        limit: &'static str,
        actual: u64,
        max: u64,
    },
    #[error("missing local dependency {dependency} (referenced from {})", chain.join(" -> "))]
    MissingDependency {
        dependency: String,
        chain: Vec<String>,
    },
    #[error("{dependency} points outside the project")]
    PathOutsideProject { dependency: String },
    #[error("dependency cycle: {}", chain.join(" -> "))]
    DependencyCycle { chain: Vec<String> },
    #[error("invalid source {path}: {message}")]
    InvalidSource { path: String, message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl StandaloneError {
    /// Stable kind for the HTTP error details.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::LimitExceeded { .. } => "limit-exceeded",
            Self::MissingDependency { .. } => "missing-local-dependency",
            Self::PathOutsideProject { .. } => "path-outside-project",
            Self::DependencyCycle { .. } => "dependency-cycle",
            Self::InvalidSource { .. } => "invalid-source",
            Self::Io(_) => "io",
        }
    }
}

pub type Result<T> = std::result::Result<T, StandaloneError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandaloneHtml {
    pub html: String,
    /// Remote URLs left as-is, sorted and deduplicated.
    pub external_dependencies: Vec<String>,
    /// Number of distinct project files embedded.
    pub embedded: usize,
}

/// Bundles `html` (the content of project file `entry_path`) into one file.
///
/// # Errors
/// See [`StandaloneError`].
pub fn bundle(
    entry_path: &str,
    html: &str,
    source: &dyn AssetSource,
    options: &Options,
) -> Result<StandaloneHtml> {
    let entry = normalize(entry_path).ok_or_else(|| StandaloneError::PathOutsideProject {
        dependency: entry_path.to_owned(),
    })?;
    let limits = options.limits;
    let entry_bytes = html.len() as u64;
    if entry_bytes > limits.entry_bytes {
        return Err(StandaloneError::LimitExceeded {
            limit: "entryBytes",
            actual: entry_bytes,
            max: limits.entry_bytes,
        });
    }
    let bundler = Bundler {
        source,
        options: *options,
        nodes: HashSet::from([entry.clone()]),
        cache: HashMap::new(),
        modules: BTreeMap::new(),
        external: Vec::new(),
        stack: vec![entry.clone()],
        output_budget: entry_bytes,
    };
    let cell = RefCell::new(bundler);
    let body = rewrite_html(&cell, &entry, html, 0)?;
    let mut bundler = cell.into_inner();
    let head = bundler.head_block();
    let html = insert_head(&body, &head);
    let out = html.len() as u64;
    if out > limits.output_bytes {
        return Err(StandaloneError::LimitExceeded {
            limit: "outputBytes",
            actual: out,
            max: limits.output_bytes,
        });
    }
    bundler.external.sort();
    bundler.external.dedup();
    Ok(StandaloneHtml {
        html,
        external_dependencies: bundler.external,
        embedded: bundler.nodes.len() - 1,
    })
}

// ---------------------------------------------------------------------------
// Paths and references

/// Collapses `.`/`..` in a `/`-separated project path. `None` when it climbs
/// above the project root.
fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn percent_decode(raw: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        char::from(b)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
    }
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| raw.to_owned())
}

#[derive(Debug, PartialEq, Eq)]
enum Reference {
    /// Nothing to do (`#frag`, `data:`, `javascript:`, empty).
    Keep,
    External(String),
    Local {
        path: String,
        fragment: String,
    },
}

static SCHEME_RE: LazyLock<Regex> = LazyLock::new(|| {
    #[allow(clippy::unwrap_used)] // literal pattern
    Regex::new(r"^[A-Za-z][A-Za-z0-9+.-]*:").unwrap()
});

/// Classifies `raw` as written in a file whose directory is `base_dir`.
fn classify(raw: &str, base_dir: &str) -> std::result::Result<Reference, String> {
    let value = raw.trim();
    if value.is_empty() || value.starts_with('#') {
        return Ok(Reference::Keep);
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("http:") || lower.starts_with("https:") || value.starts_with("//") {
        return Ok(Reference::External(value.to_owned()));
    }
    if SCHEME_RE.is_match(value) {
        // data:, blob:, javascript:, mailto:, od-project:, …
        return Ok(Reference::Keep);
    }
    let (rest, fragment) = match value.find('#') {
        Some(i) => (&value[..i], value[i..].to_owned()),
        None => (value, String::new()),
    };
    let rest = rest.split('?').next().unwrap_or("");
    if rest.is_empty() {
        return Ok(Reference::Keep);
    }
    let decoded = percent_decode(rest);
    let joined = if let Some(abs) = decoded.strip_prefix('/') {
        abs.to_owned()
    } else if base_dir.is_empty() {
        decoded
    } else {
        format!("{base_dir}/{decoded}")
    };
    match normalize(&joined) {
        Some(path) if !path.is_empty() => Ok(Reference::Local { path, fragment }),
        Some(_) => Ok(Reference::Keep),
        None => Err(value.to_owned()),
    }
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{b64}")
}

fn base_mime(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

// ---------------------------------------------------------------------------
// Bundler

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AssetKind {
    Raw,
    Css,
    Html,
    ClassicScript,
}

struct Bundler<'a> {
    source: &'a dyn AssetSource,
    options: Options,
    /// Distinct project files visited (graph nodes).
    nodes: HashSet<String>,
    cache: HashMap<(String, AssetKind), String>,
    /// path → processed module data URL; `None` while in progress.
    modules: BTreeMap<String, Option<String>>,
    external: Vec<String>,
    /// Chain of files being processed, for error messages and CSS cycles.
    stack: Vec<String>,
    output_budget: u64,
}

impl Bundler<'_> {
    fn limits(&self) -> Limits {
        self.options.limits
    }

    fn visit(&mut self, path: &str) -> Result<()> {
        if self.nodes.insert(path.to_owned()) && self.nodes.len() > self.limits().graph_nodes {
            return Err(StandaloneError::LimitExceeded {
                limit: "graphNodes",
                actual: self.nodes.len() as u64,
                max: self.limits().graph_nodes as u64,
            });
        }
        Ok(())
    }

    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.output_budget += bytes as u64;
        let max = self.limits().output_bytes;
        if self.output_budget > max {
            return Err(StandaloneError::LimitExceeded {
                limit: "outputBytes",
                actual: self.output_budget,
                max,
            });
        }
        Ok(())
    }

    /// Stats and reads one local file. `Ok(None)`: missing and lenient.
    fn load(&mut self, path: &str) -> Result<Option<(String, Vec<u8>)>> {
        let Some(meta) = self.source.stat(path)? else {
            if self.options.lenient {
                return Ok(None);
            }
            return Err(StandaloneError::MissingDependency {
                dependency: path.to_owned(),
                chain: self.stack.clone(),
            });
        };
        let max = self.limits().asset_bytes;
        if meta.size > max {
            return Err(StandaloneError::LimitExceeded {
                limit: "assetBytes",
                actual: meta.size,
                max,
            });
        }
        self.visit(path)?;
        let bytes = self.source.read(path)?;
        Ok(Some((base_mime(&meta.mime), bytes)))
    }

    /// Resolves one reference from a file in `base_dir`; returns the value
    /// to write back, or `None` to keep the original.
    fn embed_ref(&mut self, raw: &str, base_dir: &str, kind: AssetKind) -> Result<Option<String>> {
        match classify(raw, base_dir) {
            Ok(Reference::Keep) => Ok(None),
            Ok(Reference::External(url)) => {
                self.external.push(url);
                Ok(None)
            }
            Ok(Reference::Local { path, fragment }) => Ok(self
                .embed_path(&path, kind)?
                .map(|url| format!("{url}{fragment}"))),
            Err(dep) => {
                if self.options.lenient {
                    Ok(Some("data:,".to_owned()))
                } else {
                    Err(StandaloneError::PathOutsideProject { dependency: dep })
                }
            }
        }
    }

    fn embed_path(&mut self, path: &str, kind: AssetKind) -> Result<Option<String>> {
        let key = (path.to_owned(), kind);
        if let Some(hit) = self.cache.get(&key) {
            return Ok(Some(hit.clone()));
        }
        let Some((mime, bytes)) = self.load(path)? else {
            return Ok(None);
        };
        let is_css = mime == "text/css" || path.ends_with(".css");
        let url = match kind {
            AssetKind::Css if is_css => {
                if self.stack.iter().any(|p| p == path) {
                    let mut chain = self.stack.clone();
                    chain.push(path.to_owned());
                    return Err(StandaloneError::DependencyCycle { chain });
                }
                let css = utf8(path, bytes)?;
                self.stack.push(path.to_owned());
                let out = self.rewrite_css(&css, dir_of(path));
                self.stack.pop();
                data_url("text/css;charset=utf-8", out?.as_bytes())
            }
            AssetKind::Html if mime == "text/html" && self.stack.len() <= MAX_HTML_DEPTH => {
                if self.stack.iter().any(|p| p == path) {
                    // A page framing itself: leave the frame empty.
                    return Ok(Some("about:blank".to_owned()));
                }
                let html = utf8(path, bytes)?;
                let nested = self.nested_html(path, &html)?;
                data_url("text/html;charset=utf-8", nested.as_bytes())
            }
            AssetKind::ClassicScript => data_url("text/javascript", &bytes),
            _ => data_url(
                if mime.is_empty() {
                    "application/octet-stream"
                } else {
                    &mime
                },
                &bytes,
            ),
        };
        // Nested assets travel inside their parent's URL; charge only what
        // lands in the top-level document.
        if self.stack.len() == 1 {
            self.charge(url.len())?;
        }
        self.cache.insert(key, url.clone());
        Ok(Some(url))
    }

    /// A framed page gets its own complete bundle (own import map).
    fn nested_html(&mut self, path: &str, html: &str) -> Result<String> {
        let saved_modules = std::mem::take(&mut self.modules);
        let saved_external = self.external.len();
        self.stack.push(path.to_owned());
        let inner = RefCell::new(Bundler {
            source: self.source,
            options: self.options,
            nodes: std::mem::take(&mut self.nodes),
            cache: std::mem::take(&mut self.cache),
            modules: BTreeMap::new(),
            external: Vec::new(),
            stack: std::mem::take(&mut self.stack),
            output_budget: self.output_budget,
        });
        let body = rewrite_html(&inner, path, html, self.stack.len());
        let mut inner = inner.into_inner();
        let head = inner.head_block();
        self.nodes = std::mem::take(&mut inner.nodes);
        self.cache = std::mem::take(&mut inner.cache);
        self.stack = std::mem::take(&mut inner.stack);
        self.output_budget = inner.output_budget;
        self.external.truncate(saved_external);
        self.external.append(&mut inner.external);
        self.modules = saved_modules;
        self.stack.pop();
        Ok(insert_head(&body?, &head))
    }

    fn embed_module(&mut self, path: &str) -> Result<Option<String>> {
        if let Some(entry) = self.modules.get(path) {
            // In progress (a cycle) or done: the import map resolves it.
            return Ok(entry.clone().or_else(|| Some(String::new())));
        }
        let Some((_, bytes)) = self.load(path)? else {
            return Ok(None);
        };
        self.modules.insert(path.to_owned(), None);
        let js = utf8(path, bytes)?;
        self.stack.push(path.to_owned());
        let out = self.rewrite_module(&js, dir_of(path));
        self.stack.pop();
        let url = data_url("text/javascript", out?.as_bytes());
        self.charge(url.len())?;
        self.modules.insert(path.to_owned(), Some(url.clone()));
        Ok(Some(url))
    }

    /// Rewrites a module specifier; `None` keeps it.
    fn module_specifier(&mut self, spec: &str, base_dir: &str) -> Result<Option<String>> {
        let relative = spec.starts_with("./") || spec.starts_with("../") || spec.starts_with('/');
        if !relative {
            if matches!(classify(spec, base_dir), Ok(Reference::External(_))) {
                self.external.push(spec.to_owned());
            }
            return Ok(None);
        }
        match classify(spec, base_dir) {
            Ok(Reference::Local { path, .. }) => Ok(self
                .embed_module(&path)?
                .map(|_| format!("{PROJECT_MODULE_PREFIX}{path}"))),
            Ok(_) => Ok(None),
            Err(_) if self.options.lenient => Ok(Some("data:text/javascript,".to_owned())),
            Err(dep) => Err(StandaloneError::PathOutsideProject { dependency: dep }),
        }
    }

    fn rewrite_module(&mut self, js: &str, base_dir: &str) -> Result<String> {
        replace_all(&MODULE_RE, js, |caps| {
            let (Some(whole), Some(spec)) = (caps.get(0), caps.get(3).or_else(|| caps.get(5)))
            else {
                return Ok(None);
            };
            let Some(new_spec) = self.module_specifier(spec.as_str(), base_dir)? else {
                return Ok(None);
            };
            let text = whole.as_str();
            let (start, end) = (spec.start() - whole.start(), spec.end() - whole.start());
            Ok(Some(format!(
                "{}{new_spec}{}",
                &text[..start],
                &text[end..]
            )))
        })
    }

    fn rewrite_css(&mut self, css: &str, base_dir: &str) -> Result<String> {
        let css = CSS_COMMENT_RE.replace_all(css, "");
        let css = replace_all(&CSS_IMPORT_STRING_RE, &css, |caps| {
            let raw = caps.get(2).map_or("", |m| m.as_str());
            Ok(self
                .embed_ref(raw, base_dir, AssetKind::Css)?
                .map(|url| format!("@import url({url})")))
        })?;
        replace_all(&CSS_URL_RE, &css, |caps| {
            let raw = caps
                .get(2)
                .or_else(|| caps.get(3))
                .or_else(|| caps.get(4))
                .map_or("", |m| m.as_str());
            Ok(self
                .embed_ref(raw, base_dir, AssetKind::Css)?
                .map(|url| format!("url({url})")))
        })
    }

    fn rewrite_srcset(&mut self, srcset: &str, base_dir: &str) -> Result<Option<String>> {
        let mut changed = false;
        let mut parts = Vec::new();
        for candidate in srcset.split(',') {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                continue;
            }
            let (url, descriptor) = candidate
                .split_once(char::is_whitespace)
                .map_or((candidate, ""), |(u, d)| (u, d.trim()));
            let url = match self.embed_ref(url, base_dir, AssetKind::Raw)? {
                Some(new) => {
                    changed = true;
                    new
                }
                None => url.to_owned(),
            };
            parts.push(if descriptor.is_empty() {
                url
            } else {
                format!("{url} {descriptor}")
            });
        }
        Ok(changed.then(|| parts.join(", ")))
    }

    /// Import map plus the external-dependency manifest, for `<head>`.
    fn head_block(&self) -> String {
        let mut out = String::new();
        let imports: serde_json::Map<String, serde_json::Value> = self
            .modules
            .iter()
            .filter_map(|(path, url)| {
                url.as_ref().map(|u| {
                    (
                        format!("{PROJECT_MODULE_PREFIX}{path}"),
                        serde_json::Value::String(u.clone()),
                    )
                })
            })
            .collect();
        if !imports.is_empty() {
            let map = serde_json::json!({ "imports": imports });
            out.push_str("<script type=\"importmap\">");
            out.push_str(&script_safe_json(&map));
            out.push_str("</script>");
        }
        let mut external = self.external.clone();
        external.sort();
        external.dedup();
        let manifest = serde_json::json!({ "externalDependencies": external });
        out.push_str(&format!(
            "<script type=\"application/json\" id=\"{MANIFEST_ID}\">{}</script>",
            script_safe_json(&manifest)
        ));
        out
    }
}

fn utf8(path: &str, bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| StandaloneError::InvalidSource {
        path: path.to_owned(),
        message: "not UTF-8 text".to_owned(),
    })
}

/// JSON that cannot close the surrounding `<script>`.
fn script_safe_json(value: &serde_json::Value) -> String {
    value.to_string().replace("</", "<\\/")
}

fn insert_head(body: &str, head: &str) -> String {
    if let Some(i) = body.find(PLACEHOLDER) {
        let mut out = String::with_capacity(body.len() + head.len());
        out.push_str(&body[..i]);
        out.push_str(head);
        out.push_str(&body[i + PLACEHOLDER.len()..]);
        return out;
    }
    // No <head>: after the doctype, if any.
    let at = DOCTYPE_RE.find(body).map_or(0, |m| m.end());
    format!("{}{head}{}", &body[..at], &body[at..])
}

/// `Regex::replace_all` with a fallible closure; `Ok(None)` keeps the match.
fn replace_all<F>(re: &Regex, input: &str, mut f: F) -> Result<String>
where
    F: FnMut(&regex::Captures<'_>) -> Result<Option<String>>,
{
    let mut out = String::with_capacity(input.len());
    let mut last = 0;
    for caps in re.captures_iter(input) {
        let Some(m) = caps.get(0) else { continue };
        out.push_str(&input[last..m.start()]);
        match f(&caps)? {
            Some(replacement) => out.push_str(&replacement),
            None => out.push_str(m.as_str()),
        }
        last = m.end();
    }
    out.push_str(&input[last..]);
    Ok(out)
}

#[allow(clippy::unwrap_used)] // literal patterns
static CSS_COMMENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
#[allow(clippy::unwrap_used)]
static CSS_IMPORT_STRING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"@import\s+(["'])([^"']+)["']"#).unwrap());
#[allow(clippy::unwrap_used)]
static CSS_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)url\(\s*(?:(")([^"]*)"|'([^']*)'|([^)'"\s]+))\s*\)"#).unwrap()
});
#[allow(clippy::unwrap_used)]
static MODULE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(\bimport\s*(?:[\w$*{}\s,]+?\s*from\s*)?|\bexport\s*[\w$*{}\s,]+?\s*from\s*)(["'])([^"'\n]+)["']|\bimport\s*\(\s*(["'])([^"'\n]+)["']\s*\)"#,
    )
    .unwrap()
});
#[allow(clippy::unwrap_used)]
static DOCTYPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*<!doctype[^>]*>").unwrap());

// ---------------------------------------------------------------------------
// HTML

/// lol_html handlers cannot return our error type; the first error is kept
/// here and a marker error stops the rewrite.
struct Failure;

impl std::fmt::Debug for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("standalone bundling failed")
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}
impl std::error::Error for Failure {}

/// Attributes that load a resource, per element.
const URL_ATTRS: &[(&str, &str, AssetKind)] = &[
    ("img", "src", AssetKind::Raw),
    ("source", "src", AssetKind::Raw),
    ("video", "src", AssetKind::Raw),
    ("video", "poster", AssetKind::Raw),
    ("audio", "src", AssetKind::Raw),
    ("track", "src", AssetKind::Raw),
    ("embed", "src", AssetKind::Raw),
    ("object", "data", AssetKind::Raw),
    ("input", "src", AssetKind::Raw),
    ("image", "href", AssetKind::Raw),
    ("image", "xlink:href", AssetKind::Raw),
    ("iframe", "src", AssetKind::Html),
];

const EMBED_LINK_RELS: &[&str] = &[
    "icon",
    "shortcut",
    "apple-touch-icon",
    "apple-touch-startup-image",
    "mask-icon",
    "manifest",
    "preload",
];

fn rewrite_html(
    cell: &RefCell<Bundler<'_>>,
    owner: &str,
    html: &str,
    depth: usize,
) -> Result<String> {
    let base_dir = RefCell::new(dir_of(owner).to_owned());
    let failure: RefCell<Option<StandaloneError>> = RefCell::new(None);
    let style_buf = RefCell::new(String::new());
    let script_buf = RefCell::new(String::new());
    let head_seen = RefCell::new(false);
    let inline_module = RefCell::new(false);
    let _ = depth;

    let fail = |err: StandaloneError| -> Box<dyn std::error::Error + Send + Sync> {
        failure.borrow_mut().get_or_insert(err);
        Box::new(Failure)
    };

    let set_attr = |el: &mut Element<'_, '_>,
                    name: &str,
                    kind: AssetKind|
     -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Some(raw) = el.get_attribute(name) else {
            return Ok(());
        };
        let dir = base_dir.borrow().clone();
        let new = cell
            .borrow_mut()
            .embed_ref(&raw, &dir, kind)
            .map_err(fail)?;
        if let Some(new) = new {
            el.set_attribute(name, &new)?;
        }
        Ok(())
    };

    let mut handlers = vec![
        element!("head", |el| {
            if !*head_seen.borrow() {
                *head_seen.borrow_mut() = true;
                el.prepend(PLACEHOLDER, ContentType::Html);
            }
            Ok(())
        }),
        element!("base[href]", |el| {
            let raw = el.get_attribute("href").unwrap_or_default();
            let reference = classify(&raw, &base_dir.borrow());
            if let Ok(Reference::Local { path, .. }) = reference {
                // `<base href="assets/">` names a directory.
                let dir = if raw.trim_end().ends_with('/') {
                    path
                } else {
                    dir_of(&path).to_owned()
                };
                *base_dir.borrow_mut() = dir;
            }
            el.remove();
            Ok(())
        }),
        element!("link[href]", |el| {
            let rel = el
                .get_attribute("rel")
                .unwrap_or_default()
                .to_ascii_lowercase();
            let rels: Vec<&str> = rel.split_ascii_whitespace().collect();
            if rels.contains(&"stylesheet") {
                return set_attr(el, "href", AssetKind::Css);
            }
            if rels.contains(&"modulepreload")
                || rels.contains(&"prefetch")
                || rels.contains(&"dns-prefetch")
                || rels.contains(&"preconnect")
            {
                // Hints only; the import map already carries local modules.
                let href = el.get_attribute("href").unwrap_or_default();
                let reference = classify(&href, &base_dir.borrow());
                if matches!(reference, Ok(Reference::Local { .. }) | Err(_)) {
                    el.remove();
                }
                return Ok(());
            }
            if rels.iter().any(|r| EMBED_LINK_RELS.contains(r)) {
                return set_attr(el, "href", AssetKind::Raw);
            }
            Ok(())
        }),
        element!("script[src]", |el| {
            let module = el
                .get_attribute("type")
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("module"));
            if !module {
                return set_attr(el, "src", AssetKind::ClassicScript);
            }
            let raw = el.get_attribute("src").unwrap_or_default();
            let dir = base_dir.borrow().clone();
            let reference = classify(&raw, &dir);
            match reference {
                Ok(Reference::Local { path, .. }) => {
                    let url = cell.borrow_mut().embed_module(&path).map_err(fail)?;
                    if let Some(url) = url.filter(|u| !u.is_empty()) {
                        el.set_attribute("src", &url)?;
                    }
                }
                Ok(Reference::External(url)) => cell.borrow_mut().external.push(url),
                Ok(Reference::Keep) => {}
                Err(dep) => {
                    if cell.borrow().options.lenient {
                        el.set_attribute("src", "data:text/javascript,")?;
                    } else {
                        return Err(fail(StandaloneError::PathOutsideProject {
                            dependency: dep,
                        }));
                    }
                }
            }
            Ok(())
        }),
        element!("img[srcset], source[srcset]", |el| {
            let Some(raw) = el.get_attribute("srcset") else {
                return Ok(());
            };
            let dir = base_dir.borrow().clone();
            let new = cell.borrow_mut().rewrite_srcset(&raw, &dir).map_err(fail)?;
            if let Some(new) = new {
                el.set_attribute("srcset", &new)?;
            }
            Ok(())
        }),
        element!("[style]", |el| {
            let Some(raw) = el.get_attribute("style") else {
                return Ok(());
            };
            if !raw.to_ascii_lowercase().contains("url(") {
                return Ok(());
            }
            let dir = base_dir.borrow().clone();
            let new = cell.borrow_mut().rewrite_css(&raw, &dir).map_err(fail)?;
            el.set_attribute("style", &new)?;
            Ok(())
        }),
        text!("style", |t| {
            style_buf.borrow_mut().push_str(t.as_str());
            if t.last_in_text_node() {
                let css = std::mem::take(&mut *style_buf.borrow_mut());
                let dir = base_dir.borrow().clone();
                let new = cell.borrow_mut().rewrite_css(&css, &dir).map_err(fail)?;
                t.replace(&new, ContentType::Html);
            } else {
                t.remove();
            }
            Ok(())
        }),
        element!("script", |el| {
            // Text handlers match on the start tag only; remember whether
            // this script's body is an inline module.
            *inline_module.borrow_mut() = !el.has_attribute("src")
                && el
                    .get_attribute("type")
                    .is_some_and(|t| t.trim().eq_ignore_ascii_case("module"));
            Ok(())
        }),
        text!("script", |t| {
            if !*inline_module.borrow() {
                return Ok(());
            }
            script_buf.borrow_mut().push_str(t.as_str());
            if t.last_in_text_node() {
                let js = std::mem::take(&mut *script_buf.borrow_mut());
                let dir = base_dir.borrow().clone();
                let new = cell.borrow_mut().rewrite_module(&js, &dir).map_err(fail)?;
                t.replace(&new, ContentType::Html);
            } else {
                t.remove();
            }
            Ok(())
        }),
    ];
    for (selector, attr, kind) in URL_ATTRS {
        let (attr, kind) = (*attr, *kind);
        let set_attr = &set_attr;
        handlers.push(element!(*selector, move |el| set_attr(el, attr, kind)));
    }

    let settings = handlers.into_iter().fold(
        RewriteStrSettings::new().with_strict(false),
        RewriteStrSettings::append_element_content_handler,
    );
    let out = rewrite_str(html, settings);
    match (out, failure.into_inner()) {
        (_, Some(err)) => Err(err),
        (Ok(out), None) => Ok(out),
        (Err(err), None) => Err(StandaloneError::InvalidSource {
            path: owner.to_owned(),
            message: err.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_dots_and_rejects_escape() {
        assert_eq!(normalize("a/./b/../c.css").as_deref(), Some("a/c.css"));
        assert_eq!(normalize("../x"), None);
        assert_eq!(normalize("/a//b").as_deref(), Some("a/b"));
    }

    #[test]
    fn classify_references() {
        assert_eq!(classify("#top", "").ok(), Some(Reference::Keep));
        assert_eq!(
            classify("data:image/png;base64,AA", "").ok(),
            Some(Reference::Keep)
        );
        assert_eq!(
            classify("https://cdn.example/x.js", "").ok(),
            Some(Reference::External("https://cdn.example/x.js".to_owned()))
        );
        assert_eq!(
            classify("img/a%20b.png?v=1#f", "pages").ok(),
            Some(Reference::Local {
                path: "pages/img/a b.png".to_owned(),
                fragment: "#f".to_owned()
            })
        );
        assert_eq!(
            classify("/root.css", "pages").ok(),
            Some(Reference::Local {
                path: "root.css".to_owned(),
                fragment: String::new()
            })
        );
        assert!(classify("../../etc/passwd", "a").is_err());
    }

    #[test]
    fn percent_decode_handles_bad_escapes() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
