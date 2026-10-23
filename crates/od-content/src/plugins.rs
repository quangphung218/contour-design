//! Bundled plugins as presets (spec 1.4, 3.6): the first-party atoms and
//! scenarios under `plugins/_official/`, plus user plugins in
//! `<data>/plugins/` (same id shadows bundled). Read-only: no marketplace,
//! no install from the network. Applying a plugin
//! validates its inputs and renders the `## Active plugin` block that the
//! prompt composer places after the active skill (spec 2.4 block 10).

use crate::frontmatter;
use crate::pipeline::Pipeline;
use crate::text::sorted_dir_entries;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// Manifest file name next to `SKILL.md`.
pub const MANIFEST: &str = "open-design.json";
/// Claude-plugin manifest accepted when `open-design.json` is absent.
pub const CLAUDE_PLUGIN_MANIFEST: &str = ".claude-plugin/plugin.json";

/// A `context.skills[]` reference: a registry id (`ref`) or a
/// plugin-local `path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillRef {
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// One plugin, as listed by `/api/plugins`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry {
    pub id: String,
    pub title: String,
    pub description: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    pub tags: Vec<String>,
    /// `od.kind`: scenario, atom, skill or bundle.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Hidden plugins (the default router) are not shown in pickers.
    pub hidden: bool,
    pub capabilities: Vec<String>,
    /// `od.inputs[]` field specs, as declared.
    pub inputs: Vec<Value>,
    pub context: PluginContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<Value>,
    /// `bundled` or `user`.
    pub source: &'static str,
    #[serde(skip)]
    pub dir: PathBuf,
    #[serde(skip)]
    pub manifest: Map<String, Value>,
    /// `SKILL.md` body without front matter (plugin instructions).
    #[serde(skip)]
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginContext {
    pub skills: Vec<SkillRef>,
    pub craft: Vec<String>,
    pub atoms: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_system: Option<String>,
}

fn str_of(v: Option<&Value>) -> Option<String> {
    v.and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|x| str_of(Some(x)))
        .collect()
}

/// Text for `locale` from an i18n map: exact tag, then its language, then
/// `en`. A plain string is returned as is.
#[must_use]
pub fn localized(v: Option<&Value>, locale: Option<&str>) -> Option<String> {
    match v? {
        Value::String(s) => str_of(Some(&Value::String(s.clone()))),
        Value::Object(map) => {
            let mut keys: Vec<String> = Vec::new();
            if let Some(l) = locale.map(str::trim).filter(|l| !l.is_empty()) {
                keys.push(l.to_owned());
                if let Some((lang, _)) = l.split_once(['-', '_']) {
                    keys.push(lang.to_owned());
                }
            }
            keys.push("en".to_owned());
            keys.iter().find_map(|k| str_of(map.get(k)))
        }
        _ => None,
    }
}

fn read_manifest(dir: &Path) -> Option<Map<String, Value>> {
    [MANIFEST, CLAUDE_PLUGIN_MANIFEST].iter().find_map(|name| {
        let text = std::fs::read_to_string(dir.join(name)).ok()?;
        serde_json::from_str::<Value>(&text)
            .ok()?
            .as_object()
            .cloned()
    })
}

/// Parses one plugin folder; `None` when it has neither a manifest nor a
/// `SKILL.md`, or they are unreadable (discovery, not validation).
#[must_use]
pub fn load_plugin(dir: &Path, folder: &str, source: &'static str) -> Option<PluginEntry> {
    let skill = std::fs::read_to_string(dir.join("SKILL.md")).ok();
    let (front, body) = match &skill {
        Some(text) => frontmatter::parse(text).ok()?,
        None => (Map::new(), ""),
    };
    let manifest = match read_manifest(dir) {
        Some(m) => m,
        None if skill.is_some() => Map::new(),
        None => return None,
    };
    let od = manifest
        .get("od")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let context = od
        .get("context")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let id = str_of(manifest.get("name"))
        .or_else(|| str_of(front.get("name")))
        .unwrap_or_else(|| folder.to_owned());
    let skills = context
        .get("skills")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| match s {
            Value::String(r) => Some(SkillRef {
                id: Some(r.clone()),
                path: None,
            }),
            Value::Object(o) => {
                let r = SkillRef {
                    id: str_of(o.get("ref")),
                    path: str_of(o.get("path")),
                };
                (r.id.is_some() || r.path.is_some()).then_some(r)
            }
            _ => None,
        })
        .collect();
    let design_system = match context.get("designSystem") {
        Some(Value::String(s)) => str_of(Some(&Value::String(s.clone()))),
        Some(Value::Object(o)) => str_of(o.get("ref")).or_else(|| str_of(o.get("id"))),
        _ => None,
    };
    let mut atoms = strings(context.get("atoms"));
    for stage in od
        .get("pipeline")
        .and_then(|p| p.get("stages"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for atom in strings(stage.get("atoms")) {
            if !atoms.contains(&atom) {
                atoms.push(atom);
            }
        }
    }
    Some(PluginEntry {
        title: localized(manifest.get("title"), None)
            .or_else(|| str_of(manifest.get("title")))
            .unwrap_or_else(|| id.clone()),
        description: str_of(manifest.get("description"))
            .or_else(|| str_of(front.get("description")))
            .unwrap_or_default(),
        version: str_of(manifest.get("version")).unwrap_or_else(|| "0.0.0".to_owned()),
        license: str_of(manifest.get("license")),
        tags: strings(manifest.get("tags")),
        kind: str_of(od.get("kind")).unwrap_or_else(|| "skill".to_owned()),
        task_kind: str_of(od.get("taskKind")),
        mode: str_of(od.get("mode")),
        hidden: od.get("hidden").and_then(Value::as_bool).unwrap_or(false),
        capabilities: strings(od.get("capabilities")),
        inputs: od
            .get("inputs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        context: PluginContext {
            skills,
            craft: strings(context.get("craft")),
            atoms,
            design_system,
        },
        pipeline: od.get("pipeline").cloned(),
        source,
        dir: dir.to_path_buf(),
        body: body.trim().to_owned(),
        manifest,
        id,
    })
}

/// Every plugin under the roots, user roots first; the first plugin with
/// an id wins. Bundled roots hold `atoms/` and `scenarios/` groups; user
/// roots hold plugin folders directly.
#[must_use]
pub fn list_plugins(bundled: &Path, user: Option<&Path>) -> Vec<PluginEntry> {
    let mut out: Vec<PluginEntry> = Vec::new();
    let mut push = |entry: PluginEntry| {
        if !out.iter().any(|p| p.id == entry.id) {
            out.push(entry);
        }
    };
    if let Some(user) = user {
        for (name, dir) in sorted_dir_entries(user) {
            if !name.starts_with(['.', '_']) {
                if let Some(p) = load_plugin(&dir, &name, "user") {
                    push(p);
                }
            }
        }
    }
    for (group, group_dir) in sorted_dir_entries(bundled) {
        if group.starts_with('.') {
            continue;
        }
        for (name, dir) in sorted_dir_entries(&group_dir) {
            if !name.starts_with('.') {
                if let Some(p) = load_plugin(&dir, &name, "bundled") {
                    push(p);
                }
            }
        }
    }
    out
}

/// Why an apply was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplyError {
    #[error("missing required plugin inputs: {}", .0.join(", "))]
    MissingInputs(Vec<String>),
}

/// The snapshot a run is bound to (subset of the original
/// `AppliedPluginSnapshot`), with the rendered prompt block.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedPlugin {
    pub plugin_id: String,
    pub plugin_version: String,
    pub plugin_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_description: Option<String>,
    /// The example brief with `{{input}}` placeholders filled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub inputs: Map<String, Value>,
    pub task_kind: String,
    pub capabilities_required: Vec<String>,
    pub atoms: Vec<String>,
    pub craft_requires: Vec<String>,
    /// Registry skill ids from `context.skills[].ref`.
    pub skill_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_system_id: Option<String>,
    /// `od.pipeline`, validated ([`crate::pipeline`]); `None` when absent
    /// or invalid (`od plugin validate` reports why).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<Pipeline>,
    /// `## Active plugin` / `## Plugin inputs` / `## Plugin atoms`, plus
    /// the plugin's own instructions.
    pub prompt_block: String,
}

fn coerce(v: &Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(_) | Value::Number(_) | Value::Bool(_) => Some(v.clone()),
        Value::Array(items) => Some(json!(
            items
                .iter()
                .map(crate::text::js_string)
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Value::Object(_) => Some(json!(v.to_string())),
    }
}

fn input_text(v: &Value) -> String {
    match v {
        Value::String(s) if s.is_empty() => "(empty)".to_owned(),
        Value::String(s) => s.clone(),
        other => crate::text::js_string(other),
    }
}

/// Fills `{{name}}` with input values; unknown placeholders stay.
fn fill(query: &str, inputs: &Map<String, Value>) -> String {
    let mut out = query.to_owned();
    for (k, v) in inputs {
        for pattern in [format!("{{{{{k}}}}}"), format!("{{{{ {k} }}}}")] {
            out = out.replace(&pattern, &input_text(v));
        }
    }
    out
}

impl PluginEntry {
    /// Validates `raw` against `od.inputs` (defaults fill gaps, required
    /// fields must be present) and renders the prompt block.
    ///
    /// # Errors
    /// [`ApplyError::MissingInputs`].
    pub fn apply(
        &self,
        raw: &Map<String, Value>,
        locale: Option<&str>,
    ) -> Result<AppliedPlugin, ApplyError> {
        let mut inputs = Map::new();
        let mut missing = Vec::new();
        for field in &self.inputs {
            let Some(name) = str_of(field.get("name")) else {
                continue;
            };
            match raw.get(&name).and_then(coerce) {
                Some(v) => {
                    inputs.insert(name, v);
                }
                None => match field.get("default").and_then(coerce) {
                    Some(d) => {
                        inputs.insert(name, d);
                    }
                    None if field.get("required") == Some(&Value::Bool(true)) => {
                        missing.push(name);
                    }
                    None => {}
                },
            }
        }
        if !missing.is_empty() {
            return Err(ApplyError::MissingInputs(missing));
        }
        for (k, v) in raw {
            if !inputs.contains_key(k) {
                if let Some(v) = coerce(v) {
                    inputs.insert(k.clone(), v);
                }
            }
        }
        let title = localized(self.manifest.get("title_i18n"), locale)
            .unwrap_or_else(|| self.title.clone());
        let description = localized(self.manifest.get("description_i18n"), locale)
            .or_else(|| Some(self.description.clone()).filter(|d| !d.is_empty()));
        let query = localized(
            self.manifest
                .get("od")
                .and_then(|od| od.get("useCase"))
                .and_then(|u| u.get("query")),
            locale,
        )
        .map(|q| fill(&q, &inputs));
        let mut applied = AppliedPlugin {
            plugin_id: self.id.clone(),
            plugin_version: self.version.clone(),
            plugin_title: title,
            plugin_description: description,
            query,
            inputs,
            task_kind: self
                .task_kind
                .clone()
                .unwrap_or_else(|| "new-generation".to_owned()),
            capabilities_required: self.capabilities.clone(),
            atoms: self.context.atoms.clone(),
            craft_requires: self.context.craft.clone(),
            skill_ids: self
                .context
                .skills
                .iter()
                .filter_map(|s| s.id.clone())
                .collect(),
            design_system_id: self.context.design_system.clone(),
            pipeline: self
                .pipeline
                .as_ref()
                .and_then(|p| Pipeline::parse(p).ok()),
            prompt_block: String::new(),
        };
        applied.prompt_block = render_block(&applied, &self.body);
        Ok(applied)
    }
}

/// The original `renderPluginBlock`, plus the plugin's `SKILL.md` body
/// (the original reads plugin-local skill bodies at compose time).
#[must_use]
pub fn render_block(p: &AppliedPlugin, instructions: &str) -> String {
    let mut lines: Vec<String> = vec![
        "\n\n## Active plugin".to_owned(),
        String::new(),
        format!(
            "The user applied plugin **{}** (`{}@{}`).",
            p.plugin_title, p.plugin_id, p.plugin_version
        ),
    ];
    if let Some(d) = p
        .plugin_description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    {
        lines.extend([String::new(), d.to_owned()]);
    }
    if let Some(q) = p.query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        lines.extend([
            String::new(),
            format!("The plugin's example brief is: _{q}_"),
        ]);
    }
    if !p.inputs.is_empty() {
        lines.extend([
            String::new(),
            "## Plugin inputs".to_owned(),
            String::new(),
            "Treat these as authoritative answers to questions the plugin author baked into the brief — do not re-ask the user about them.".to_owned(),
            String::new(),
        ]);
        let mut keys: Vec<&String> = p.inputs.keys().collect();
        keys.sort();
        for k in keys {
            lines.push(format!("- **{k}**: {}", input_text(&p.inputs[k])));
        }
    }
    if !p.atoms.is_empty() {
        lines.extend([
            String::new(),
            "## Plugin atoms".to_owned(),
            String::new(),
            "The plugin opted into these workflow atoms; prefer them over ad-hoc shortcuts:"
                .to_owned(),
            String::new(),
        ]);
        lines.extend(p.atoms.iter().map(|a| format!("- `{a}`")));
    }
    if let Some(pipeline) = &p.pipeline {
        lines.extend([
            String::new(),
            "## Plugin pipeline".to_owned(),
            String::new(),
            "Work through these stages in order:".to_owned(),
            String::new(),
        ]);
        for (i, stage) in pipeline.stages.iter().enumerate() {
            let atoms: Vec<String> = stage.atoms.iter().map(|a| format!("`{a}`")).collect();
            let mut line = format!("{}. **{}**: {}", i + 1, stage.id, atoms.join(", "));
            if let (true, Some(until)) = (stage.repeat, stage.until.as_deref()) {
                line.push_str(&format!(" (repeat until `{until}`)"));
            }
            lines.push(line);
        }
    }
    let instructions = instructions.trim();
    if !instructions.is_empty() {
        lines.extend([
            String::new(),
            "## Plugin instructions".to_owned(),
            String::new(),
            instructions.to_owned(),
        ]);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(dir: &Path, manifest: &Value, skill: &str) -> PluginEntry {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(MANIFEST), manifest.to_string()).unwrap();
        std::fs::write(dir.join("SKILL.md"), skill).unwrap();
        load_plugin(dir, "x", "bundled").unwrap()
    }

    #[test]
    fn apply_validates_inputs_and_renders_the_block() {
        let tmp = tempfile::tempdir().unwrap();
        let p = plugin(
            &tmp.path().join("p"),
            &json!({
                "name": "demo", "title": "Demo", "version": "1.2.0",
                "title_i18n": { "vi": "Bản demo" },
                "description": "Does a demo.",
                "od": {
                    "kind": "scenario", "taskKind": "new-generation",
                    "useCase": { "query": { "en": "Make a {{kind}} about {{topic}}" } },
                    "context": { "craft": ["color"], "skills": [{ "ref": "web-prototype" }, { "path": "./SKILL.md" }] },
                    "pipeline": { "stages": [{ "id": "plan", "atoms": ["todo-write"] }] },
                    "inputs": [
                        { "name": "topic", "type": "text", "required": true },
                        { "name": "kind", "type": "select", "default": "deck" }
                    ]
                }
            }),
            "---\nname: demo\n---\n\n# Demo\n\nDo the thing.\n",
        );
        assert_eq!(p.context.atoms, ["todo-write"]);
        let err = p.apply(&Map::new(), None).unwrap_err();
        assert_eq!(err, ApplyError::MissingInputs(vec!["topic".to_owned()]));

        let raw = json!({ "topic": "rust", "extra": ["a", "b"] });
        let a = p.apply(raw.as_object().unwrap(), Some("vi-VN")).unwrap();
        assert_eq!(a.plugin_title, "Bản demo");
        assert_eq!(a.query.as_deref(), Some("Make a deck about rust"));
        assert_eq!(a.skill_ids, ["web-prototype"]);
        assert_eq!(a.craft_requires, ["color"]);
        assert_eq!(
            a.prompt_block,
            "\n\n## Active plugin\n\nThe user applied plugin **Bản demo** (`demo@1.2.0`).\n\nDoes a demo.\n\nThe plugin's example brief is: _Make a deck about rust_\n\n## Plugin inputs\n\nTreat these as authoritative answers to questions the plugin author baked into the brief — do not re-ask the user about them.\n\n- **extra**: a, b\n- **kind**: deck\n- **topic**: rust\n\n## Plugin atoms\n\nThe plugin opted into these workflow atoms; prefer them over ad-hoc shortcuts:\n\n- `todo-write`\n\n## Plugin pipeline\n\nWork through these stages in order:\n\n1. **plan**: `todo-write`\n\n## Plugin instructions\n\n# Demo\n\nDo the thing."
        );
    }

    #[test]
    fn user_plugins_shadow_bundled_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = tmp.path().join("bundled");
        let user = tmp.path().join("user");
        plugin(
            &bundled.join("atoms/a"),
            &json!({ "name": "a", "title": "Bundled A" }),
            "",
        );
        plugin(&bundled.join("scenarios/b"), &json!({ "name": "b" }), "");
        plugin(
            &user.join("a"),
            &json!({ "name": "a", "title": "User A" }),
            "",
        );
        let all = list_plugins(&bundled, Some(&user));
        let ids: Vec<(&str, &str)> = all
            .iter()
            .map(|p| (p.id.as_str(), p.title.as_str()))
            .collect();
        assert_eq!(ids, [("a", "User A"), ("b", "b")]);
        assert_eq!(all[0].source, "user");
    }
}
