//! Media prompt templates (spec 3.6): `prompt-templates/{image,video}/*.json`
//! with id, surface, title, summary, category, tags, model, aspect, prompt
//! and `source` (repo, license, author, url) for attribution. Validation
//! follows the original loader; invalid files are skipped.
//!
//! Preview URLs point at third-party CDNs; they are not exposed (no
//! outbound network from the app).

use serde::Serialize;
use serde_json::Value;
use std::path::Path;

pub const SURFACES: [&str; 2] = ["image", "video"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemplateSource {
    pub repo: String,
    pub license: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptTemplate {
    pub id: String,
    pub surface: &'static str,
    pub title: String,
    pub summary: String,
    pub category: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect: Option<String>,
    pub prompt: String,
    pub source: TemplateSource,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}

/// Validates one parsed file (`file_stem` = name without `.json`).
#[must_use]
pub fn validate(raw: &Value, surface: &'static str, file_stem: &str) -> Option<PromptTemplate> {
    let id = s(raw, "id").filter(|i| !i.is_empty() && i == file_stem)?;
    if raw.get("surface").and_then(Value::as_str) != Some(surface) {
        return None;
    }
    let title = s(raw, "title")
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())?;
    let prompt = s(raw, "prompt")
        .map(|p| p.trim().to_owned())
        .filter(|p| p.chars().count() >= 20)?;
    let source = raw.get("source")?;
    Some(PromptTemplate {
        id,
        surface,
        title,
        summary: s(raw, "summary")
            .map(|x| x.trim().to_owned())
            .unwrap_or_default(),
        category: s(raw, "category").unwrap_or_else(|| "General".to_owned()),
        tags: raw
            .get("tags")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| t.as_str().map(str::to_owned))
            .collect(),
        model: s(raw, "model"),
        aspect: s(raw, "aspect"),
        prompt,
        source: TemplateSource {
            repo: s(source, "repo")?,
            license: s(source, "license")?,
            author: s(source, "author"),
            url: s(source, "url"),
        },
    })
}

/// Every valid template, image first, then by title.
#[must_use]
pub fn list_prompt_templates(root: &Path) -> Vec<PromptTemplate> {
    let mut out = Vec::new();
    for surface in SURFACES {
        let Ok(entries) = std::fs::read_dir(root.join(surface)) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".json") else {
                continue;
            };
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let parsed = std::fs::read(entry.path())
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            match parsed.and_then(|v| validate(&v, surface, stem)) {
                Some(t) => out.push(t),
                None => {
                    tracing::warn!(file = %entry.path().display(), "skipping invalid prompt template")
                }
            }
        }
    }
    out.sort_by(|a, b| {
        (a.surface != "image", a.title.to_lowercase())
            .cmp(&(b.surface != "image", b.title.to_lowercase()))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validation_rules() {
        let ok = json!({ "id": "a", "surface": "image", "title": " T ", "prompt": "x".repeat(20),
                         "source": { "repo": "r", "license": "MIT" }, "previewImageUrl": "https://cdn/x.png" });
        let t = validate(&ok, "image", "a").unwrap();
        assert_eq!((t.title.as_str(), t.category.as_str()), ("T", "General"));
        assert!(!serde_json::to_string(&t).unwrap().contains("cdn"));
        assert!(
            validate(&ok, "image", "b").is_none(),
            "id must match the file name"
        );
        assert!(
            validate(&ok, "video", "a").is_none(),
            "surface must match the folder"
        );
        let mut short = ok.clone();
        short["prompt"] = json!("too short");
        assert!(validate(&short, "image", "a").is_none());
        let mut no_license = ok;
        no_license["source"] = json!({ "repo": "r" });
        assert!(validate(&no_license, "image", "a").is_none());
    }
}
