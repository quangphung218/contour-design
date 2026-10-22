//! Live artifact records (`artifact.json`), inputs and the bounded-JSON
//! rules shared by data, sources and outputs. Field names follow the
//! original app's contracts (camelCase on disk and on the wire).

use crate::LiveError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const SCHEMA_VERSION: u32 = 1;
pub const TEMPLATE_FILE: &str = "template.html";
pub const PREVIEW_FILE: &str = "index.html";
pub const DATA_FILE: &str = "data.json";

/// Bounds on any JSON a live artifact stores (original
/// `LIVE_ARTIFACT_BOUNDED_JSON_CONSTRAINTS`).
pub const MAX_DEPTH: usize = 8;
pub const MAX_OBJECT_KEYS: usize = 100;
pub const MAX_ARRAY_LEN: usize = 500;
pub const MAX_STRING_CHARS: usize = 16 * 1024;
pub const MAX_SERIALIZED_BYTES: usize = 256 * 1024;
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_MAPPING_PATHS: usize = 100;

/// Keys that would smuggle raw responses or credentials into stored data.
const FORBIDDEN_KEYS: [&str; 11] = [
    "raw",
    "rawresponse",
    "payload",
    "body",
    "headers",
    "cookie",
    "authorization",
    "token",
    "secret",
    "credential",
    "password",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Active,
    Archived,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RefreshStatus {
    Never,
    #[default]
    Idle,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preview {
    #[serde(rename = "type")]
    pub kind: PreviewType,
    pub entry: String,
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            kind: PreviewType::Html,
            entry: PREVIEW_FILE.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewType {
    Html,
    Jsx,
    Markdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    LocalFile,
    DaemonTool,
    ConnectorTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshPermission {
    None,
    ManualRefreshGrantedForReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transform {
    #[default]
    Identity,
    CompactTable,
    MetricSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataPath {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputMapping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_paths: Option<Vec<DataPath>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<Transform>,
}

/// Where a refresh reads from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Source {
    #[serde(rename = "type")]
    pub kind: SourceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub input: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connector: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_mapping: Option<OutputMapping>,
    pub refresh_permission: RefreshPermission,
}

fn format_default() -> String {
    crate::render::RENDER_FORMAT.to_owned()
}
fn template_default() -> String {
    TEMPLATE_FILE.to_owned()
}
fn preview_default() -> String {
    PREVIEW_FILE.to_owned()
}
fn data_default() -> String {
    DATA_FILE.to_owned()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    #[serde(default = "format_default")]
    pub format: String,
    #[serde(default = "template_default")]
    pub template_path: String,
    #[serde(default = "preview_default")]
    pub generated_preview_path: String,
    #[serde(default = "data_default")]
    pub data_path: String,
    #[serde(default)]
    pub data_json: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_schema_json: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_json: Option<Source>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            format: format_default(),
            template_path: template_default(),
            generated_preview_path: preview_default(),
            data_path: data_default(),
            data_json: Map::new(),
            data_schema_json: None,
            source_json: None,
        }
    }
}

/// `artifact.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveArtifact {
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by_run_id: Option<String>,
    pub title: String,
    pub slug: String,
    pub status: Status,
    pub pinned: bool,
    pub preview: Preview,
    pub refresh_status: RefreshStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<String>,
    pub document: Document,
}

/// A listed artifact: everything but the document.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by_run_id: Option<String>,
    pub title: String,
    pub slug: String,
    pub status: Status,
    pub pinned: bool,
    pub preview: Preview,
    pub refresh_status: RefreshStatus,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<String>,
    pub has_document: bool,
    /// Whether a manual refresh is possible (a source that allows it).
    pub refreshable: bool,
}

impl From<&LiveArtifact> for Summary {
    fn from(a: &LiveArtifact) -> Self {
        Self {
            schema_version: a.schema_version,
            id: a.id.clone(),
            project_id: a.project_id.clone(),
            session_id: a.session_id.clone(),
            created_by_run_id: a.created_by_run_id.clone(),
            title: a.title.clone(),
            slug: a.slug.clone(),
            status: a.status,
            pinned: a.pinned,
            preview: a.preview.clone(),
            refresh_status: a.refresh_status,
            created_at: a.created_at.clone(),
            updated_at: a.updated_at.clone(),
            last_refreshed_at: a.last_refreshed_at.clone(),
            has_document: true,
            refreshable: a.document.source_json.as_ref().is_some_and(|s| {
                s.refresh_permission == RefreshPermission::ManualRefreshGrantedForReadOnly
            }),
        }
    }
}

/// `LiveArtifactCreateInput`. Daemon-owned fields (`id`, `projectId`,
/// timestamps, `refreshStatus`…) are unknown fields and refused.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateInput {
    pub title: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub preview: Option<Preview>,
    #[serde(default)]
    pub document: Option<Document>,
}

/// `LiveArtifactUpdateInput`: every field optional.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateInput {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub preview: Option<Preview>,
    #[serde(default)]
    pub document: Option<Document>,
}

fn invalid(path: &str, message: impl std::fmt::Display) -> LiveError {
    LiveError::Validation(format!("{path}: {message}"))
}

/// Checks depth, sizes, forbidden keys and total serialized size.
///
/// # Errors
/// [`LiveError::Validation`] naming the offending path.
pub fn validate_bounded(value: &Value, path: &str) -> Result<(), LiveError> {
    fn walk(value: &Value, path: &str, depth: usize) -> Result<(), LiveError> {
        if depth > MAX_DEPTH {
            return Err(invalid(path, format!("nesting deeper than {MAX_DEPTH}")));
        }
        match value {
            Value::String(s) if s.chars().count() > MAX_STRING_CHARS => Err(invalid(
                path,
                format!("string longer than {MAX_STRING_CHARS} characters"),
            )),
            Value::Number(n) if n.as_f64().is_none_or(|f| !f.is_finite()) => {
                Err(invalid(path, "numbers must be finite"))
            }
            Value::Array(items) => {
                if items.len() > MAX_ARRAY_LEN {
                    return Err(invalid(path, format!("more than {MAX_ARRAY_LEN} items")));
                }
                items
                    .iter()
                    .enumerate()
                    .try_for_each(|(i, v)| walk(v, &format!("{path}.{i}"), depth + 1))
            }
            Value::Object(map) => {
                if map.len() > MAX_OBJECT_KEYS {
                    return Err(invalid(path, format!("more than {MAX_OBJECT_KEYS} keys")));
                }
                map.iter().try_for_each(|(k, v)| {
                    let lower = k.to_ascii_lowercase();
                    if FORBIDDEN_KEYS.contains(&lower.as_str()) || lower == "__proto__" {
                        return Err(invalid(&format!("{path}.{k}"), "key is not allowed"));
                    }
                    walk(v, &format!("{path}.{k}"), depth + 1)
                })
            }
            _ => Ok(()),
        }
    }
    walk(value, path, 0)?;
    let size = serde_json::to_vec(value).map_or(usize::MAX, |b| b.len());
    if size > MAX_SERIALIZED_BYTES {
        return Err(invalid(
            path,
            format!("serialized size {size} exceeds {MAX_SERIALIZED_BYTES} bytes"),
        ));
    }
    Ok(())
}

/// A dot path for output mapping (`$.` prefix allowed); no empty,
/// `__proto__`, `constructor` or `prototype` segments.
///
/// # Errors
/// [`LiveError::Validation`].
pub fn mapping_segments<'a>(value: &'a str, path: &str) -> Result<Vec<&'a str>, LiveError> {
    let normalized = value.strip_prefix("$.").unwrap_or(value);
    let segments: Vec<&str> = normalized.split('.').collect();
    let ok = !normalized.is_empty()
        && segments.iter().all(|s| {
            !s.is_empty()
                && s.len() <= 128
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                && !["__proto__", "constructor", "prototype"].contains(s)
        });
    if ok {
        Ok(segments)
    } else {
        Err(invalid(path, "must be a dot-separated JSON path"))
    }
}

/// Validates a document as supplied by an agent.
///
/// # Errors
/// [`LiveError::Validation`].
pub fn validate_document(doc: &Document) -> Result<(), LiveError> {
    let fixed = [
        (
            "document.format",
            doc.format.as_str(),
            crate::render::RENDER_FORMAT,
        ),
        ("document.templatePath", &doc.template_path, TEMPLATE_FILE),
        (
            "document.generatedPreviewPath",
            &doc.generated_preview_path,
            PREVIEW_FILE,
        ),
        ("document.dataPath", &doc.data_path, DATA_FILE),
    ];
    for (path, got, want) in fixed {
        if got != want {
            return Err(invalid(path, format!("must be {want:?}")));
        }
    }
    validate_bounded(&Value::Object(doc.data_json.clone()), "document.dataJson")?;
    if let Some(schema) = &doc.data_schema_json {
        validate_bounded(&Value::Object(schema.clone()), "document.dataSchemaJson")?;
    }
    if let Some(source) = &doc.source_json {
        validate_source(source)?;
    }
    Ok(())
}

fn validate_source(source: &Source) -> Result<(), LiveError> {
    validate_bounded(
        &Value::Object(source.input.clone()),
        "document.sourceJson.input",
    )?;
    match source.kind {
        SourceType::DaemonTool if source.tool_name.is_none() => {
            return Err(invalid(
                "document.sourceJson.toolName",
                "is required for daemon_tool sources",
            ));
        }
        SourceType::ConnectorTool if source.connector.is_none() => {
            return Err(invalid(
                "document.sourceJson.connector",
                "is required for connector_tool sources",
            ));
        }
        _ => {}
    }
    if let Some(mapping) = &source.output_mapping {
        let paths = mapping.data_paths.as_deref().unwrap_or_default();
        if paths.len() > MAX_MAPPING_PATHS {
            return Err(invalid(
                "document.sourceJson.outputMapping.dataPaths",
                "must be a bounded array",
            ));
        }
        for (i, p) in paths.iter().enumerate() {
            let at = format!("document.sourceJson.outputMapping.dataPaths.{i}");
            mapping_segments(&p.from, &format!("{at}.from"))?;
            mapping_segments(&p.to, &format!("{at}.to"))?;
        }
    }
    Ok(())
}

/// Title rules: non-empty after trimming, at most 200 characters.
///
/// # Errors
/// [`LiveError::Validation`].
pub fn validate_title(title: &str) -> Result<String, LiveError> {
    let t = title.trim();
    if t.is_empty() || t.chars().count() > MAX_TITLE_CHARS {
        return Err(invalid(
            "title",
            format!("must be 1–{MAX_TITLE_CHARS} characters"),
        ));
    }
    Ok(t.to_owned())
}

/// Lowercase ASCII slug of `input` (accents dropped), `live-artifact`
/// when nothing is left, at most 128 bytes cut at a `-`.
#[must_use]
pub fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for c in input.chars().flat_map(fold_accent) {
        if c.is_ascii_alphanumeric() {
            if dash && !slug.is_empty() {
                slug.push('-');
            }
            dash = false;
            slug.push(c.to_ascii_lowercase());
        } else {
            dash = true;
        }
    }
    if slug.is_empty() {
        slug.push_str("live-artifact");
    }
    truncate_slug(&slug, 128)
}

/// Truncates at a segment boundary when one is close enough.
fn truncate_slug(slug: &str, max: usize) -> String {
    if slug.len() <= max {
        return slug.to_owned();
    }
    let cut = &slug[..max];
    match cut.rfind('-') {
        Some(i) if i > 0 => cut[..i].to_owned(),
        _ => cut.trim_end_matches('-').to_owned(),
    }
}

/// Latin letters with diacritics → base letter (NFKD + strip marks for
/// the common cases, including Vietnamese).
fn fold_accent(c: char) -> impl Iterator<Item = char> {
    const MAP: [(&str, char); 14] = [
        ("àáâãäåāăąạảấầẩẫậắằẳẵặ", 'a'),
        ("ÀÁÂÃÄÅĀĂĄẠẢẤẦẨẪẬẮẰẲẴẶ", 'A'),
        ("èéêëēĕėęěẹẻẽếềểễệ", 'e'),
        ("ÈÉÊËĒĔĖĘĚẸẺẼẾỀỂỄỆ", 'E'),
        ("ìíîïĩīĭįỉị", 'i'),
        ("ÌÍÎÏĨĪĬĮỈỊ", 'I'),
        ("òóôõöōŏőơọỏốồổỗộớờởỡợ", 'o'),
        ("ÒÓÔÕÖŌŎŐƠỌỎỐỒỔỖỘỚỜỞỠỢ", 'O'),
        ("ùúûüũūŭůűųưụủứừửữự", 'u'),
        ("ÙÚÛÜŨŪŬŮŰŲƯỤỦỨỪỬỮỰ", 'U'),
        ("ýÿỳỵỷỹ", 'y'),
        ("ÝŸỲỴỶỸ", 'Y'),
        ("đ", 'd'),
        ("Đ", 'D'),
    ];
    let folded = MAP
        .iter()
        .find(|(set, _)| set.contains(c))
        .map_or(c, |(_, base)| *base);
    std::iter::once(folded)
}

/// `la-<slug>-<12 hex>`, at most 128 bytes.
#[must_use]
pub fn new_artifact_id(title: &str, slug: Option<&str>) -> String {
    let suffix: String = uuid::Uuid::new_v4().simple().to_string()[..12].to_owned();
    let max_slug = 128 - "la".len() - suffix.len() - 2;
    let slug = truncate_slug(&slugify(slug.unwrap_or(title)), max_slug);
    format!("la-{slug}-{suffix}")
}

/// Storage ids: `[A-Za-z0-9][A-Za-z0-9._-]{0,127}`, not `.`/`..`.
#[must_use]
pub fn is_safe_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c))
}

/// UTC now as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
#[must_use]
pub fn now_iso() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    iso_from_ms(i64::try_from(ms).unwrap_or(i64::MAX))
}

/// Formats Unix milliseconds (civil-from-days, proleptic Gregorian).
#[must_use]
pub fn iso_from_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        tod / 3600,
        tod % 3600 / 60,
        tod % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slugs_and_ids() {
        assert_eq!(slugify("Báo cáo Doanh thu — Q3!"), "bao-cao-doanh-thu-q3");
        assert_eq!(slugify("***"), "live-artifact");
        let id = new_artifact_id("Weekly KPIs", None);
        assert!(id.starts_with("la-weekly-kpis-") && id.len() == "la-weekly-kpis-".len() + 12);
        assert!(is_safe_id(&id));
        assert!(!is_safe_id("../x") && !is_safe_id(".") && !is_safe_id(""));
    }

    #[test]
    fn iso_timestamps() {
        assert_eq!(iso_from_ms(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_from_ms(1_761_193_665_123), "2025-10-23T04:27:45.123Z");
    }

    #[test]
    fn bounded_json() {
        assert!(validate_bounded(&json!({ "a": [1, "x", { "b": null }] }), "d").is_ok());
        assert!(validate_bounded(&json!({ "Token": "x" }), "d").is_err());
        assert!(validate_bounded(&json!({ "a": "x".repeat(MAX_STRING_CHARS + 1) }), "d").is_err());
        let mut deep = json!(1);
        for _ in 0..=MAX_DEPTH {
            deep = json!({ "a": deep });
        }
        assert!(validate_bounded(&deep, "d").is_err());
        assert!(mapping_segments("$.json.items", "p").is_ok());
        assert!(mapping_segments("a..b", "p").is_err());
        assert!(mapping_segments("a.__proto__", "p").is_err());
    }
}
