//! `od plugin validate <dir>` (spec 2.6): the checks a plugin folder must
//! pass before it is worth installing under `<data>/plugins/`. Loading
//! stays "discovery, not validation"; this is the strict pass.

use crate::atoms::is_known_atom;
use crate::pipeline::Pipeline;
use crate::plugins::{CLAUDE_PLUGIN_MANIFEST, MANIFEST, PluginEntry, load_plugin};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

pub const KINDS: [&str; 4] = ["skill", "scenario", "atom", "bundle"];

/// The capability vocabulary (`connector:<id>` is checked separately).
pub const CAPABILITIES: [&str; 8] = [
    "prompt:inject",
    "fs:read",
    "fs:write",
    "mcp",
    "subprocess",
    "bash",
    "network",
    "connector",
];

/// Always granted (spec 3.6: restricted plugins still inject prompts).
pub const DEFAULT_GRANTED: &str = "prompt:inject";

#[must_use]
pub fn is_known_capability(cap: &str) -> bool {
    CAPABILITIES.contains(&cap)
        || cap
            .strip_prefix("connector:")
            .is_some_and(|id| !id.is_empty() && !id.contains('*'))
}

/// Capabilities a plugin needs beyond what it was granted. Bundled
/// first-party plugins are trusted; others are restricted.
#[must_use]
pub fn missing_capabilities(required: &[String], granted: &[String], trusted: bool) -> Vec<String> {
    if trusted {
        return Vec::new();
    }
    required
        .iter()
        .filter(|c| c.as_str() != DEFAULT_GRANTED && !granted.contains(c))
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Validation {
    pub id: Option<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Validation {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Validates one plugin folder.
#[must_use]
pub fn validate_plugin_dir(dir: &Path) -> Validation {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let folder = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("plugin")
        .to_owned();
    let has_manifest = dir.join(MANIFEST).is_file() || dir.join(CLAUDE_PLUGIN_MANIFEST).is_file();
    if has_manifest {
        for name in [MANIFEST, CLAUDE_PLUGIN_MANIFEST] {
            if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
                match serde_json::from_str::<Value>(&text) {
                    Ok(Value::Object(_)) => {}
                    Ok(_) => errors.push(format!("{name} must be a JSON object")),
                    Err(e) => errors.push(format!("{name} is not valid JSON: {e}")),
                }
                break;
            }
        }
    }
    let Some(plugin) = load_plugin(dir, &folder, "user") else {
        if errors.is_empty() {
            errors.push(format!(
                "no {MANIFEST}, {CLAUDE_PLUGIN_MANIFEST} or SKILL.md in {}",
                dir.display()
            ));
        }
        return Validation {
            id: None,
            errors,
            warnings,
        };
    };
    check(&plugin, has_manifest, &mut errors, &mut warnings);
    Validation {
        id: Some(plugin.id),
        errors,
        warnings,
    }
}

fn check(
    p: &PluginEntry,
    has_manifest: bool,
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    if !has_manifest {
        warnings.push(format!("no {MANIFEST}: the plugin loads as a plain skill"));
    }
    if !p
        .id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        errors.push(format!("name {:?} must be letters, digits, - _ or .", p.id));
    }
    if !KINDS.contains(&p.kind.as_str()) {
        errors.push(format!(
            "od.kind {:?} must be one of {}",
            p.kind,
            KINDS.join(", ")
        ));
    }
    if p.version == "0.0.0" && has_manifest {
        warnings.push("no version".to_owned());
    }
    for cap in &p.capabilities {
        if !is_known_capability(cap) {
            errors.push(format!("unknown capability {cap:?}"));
        }
    }
    for atom in &p.context.atoms {
        if !is_known_atom(atom) {
            errors.push(format!("unknown atom {atom:?}"));
        }
    }
    if let Some(raw) = &p.pipeline {
        if let Err(e) = Pipeline::parse(raw) {
            errors.push(format!("od.pipeline: {e}"));
        }
    }
    for (i, field) in p.inputs.iter().enumerate() {
        if field
            .get("name")
            .and_then(Value::as_str)
            .is_none_or(|n| n.trim().is_empty())
        {
            errors.push(format!("od.inputs[{i}] has no name"));
        }
    }
    if p.body.is_empty() && p.kind != "bundle" {
        warnings.push("SKILL.md is missing or empty".to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write(dir: &Path, manifest: &Value, skill: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(MANIFEST), manifest.to_string()).unwrap();
        std::fs::write(dir.join("SKILL.md"), skill).unwrap();
    }

    #[test]
    fn a_good_plugin_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("good");
        write(
            &dir,
            &json!({"name": "good", "version": "1.0.0", "od": {
                "kind": "scenario",
                "capabilities": ["prompt:inject", "connector:slack"],
                "pipeline": {"stages": [{"id": "a", "atoms": ["todo-write"]}]},
                "inputs": [{"name": "topic"}]
            }}),
            "---\nname: good\n---\nDo it.\n",
        );
        let v = validate_plugin_dir(&dir);
        assert!(v.ok(), "{v:?}");
        assert_eq!(v.id.as_deref(), Some("good"));
        assert!(v.warnings.is_empty(), "{v:?}");
    }

    #[test]
    fn every_problem_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bad");
        write(
            &dir,
            &json!({"name": "bad one", "od": {
                "kind": "widget",
                "capabilities": ["root", "connector:*"],
                "context": {"atoms": ["nope"]},
                "pipeline": {"stages": [{"id": "x", "atoms": ["todo-write"], "repeat": true}]},
                "inputs": [{"type": "text"}]
            }}),
            "",
        );
        let v = validate_plugin_dir(&dir);
        assert_eq!(v.errors.len(), 7, "{:#?}", v.errors);
        assert!(v.errors.iter().any(|e| e.contains("repeat:true")));
        assert!(!validate_plugin_dir(&tmp.path().join("missing")).ok());
        std::fs::create_dir_all(tmp.path().join("json")).unwrap();
        std::fs::write(tmp.path().join("json").join(MANIFEST), "{").unwrap();
        let v = validate_plugin_dir(&tmp.path().join("json"));
        assert!(v.errors[0].contains("not valid JSON"), "{v:?}");
    }

    #[test]
    fn capability_gate() {
        let req = vec![
            "prompt:inject".to_owned(),
            "fs:read".to_owned(),
            "mcp".to_owned(),
        ];
        assert_eq!(
            missing_capabilities(&req, &["fs:read".to_owned()], false),
            ["mcp"]
        );
        assert!(missing_capabilities(&req, &[], true).is_empty());
        assert!(missing_capabilities(&req[..1], &[], false).is_empty());
    }
}
