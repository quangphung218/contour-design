//! Composer context (track B3-chat): the `@` mention palette and `/`
//! commands, as the original's composer triggers them. Pure, tested
//! natively; offsets are byte offsets into the draft (see [`byte_offset`]
//! for the browser's UTF-16 caret).

use crate::fuzzy;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// What a mention points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MentionKind {
    /// A project file: attached to the next message.
    File,
    /// A skill: added to the run's `skillIds`.
    Skill,
    /// A plugin: the run's `pluginId`.
    Plugin,
    /// An MCP server: the message asks the agent to use its tools.
    Mcp,
    /// A media library asset (track C2): copied into the project and
    /// attached, like "Import from library" in "+".
    Library,
}

impl MentionKind {
    /// Sections in palette order.
    pub const ALL: [Self; 5] = [
        Self::File,
        Self::Plugin,
        Self::Skill,
        Self::Mcp,
        Self::Library,
    ];

    /// Most rows a section shows (the original's caps).
    #[must_use]
    pub fn cap(self) -> usize {
        match self {
            Self::File => 12,
            Self::Plugin | Self::Skill | Self::Mcp | Self::Library => 8,
        }
    }
}

/// One row of the palette; also what Home hands the Studio's first turn
/// (`metadata.pendingContext`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub kind: MentionKind,
    pub id: String,
    /// What `@` inserts and what the row shows.
    pub label: String,
    /// Second line (description, folder).
    #[serde(default)]
    pub detail: String,
    /// A plugin's input defaults, sent as `pluginInputs`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<Map<String, Value>>,
}

/// A plugin row for the palette from one `/api/plugins` entry: `None` for an
/// atom, or a plugin that needs inputs without defaults (its own page asks
/// for them).
#[must_use]
pub fn plugin_candidate(p: &Value) -> Option<Candidate> {
    // Atoms are building blocks of other plugins, not something to apply.
    if p["kind"].as_str() == Some("atom") {
        return None;
    }
    let id = p["id"].as_str()?.to_owned();
    let mut inputs = Map::new();
    for input in p["inputs"].as_array().into_iter().flatten() {
        let Some(name) = input["name"].as_str() else {
            continue;
        };
        match input.get("default") {
            Some(d) if !d.is_null() => {
                inputs.insert(name.to_owned(), d.clone());
            }
            _ if input["required"].as_bool() == Some(true) => return None,
            _ => {}
        }
    }
    Some(Candidate {
        kind: MentionKind::Plugin,
        label: id.clone(),
        detail: p["title"].as_str().unwrap_or_default().to_owned(),
        id,
        inputs: Some(inputs),
    })
}

/// What the staged mentions add to a run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extras {
    pub skill_ids: Vec<String>,
    pub plugin_id: Option<String>,
    pub plugin_inputs: Option<Map<String, Value>>,
    /// The message, with a line asking for each mentioned MCP server.
    pub message: String,
}

/// The run extras for `text` from the mentions still in it. The first
/// plugin wins (a run applies one).
#[must_use]
pub fn extras(staged: &[Candidate], text: &str) -> Extras {
    let mut live = staged.to_vec();
    prune(&mut live, text);
    let mut out = Extras::default();
    let mut asks = String::new();
    for c in &live {
        match c.kind {
            MentionKind::Skill if !out.skill_ids.contains(&c.id) => {
                out.skill_ids.push(c.id.clone())
            }
            MentionKind::Plugin if out.plugin_id.is_none() => {
                out.plugin_id = Some(c.id.clone());
                out.plugin_inputs.clone_from(&c.inputs);
            }
            MentionKind::Mcp => asks.push_str(&mcp_use_text(&c.id)),
            _ => {}
        }
    }
    out.message = if asks.is_empty() {
        text.to_owned()
    } else {
        format!("{text}\n\n{}", asks.trim_end())
    };
    out
}

/// An open `@query` or `/query` before the caret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    /// Byte offset of the `@` or `/`.
    pub start: usize,
    pub query: String,
}

/// The `@query` the caret is in: `(^|\s)@([^\s@]*)$` on the text before
/// the caret, the original's rule.
#[must_use]
pub fn mention_at(text: &str, caret: usize) -> Option<Trigger> {
    let before = text.get(..caret)?;
    let at = before.rfind('@')?;
    let query = &before[at + 1..];
    if query.chars().any(|c| c.is_whitespace() || c == '@') {
        return None;
    }
    let opens = before[..at]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace);
    opens.then(|| Trigger {
        start: at,
        query: query.to_owned(),
    })
}

/// The `/command` being typed: only when the whole line before the caret
/// is `/` and a word (arguments end the palette).
#[must_use]
pub fn slash_at(text: &str, caret: usize) -> Option<Trigger> {
    let before = text.get(..caret)?;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = &before[line_start..];
    let query = line.strip_prefix('/')?;
    if query.chars().any(char::is_whitespace) {
        return None;
    }
    Some(Trigger {
        start: line_start,
        query: query.to_owned(),
    })
}

/// Replaces the trigger (up to the caret) with `insert`; returns the new
/// text and the caret after the insertion.
#[must_use]
pub fn apply(text: &str, trigger: &Trigger, caret: usize, insert: &str) -> (String, usize) {
    let caret = caret.min(text.len()).max(trigger.start);
    let mut out = String::with_capacity(text.len() + insert.len());
    out.push_str(&text[..trigger.start]);
    out.push_str(insert);
    let at = out.len();
    out.push_str(&text[caret..]);
    (out, at)
}

/// The palette rows for `query`, section by section, best first, each
/// section capped.
#[must_use]
pub fn rank(query: &str, candidates: &[Candidate]) -> Vec<Candidate> {
    let mut out = Vec::new();
    for kind in MentionKind::ALL {
        let mut scored: Vec<(i64, &Candidate)> = candidates
            .iter()
            .filter(|c| c.kind == kind)
            .filter_map(|c| {
                let by_label = fuzzy::score(query, &c.label);
                let by_id = fuzzy::score(query, &c.id).map(|s| s - 1);
                by_label.max(by_id).map(|s| (s, c))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.label.cmp(&b.1.label)));
        out.extend(scored.into_iter().take(kind.cap()).map(|(_, c)| c.clone()));
    }
    out
}

/// A library asset's palette row: `id` is `<projectId>:<path>` (project
/// ids never hold a colon), the label its file name.
#[must_use]
pub fn library_candidate(project_id: &str, path: &str, project_name: &str) -> Candidate {
    Candidate {
        kind: MentionKind::Library,
        id: format!("{project_id}:{path}"),
        label: path.rsplit('/').next().unwrap_or(path).to_owned(),
        detail: project_name.to_owned(),
        inputs: None,
    }
}

/// The `(projectId, path)` of a library row.
#[must_use]
pub fn library_ref(c: &Candidate) -> Option<(&str, &str)> {
    (c.kind == MentionKind::Library)
        .then(|| c.id.split_once(':'))
        .flatten()
}

/// The text a mention leaves in the draft.
#[must_use]
pub fn token(c: &Candidate) -> String {
    format!("@{} ", c.label)
}

/// Keeps only the staged mentions whose `@label` is still in the draft, so
/// deleting the text drops the context too.
pub fn prune(staged: &mut Vec<Candidate>, text: &str) {
    staged.retain(|c| text.contains(&format!("@{}", c.label)));
}

/// A `/` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slash {
    /// `/mcp` alone: open the MCP settings, send nothing.
    McpSettings,
    /// `/mcp <id>`: ask the agent to use that server's tools.
    McpUse(String),
    /// `/search <query>`: a research turn.
    Search,
}

impl Slash {
    /// What the palette shows and inserts.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::McpSettings => "/mcp".to_owned(),
            Self::McpUse(id) => format!("/mcp {id}"),
            Self::Search => "/search".to_owned(),
        }
    }
}

/// Commands matching `query` (prefix of the command word).
#[must_use]
pub fn slash_commands(query: &str, mcp_ids: &[String]) -> Vec<Slash> {
    let q = query.to_ascii_lowercase();
    let mut all = vec![Slash::McpSettings];
    all.extend(mcp_ids.iter().map(|id| Slash::McpUse(id.clone())));
    all.push(Slash::Search);
    all.into_iter()
        .filter(|c| c.label()[1..].to_ascii_lowercase().starts_with(&q))
        .collect()
}

/// Text that replaces a picked command in the draft.
#[must_use]
pub fn slash_insert(cmd: &Slash) -> String {
    match cmd {
        Slash::McpSettings => "/mcp".to_owned(),
        Slash::McpUse(id) => mcp_use_text(id),
        Slash::Search => "/search ".to_owned(),
    }
}

/// What a message asks of an MCP server (the original's wording).
#[must_use]
pub fn mcp_use_text(id: &str) -> String {
    format!("Use the `{id}` MCP server tools. ")
}

/// What to do with a draft on send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outgoing {
    /// Send this text.
    Send(String),
    /// Open the MCP settings instead of sending.
    OpenMcpSettings,
}

/// Expands `/mcp` and `/search <query>`; anything else is sent as typed.
#[must_use]
pub fn outgoing(draft: &str) -> Outgoing {
    let text = draft.trim();
    if text == "/mcp" {
        return Outgoing::OpenMcpSettings;
    }
    if let Some(query) = text
        .strip_prefix("/search ")
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        return Outgoing::Send(search_prompt(query));
    }
    Outgoing::Send(text.to_owned())
}

/// A research turn: search, then write the findings into the project.
#[must_use]
pub fn search_prompt(query: &str) -> String {
    let slug: String = query
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() {
        "notes".to_owned()
    } else {
        slug
    };
    format!(
        "Research this topic: {query}\n\nSearch the web with the tools you have, read the most relevant sources, and write a short report to `research/{slug}.md`: key findings as bullets, each with its source link, then open questions. Reply with a summary of the findings."
    )
}

/// The byte offset of a UTF-16 index (`selectionStart`) in `text`.
#[must_use]
pub fn byte_offset(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, c) in text.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += c.len_utf16();
    }
    text.len()
}

/// The UTF-16 index of a byte offset, for `setSelectionRange`.
#[must_use]
pub fn utf16_offset(text: &str, byte: usize) -> usize {
    text.get(..byte.min(text.len()))
        .map_or(0, |s| s.encode_utf16().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_rows_round_trip_their_asset_and_rank_last() {
        let c = library_candidate("p1", "assets/hero shot.png", "Launch");
        assert_eq!(
            (c.label.as_str(), c.detail.as_str()),
            ("hero shot.png", "Launch")
        );
        assert_eq!(library_ref(&c), Some(("p1", "assets/hero shot.png")));
        assert_eq!(library_ref(&cand(MentionKind::File, "p1:x")), None);
        let rows = rank("", &[c, cand(MentionKind::Mcp, "github")]);
        assert_eq!(rows.last().map(|r| r.kind), Some(MentionKind::Library));
    }

    fn cand(kind: MentionKind, id: &str) -> Candidate {
        Candidate {
            kind,
            id: id.to_owned(),
            label: id.to_owned(),
            detail: String::new(),
            inputs: None,
        }
    }

    #[test]
    fn plugins_with_open_required_inputs_stay_out() {
        let ok = serde_json::json!({ "id": "od-media-generation", "title": "Media",
            "inputs": [{ "name": "mediaKind", "default": "image" }, { "name": "style" }] });
        let c = plugin_candidate(&ok).unwrap_or_else(|| panic!("listed"));
        assert_eq!(
            c.inputs,
            serde_json::json!({ "mediaKind": "image" })
                .as_object()
                .cloned()
        );
        let needs = serde_json::json!({ "id": "od-new-generation",
            "inputs": [{ "name": "topic", "required": true }] });
        assert_eq!(plugin_candidate(&needs), None);
        let atom = serde_json::json!({ "id": "todo-write", "kind": "atom", "inputs": [] });
        assert_eq!(plugin_candidate(&atom), None);
    }

    #[test]
    fn extras_follow_the_mentions_left_in_the_text() {
        let mut plugin = cand(MentionKind::Plugin, "od-default");
        plugin.inputs = Some(Map::new());
        let staged = vec![
            cand(MentionKind::Skill, "web-clone"),
            cand(MentionKind::Skill, "web-clone"),
            plugin,
            cand(MentionKind::Plugin, "second"),
            cand(MentionKind::Mcp, "github"),
            cand(MentionKind::Skill, "gone"),
        ];
        let x = extras(&staged, "@web-clone @od-default @second @github go");
        assert_eq!(x.skill_ids, ["web-clone"]);
        assert_eq!(x.plugin_id.as_deref(), Some("od-default"));
        assert_eq!(x.plugin_inputs, Some(Map::new()));
        assert_eq!(
            x.message,
            "@web-clone @od-default @second @github go\n\nUse the `github` MCP server tools."
        );
        assert_eq!(extras(&staged, "plain").message, "plain");
    }

    #[test]
    fn mention_opens_after_space_or_line_start_only() {
        assert_eq!(
            mention_at("look at @ind", 12),
            Some(Trigger {
                start: 8,
                query: "ind".into()
            })
        );
        assert_eq!(mention_at("@", 1).map(|t| t.query), Some(String::new()));
        assert_eq!(mention_at("line\n@a", 7).map(|t| t.start), Some(5));
        assert_eq!(mention_at("mail me@home", 12), None, "inside a word");
        assert_eq!(mention_at("@a b", 4), None, "a space closes it");
        assert_eq!(mention_at("@a@b", 4), None);
        assert_eq!(mention_at("no at", 5), None);
        // Caret in the middle: only the text before it counts.
        assert_eq!(mention_at("@ab cd", 2).map(|t| t.query), Some("a".into()));
    }

    #[test]
    fn slash_needs_the_whole_line() {
        assert_eq!(slash_at("/mc", 3).map(|t| t.query), Some("mc".into()));
        assert_eq!(slash_at("hi\n/se", 6).map(|t| t.start), Some(3));
        assert_eq!(slash_at("hi /mcp", 7), None);
        assert_eq!(slash_at("/mcp github", 11), None);
    }

    #[test]
    fn apply_replaces_up_to_the_caret() {
        let text = "fix @ind please";
        let tr = mention_at(text, 8).unwrap_or_else(|| panic!("trigger"));
        let (out, caret) = apply(text, &tr, 8, "@index.html ");
        assert_eq!(out, "fix @index.html  please");
        assert_eq!(&out[..caret], "fix @index.html ");
    }

    #[test]
    fn rank_groups_sections_and_caps_them() {
        let mut all: Vec<Candidate> = (0..20)
            .map(|i| cand(MentionKind::File, &format!("page-{i:02}.html")))
            .collect();
        all.push(cand(MentionKind::Mcp, "github"));
        all.push(cand(MentionKind::Skill, "web-clone"));
        all.push(cand(MentionKind::Plugin, "od-default"));
        let rows = rank("", &all);
        assert_eq!(
            rows.iter().filter(|c| c.kind == MentionKind::File).count(),
            12
        );
        let kinds: Vec<MentionKind> = rows.iter().map(|c| c.kind).collect();
        let first = |k| kinds.iter().position(|x| *x == k);
        assert!(first(MentionKind::File) < first(MentionKind::Plugin));
        assert!(first(MentionKind::Plugin) < first(MentionKind::Skill));
        assert!(first(MentionKind::Skill) < first(MentionKind::Mcp));
        let gh = rank("github", &all);
        assert_eq!(gh.len(), 1);
        assert_eq!(gh[0].id, "github");
    }

    #[test]
    fn pruning_follows_the_draft() {
        let mut staged = vec![
            cand(MentionKind::Skill, "web-clone"),
            cand(MentionKind::Mcp, "github"),
        ];
        prune(&mut staged, "clone it @web-clone now");
        assert_eq!(staged.len(), 1);
        assert_eq!(token(&staged[0]), "@web-clone ");
    }

    #[test]
    fn slash_commands_and_outgoing() {
        let ids = vec!["github".to_owned()];
        assert_eq!(
            slash_commands("", &ids),
            [
                Slash::McpSettings,
                Slash::McpUse("github".into()),
                Slash::Search
            ]
        );
        assert_eq!(slash_commands("se", &ids), [Slash::Search]);
        assert_eq!(
            slash_insert(&Slash::McpUse("github".into())),
            "Use the `github` MCP server tools. "
        );
        assert_eq!(outgoing(" /mcp "), Outgoing::OpenMcpSettings);
        assert_eq!(outgoing("/search"), Outgoing::Send("/search".into()));
        let Outgoing::Send(p) = outgoing("/search Tea shop trends 2026!") else {
            panic!("send")
        };
        assert!(p.starts_with("Research this topic: Tea shop trends 2026!"));
        assert!(p.contains("`research/tea-shop-trends-2026.md`"));
        assert_eq!(outgoing("hello"), Outgoing::Send("hello".into()));
    }

    #[test]
    fn utf16_offsets_round_trip() {
        let text = "Trà 🍵 @x";
        let byte = text.find('@').unwrap_or_default();
        let units = utf16_offset(text, byte);
        assert_eq!(units, 7);
        assert_eq!(byte_offset(text, units), byte);
        assert_eq!(byte_offset(text, 999), text.len());
    }
}
