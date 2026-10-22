//! Small helpers shared by the loaders.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// JavaScript `String(value)` for front-matter values, so regex inference
/// sees the same text the original app did.
#[must_use]
pub fn js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|i| {
                if i.is_null() {
                    String::new()
                } else {
                    js_string(i)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_owned(),
    }
}

/// Child folders (or symlinks) of `root`, sorted by name. Missing root is
/// empty: discovery, not validation.
#[must_use]
pub fn sorted_dir_entries(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() || t.is_symlink()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
