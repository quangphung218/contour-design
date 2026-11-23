//! Whether a user's file or folder name is taken is decided one way, by
//! `od_contracts::name_key` (re-exported as `od_store::paths::name_key`):
//! NFC and case ignored, as macOS and Windows disks do. A check written by
//! hand (`to_lowercase` as a set key, `eq_ignore_ascii_case` on a path, an
//! own NFC) differs from the disk on some OS: on Linux two files that are
//! one on a Mac, on a Mac a file written over (#225, #242, #94).
//!
//! This test scans every `.rs` under `crates/*/src` and `apps/*/src` for
//! such checks, a statement at a time (rustfmt splits a long compare over
//! lines). Each one found either goes through `name_key`, or is listed in
//! [`KEPT`] with why it is no check whether a name is taken.
//!
//! A write in `od-store/src/files.rs` spells its target as on disk first
//! (`as_on_disk`, `renamed_as`), else `logo.png` lands beside `Logo.png`
//! on Linux: [`Rule::UnspelledWrite`]. So does each name it is sent, the
//! source of a rename, move or delete too, before it is used, else what is
//! recorded by that name (history, comments) stays behind on a Mac:
//! [`Rule::UnspelledName`], in `files.rs` and in the store's doors to it
//! (`store.rs`, `edit.rs`, `comments.rs`, `drafts.rs`). A function there
//! that decides on a file (what it holds, its history) and then writes or
//! deletes it gives both steps one name, spelled: [`Rule::DecidedApart`].

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The kinds of hand-written name checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rule {
    /// Its own Unicode normalization (`.nfc()`…): the key is `name_key`.
    OwnNormalization,
    /// A lowercased value looked up in or put into a set or map, or
    /// searched for: `seen.insert(path.to_lowercase(), …)`.
    LowercaseKey,
    /// Two values folded by hand: `a.eq_ignore_ascii_case(b)`,
    /// `a.to_lowercase() == b…`. Whatever they name: one that is no file
    /// name goes in [`KEPT`].
    FoldedCompare,
    /// A function of `od-store/src/files.rs` that writes (resolves a
    /// target, creates, renames) before spelling the name as on disk.
    /// Found by its signature.
    UnspelledWrite,
    /// A name checked by `validate_project_path`/`sanitize_path` in
    /// `od-store/src/files.rs` that is used before it is spelled as on disk:
    /// `B.html` renamed while the disk has `b.html` moves the file, but the
    /// history of `b.html` stays behind. Found by the function's signature
    /// and the statement that uses it.
    UnspelledName,
    /// A function of the store that decides on a file ([`DECIDE_CALLS`]:
    /// what it holds, its stat, its history) and writes or deletes it
    /// ([`ACT_CALLS`]) under names that differ, or under one that is
    /// neither its parameter nor spelled as on disk where it is bound: a
    /// history read for `B.html` while `b.html` is written. Also every
    /// call of `fate(`, which reads one name: a draft's row with no ident
    /// tells its file only by that name, so where the name on disk differs
    /// it must decide through `fate_spelled`, which sees both.
    DecidedApart,
}

/// `(file from the repo root, text on the line, rule, why it may stay)`.
const KEPT: &[(&str, &str, Rule, &str)] = &[
    (
        "crates/od-contracts/src/files.rs",
        "name.nfc()",
        Rule::OwnNormalization,
        "the definition of `name_key`",
    ),
    (
        "crates/od-store/src/paths.rs",
        "n.nfc()",
        Rule::OwnNormalization,
        "`entry_named` orders the entries of one key (the exact name, then \
         the same NFC, then the key): which one, not whether one is taken",
    ),
    (
        "crates/od-server/src/routes/library/design_systems.rs",
        "n.nfc()",
        Rule::OwnNormalization,
        "`replaced_asset` orders the assets of one key as `entry_named` does",
    ),
    (
        "crates/od-store/src/paths.rs",
        "reserved.eq_ignore_ascii_case(segment)",
        Rule::FoldedCompare,
        "an app folder name (`.open-design`), ASCII; the real path is checked \
         again after `canonicalize`",
    ),
    (
        "crates/od-export/src/hyperframes.rs",
        "n.as_str().eq_ignore_ascii_case(name)",
        Rule::FoldedCompare,
        "an HTML attribute name, not a file",
    ),
    (
        "crates/od-preview/src/lint.rs",
        "a.eq_ignore_ascii_case(name)",
        Rule::FoldedCompare,
        "an HTML attribute name, not a file",
    ),
    (
        "crates/od-preview/src/source_patch.rs",
        "end.name().eq_ignore_ascii_case(&tag)",
        Rule::FoldedCompare,
        "an HTML tag name, not a file",
    ),
    (
        "crates/od-preview/src/source_patch.rs",
        "name.eq_ignore_ascii_case(ANCHOR_ATTR)",
        Rule::FoldedCompare,
        "an HTML attribute name, not a file",
    ),
    (
        "crates/od-server/src/guard.rs",
        "host.hostname == bind_host.to_lowercase()",
        Rule::FoldedCompare,
        "a host name, not a file",
    ),
    (
        "crates/od-contracts/src/agent_w3.rs",
        "r.as_str().eq_ignore_ascii_case(raw.trim())",
        Rule::FoldedCompare,
        "a role word parsed into its enum, not a file",
    ),
    (
        "crates/od-contracts/src/next_step.rs",
        "o.to_lowercase() == s.to_lowercase()",
        Rule::FoldedCompare,
        "two suggested next steps (reply text), not files",
    ),
    (
        "crates/od-contracts/src/next_step.rs",
        "text[start..].eq_ignore_ascii_case(&OPEN[..n])",
        Rule::FoldedCompare,
        "a tag in a reply's text, not a file",
    ),
    (
        "crates/od-mcp/src/tools.rs",
        "n.to_lowercase() == lower",
        Rule::FoldedCompare,
        "a project's display name the agent asked for, not a file",
    ),
    (
        "crates/od-mcp/src/tools.rs",
        "a.eq_ignore_ascii_case(b) => go(",
        Rule::FoldedCompare,
        "`glob_match` searches the listed files: finds names, never \
         decides whether one is taken",
    ),
    (
        "crates/od-preview/src/source_patch.rs",
        "target.tag.eq_ignore_ascii_case(expected.trim())",
        Rule::FoldedCompare,
        "an HTML tag name, not a file",
    ),
    (
        "crates/od-preview/src/source_patch.rs",
        "p.eq_ignore_ascii_case(&key)",
        Rule::FoldedCompare,
        "a CSS property name, not a file",
    ),
    (
        "crates/od-preview/src/still.rs",
        "s.eq_ignore_ascii_case(tag)",
        Rule::FoldedCompare,
        "an HTML tag name, not a file",
    ),
    (
        "crates/od-runtime/src/detect.rs",
        "w.eq_ignore_ascii_case(e)",
        Rule::FoldedCompare,
        "a program extension (`.exe`) read as Windows reads it, not \
         whether a name is taken",
    ),
    (
        "crates/od-server/src/routes/genui.rs",
        "i.to_lowercase() == sid",
        Rule::FoldedCompare,
        "a UI surface id, not a file",
    ),
    (
        "crates/od-server/src/routes/w3/reference.rs",
        "head.eq_ignore_ascii_case(scheme)",
        Rule::FoldedCompare,
        "a URL scheme, not a file",
    ),
    (
        "crates/od-store/src/memory.rs",
        "m.text.eq_ignore_ascii_case(&text)",
        Rule::FoldedCompare,
        "the text of a memory note, not a file",
    ),
    (
        "crates/od-ui/src/app/native_shell.rs",
        "p.eq_ignore_ascii_case(s)",
        Rule::FoldedCompare,
        "a URL scheme, not a file",
    ),
    (
        "crates/od-ui/src/highlight.rs",
        "hay[i..i + n.len()].eq_ignore_ascii_case(n)",
        Rule::FoldedCompare,
        "a word searched for in code text, not a file",
    ),
    (
        "crates/od-ui/src/i18n.rs",
        "l.tag().eq_ignore_ascii_case(tag)",
        Rule::FoldedCompare,
        "a language tag, not a file",
    ),
    (
        "crates/od-store/src/files.rs",
        "fn ensure_exists(",
        Rule::UnspelledWrite,
        "creates the project folder itself, named by the app",
    ),
    (
        "crates/od-store/src/files.rs",
        "fn stat_file(",
        Rule::UnspelledWrite,
        "reads: makes nothing on disk",
    ),
    (
        "crates/od-store/src/files.rs",
        "fn write_atomic(",
        Rule::UnspelledWrite,
        "the write itself: every caller spells its target first",
    ),
    (
        "crates/od-store/src/files.rs",
        "fn place_new(",
        Rule::UnspelledWrite,
        "part of `write_atomic`, on the target its caller spelled",
    ),
    (
        "crates/od-store/src/files.rs",
        "fn replace_atomic(",
        Rule::UnspelledWrite,
        "app files (history manifests, snapshots) the app names",
    ),
    (
        "crates/od-store/src/files.rs",
        "fn write_named(",
        Rule::UnspelledWrite,
        "its callers spell the name first, which `UnspelledName` checks",
    ),
    (
        "crates/od-store/src/files.rs",
        "fn put_back(",
        Rule::UnspelledWrite,
        "moves a file back to the name it had on disk a moment ago",
    ),
    (
        "crates/od-ui/src/app/api.rs",
        "self.message.to_lowercase().contains(&hint.to_lowercase())",
        Rule::LowercaseKey,
        "finds a hint in an error message, not a file",
    ),
    (
        "crates/od-store/src/store.rs",
        "pub fn write_file(",
        Rule::DecidedApart,
        "decides on `name` sanitized and spelled; `files::write_file` \
         sanitizes and spells `name` the same way itself (`UnspelledName` \
         holds it in files.rs)",
    ),
    (
        "crates/od-store/src/store.rs",
        "pub fn create_file_exact(",
        Rule::DecidedApart,
        "decides on `name` spelled; `files::create_file_exact` spells \
         `name` the same way itself",
    ),
    (
        "crates/od-store/src/store.rs",
        "pub fn rename_file(",
        Rule::DecidedApart,
        "two names, source and target, each spelled as `files::rename_file` \
         spells them (`as_on_disk`, `renamed_as`)",
    ),
    (
        "crates/od-store/src/store.rs",
        "pub fn move_folder(",
        Rule::DecidedApart,
        "decides on the source spelled; `files::move_folder` spells it the \
         same way itself",
    ),
    (
        "crates/od-store/src/store.rs",
        "fn upload_file_held(",
        Rule::DecidedApart,
        "the history is refused by the name `files::upload_file` just wrote \
         (spelled there); what it holds and the delete share `path`",
    ),
    (
        "crates/od-store/src/store.rs",
        "pub fn duplicate_project(",
        Rule::DecidedApart,
        "copies each file under the name `files::list_files` read from \
         disk: spelled as on disk already",
    ),
    (
        "crates/od-store/src/drafts.rs",
        "fn fate_spelled(",
        Rule::DecidedApart,
        "the one call of `fate`, on the spelled name, after a row with no \
         ident whose name differs on disk is told another file",
    ),
];

/// What `rule` finds in `stmt`, one statement (comments left out and its
/// lines joined by the caller).
fn finds(rule: Rule, stmt: &str) -> bool {
    let lower_call = |s: &str| s.contains("to_lowercase()") || s.contains("to_ascii_lowercase()");
    match rule {
        Rule::OwnNormalization => [".nfc()", ".nfd()", ".nfkc()", ".nfkd()"]
            .iter()
            .any(|n| stmt.contains(n)),
        Rule::LowercaseKey => [
            ".insert(",
            ".contains(",
            ".contains_key(",
            ".entry(",
            ".get(",
            ".remove(",
        ]
        .iter()
        .any(|call| {
            stmt.match_indices(call)
                .any(|(at, _)| lower_call(call_args(&stmt[at + call.len()..])))
        }),
        Rule::FoldedCompare => {
            stmt.match_indices("eq_ignore_ascii_case(").any(|(at, m)| {
                // A fixed string (`"index.html"`, `"t"`) is no user name.
                !stmt[at + m.len()..].starts_with('"')
            }) || ["==", "!="].iter().any(|op| {
                stmt.match_indices(op).any(|(at, _)| {
                    lower_call(left_operand(&stmt[..at]))
                        || lower_call(right_operand(&stmt[at + op.len()..]))
                })
            })
        }
        Rule::UnspelledWrite | Rule::UnspelledName | Rule::DecidedApart => false,
    }
}

/// Files whose functions [`Rule::UnspelledWrite`] checks: the raw disk ops
/// every write of a user's file goes through.
const WRITES_CHECKED: &[&str] = &["crates/od-store/src/files.rs"];

/// Calls that pick or change a path on disk.
const WRITE_CALLS: &[&str] = &[
    "root.resolve(",
    "write_atomic(",
    "fs::write(",
    "fs::rename(",
    "fs::create_dir",
    "fs::copy(",
    "File::create(",
    ".create_new(",
];

/// Calls that spell a name as the entry on disk.
const SPELL_CALLS: &[&str] = &["as_on_disk(", "renamed_as("];

/// The signature line of each function in `lines` that makes a
/// [`WRITE_CALLS`] call before any [`SPELL_CALLS`] call.
fn unspelled_writes(lines: &[&str]) -> Vec<String> {
    let is_fn = |l: &str| {
        let l = l.trim_start();
        ["fn ", "pub fn ", "pub(crate) fn ", "pub(super) fn "]
            .iter()
            .any(|p| l.starts_with(p))
    };
    let first = |body: &str, calls: &[&str]| {
        calls
            .iter()
            .flat_map(|c| body.match_indices(c).map(|(at, _)| at))
            // `fn write_atomic(` is the definition, no call.
            .filter(|&at| !body[..at].ends_with("fn "))
            .min()
    };
    let starts: Vec<usize> = (0..lines.len()).filter(|&i| is_fn(lines[i])).collect();
    let mut out = Vec::new();
    for (n, &at) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(lines.len());
        let body = lines[at..end].join("\n");
        if let Some(write) = first(&body, WRITE_CALLS)
            && first(&body, SPELL_CALLS).is_none_or(|spell| spell > write)
        {
            out.push(lines[at].trim().to_owned());
        }
    }
    out
}

/// Calls that check a name the caller sent and give it back as sent.
const NAME_CALLS: &[&str] = &["validate_project_path(", "sanitize_path("];

/// Files whose functions [`Rule::UnspelledName`] checks: the raw disk ops,
/// and the store's functions that take a name from a caller.
const NAMES_CHECKED: &[&str] = &[
    "crates/od-store/src/files.rs",
    "crates/od-store/src/store.rs",
    "crates/od-store/src/edit.rs",
    "crates/od-store/src/comments.rs",
    "crates/od-store/src/drafts.rs",
];

/// Files whose functions [`Rule::DecidedApart`] checks.
const DECIDED_CHECKED: &[&str] = &[
    "crates/od-store/src/store.rs",
    "crates/od-store/src/edit.rs",
    "crates/od-store/src/comments.rs",
    "crates/od-store/src/drafts.rs",
];

/// Calls that decide on a file, and which argument (from 0) names it.
const DECIDE_CALLS: &[(&str, usize)] = &[
    ("files::stat_file(", 1),
    ("files::read_file(", 1),
    ("versions::read_version(", 1),
    ("versions::list_versions(", 1),
    ("versions::refuse_linked_history(", 1),
    ("self.away_before(", 1),
    ("holds(", 1),
    ("file_ident(", 1),
    ("fate_spelled(", 2),
];

/// Calls that write or delete a file or its history, and which argument
/// names it.
const ACT_CALLS: &[(&str, usize)] = &[
    ("files::write_file(", 1),
    ("files::write_file_exact(", 1),
    ("files::create_file_exact(", 1),
    ("files::delete_file(", 1),
    ("files::rename_file(", 1),
    ("files::move_folder(", 1),
    ("files::delete_folder(", 1),
    ("self.delete_file_held(", 2),
    ("versions::ensure_current_version(", 1),
    ("settle(", 1),
];

/// Calls that decide on one name only: never where a recorded name and
/// the name on disk may differ ([`Rule::DecidedApart`]).
const ONE_NAME_CALLS: &[&str] = &["fate("];

/// The functions of `lines`: `(signature, body)`, the signature up to the
/// `{` that opens the body.
fn functions(lines: &[&str]) -> Vec<(String, String)> {
    let is_fn = |l: &str| {
        let l = l.trim_start();
        ["fn ", "pub fn ", "pub(crate) fn ", "pub(super) fn "]
            .iter()
            .any(|p| l.starts_with(p))
    };
    let starts: Vec<usize> = (0..lines.len()).filter(|&i| is_fn(lines[i])).collect();
    starts
        .iter()
        .enumerate()
        .map(|(n, &at)| {
            let end = starts.get(n + 1).copied().unwrap_or(lines.len());
            let text = lines[at..end]
                .iter()
                .map(|l| l.trim())
                .collect::<Vec<_>>()
                .join(" ");
            let open = text.find('{').unwrap_or(text.len());
            (text[..open].to_owned(), text[open..].to_owned())
        })
        .collect()
}

/// Each call of `call` in `body` (not its definition, not the tail of a
/// longer name) and its argument `index`, without `&` and spaces.
fn calls_of(body: &str, call: &str, index: usize) -> Vec<(usize, String)> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    body.match_indices(call)
        .filter(|&(at, _)| !body[..at].ends_with("fn ") && !body[..at].ends_with(word))
        .map(|(at, m)| {
            let args = call_args(&body[at + m.len()..]);
            let mut depth = 0usize;
            let mut parts = vec![String::new()];
            for c in args.chars() {
                match c {
                    '(' | '[' | '{' | '<' => depth += 1,
                    ')' | ']' | '}' | '>' => depth = depth.saturating_sub(1),
                    ',' if depth == 0 => {
                        parts.push(String::new());
                        continue;
                    }
                    _ => {}
                }
                if let Some(last) = parts.last_mut() {
                    last.push(c);
                }
            }
            let arg = parts.get(index).map_or("", |a| a.trim());
            (at, arg.trim_start_matches('&').trim().to_owned())
        })
        .collect()
}

/// Whether `name` is a parameter of the function `sig`, or bound in
/// `body` by a `let` whose statement spells it ([`SPELL_CALLS`]).
fn is_spelled(sig: &str, body: &str, name: &str) -> bool {
    let param = [format!("({name}:"), format!(" {name}:")];
    if param.iter().any(|p| sig.contains(p.as_str())) {
        return true;
    }
    [format!("let {name} ="), format!("let mut {name} =")]
        .iter()
        .flat_map(|l| body.match_indices(l.as_str()).map(|(at, _)| at))
        .any(|at| {
            let stmt = &body[at..body[at..].find(';').map_or(body.len(), |e| at + e)];
            SPELL_CALLS.iter().any(|c| stmt.contains(c))
        })
}

/// Each function of `lines` that decides on a file under one name and
/// writes or deletes it under another, or under one not spelled, and
/// each call of a [`ONE_NAME_CALLS`] call: `"<signature> <what>"`.
fn decided_apart(lines: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for (sig, body) in functions(lines) {
        let head = sig.split('(').next().unwrap_or(&sig).to_owned() + "(";
        for call in ONE_NAME_CALLS {
            for (_, arg) in calls_of(&body, call, 1) {
                out.push(format!("{head} {call}{arg}, ..)"));
            }
        }
        let acts: Vec<(usize, String)> = ACT_CALLS
            .iter()
            .flat_map(|(c, i)| calls_of(&body, c, *i))
            .collect();
        if acts.is_empty() {
            continue;
        }
        let mut names: Vec<String> = DECIDE_CALLS
            .iter()
            .flat_map(|(c, i)| calls_of(&body, c, *i))
            .chain(acts)
            .map(|(_, arg)| arg)
            .collect();
        names.sort();
        names.dedup();
        if names.len() > 1 {
            out.push(format!("{head} decides and acts on {names:?}"));
        } else if let Some(name) = names.first()
            && !is_spelled(&sig, &body, name)
        {
            out.push(format!("{head} acts on `{name}`, not spelled"));
        }
    }
    out
}

/// Where the name of the call at `at` is first used, when the call is
/// followed by `.map(|x| …)` (after `?`, `.ok()`): the first mention of
/// `x` in it. `None` with no such closure.
/// `sanitize_path(to).ok().map(|to| renamed_as(root, from, &to))`.
fn closure_use(body: &str, at: usize) -> Option<usize> {
    let open = at + body[at..].find('(').map_or(0, |i| i + 1);
    let mut rest = &body[open + call_args(&body[open..]).len() + 1..];
    loop {
        let r = rest.trim_start();
        if let Some(r) = r.strip_prefix('?').or_else(|| r.strip_prefix(".ok()")) {
            rest = r;
        } else {
            rest = r;
            break;
        }
    }
    let closure = rest.strip_prefix(".map(|")?;
    let bar = closure.find('|')?;
    let name = closure[..bar].trim();
    let from = body.len() - closure.len() + bar + 1;
    next_mention(body, name, from)
}

/// Whether `at` in `body` lies in the name a [`SPELL_CALLS`] call spells:
/// its last argument (`as_on_disk(dir, NAME)`, `renamed_as(root, old,
/// NAME)`; the `old` of `renamed_as` is not spelled by it).
fn in_spelled_argument(body: &str, at: usize) -> bool {
    SPELL_CALLS.iter().any(|call| {
        body.match_indices(call).any(|(q, m)| {
            let start = q + m.len();
            let args = call_args(&body[start..]);
            let mut depth = 0usize;
            let mut last = 0;
            for (i, c) in args.char_indices() {
                match c {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth = depth.saturating_sub(1),
                    // A comma rustfmt puts after the last argument starts none.
                    ',' if depth == 0 && !args[i + 1..].trim().is_empty() => last = i + 1,
                    _ => {}
                }
            }
            (start + last..start + args.len()).contains(&at)
        })
    })
}

/// The `let NAME =` whose statement holds `at`: the closest `let` before
/// it with no `;` between them outside braces opened since. Returns the
/// name and where its statement ends.
fn binding_at(body: &str, at: usize) -> Option<(&str, usize)> {
    let let_at = body[..at].rfind("let ")?;
    let after = body[let_at + 4..].trim_start_matches("mut ");
    let name_len = after
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    let name_at = body.len() - after.len();
    if name_len == 0 || !body[name_at + name_len..].trim_start().starts_with('=') {
        return None;
    }
    let mut depth = 0i32;
    for (i, c) in body[let_at..].char_indices() {
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ';' if depth <= 0 => {
                let end = let_at + i;
                return (end > at).then_some((&body[name_at..name_at + name_len], end));
            }
            _ => {}
        }
    }
    None
}

/// Where `name` is next mentioned in `body` from `from` on, as a whole
/// word, leaving out a new `let name` (which only shadows it).
fn next_mention(body: &str, name: &str, from: usize) -> Option<usize> {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    body[from..]
        .match_indices(name)
        .map(|(i, _)| from + i)
        .find(|&i| {
            !body[..i].ends_with(word)
                && !body[i + name.len()..].starts_with(word)
                && !body[..i].ends_with("let ")
                && !body[..i].ends_with("let mut ")
        })
}

/// Each use of a [`NAME_CALLS`] result in `lines` before it is spelled,
/// as `"<signature> <statement>"`: the call is in the spelled argument of
/// a [`SPELL_CALLS`] call, or bound by `let x =` and `x` next mentioned
/// there; else the name is used as sent.
fn unspelled_names(lines: &[&str]) -> Vec<String> {
    let is_fn = |l: &str| {
        let l = l.trim_start();
        ["fn ", "pub fn ", "pub(crate) fn ", "pub(super) fn "]
            .iter()
            .any(|p| l.starts_with(p))
    };
    let starts: Vec<usize> = (0..lines.len()).filter(|&i| is_fn(lines[i])).collect();
    let mut out = Vec::new();
    for (n, &at) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(lines.len());
        let body = lines[at + 1..end]
            .iter()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" ");
        let calls = NAME_CALLS
            .iter()
            .flat_map(|c| body.match_indices(c).map(|(i, _)| i))
            .filter(|&i| !body[..i].ends_with("fn "));
        for call in calls {
            if in_spelled_argument(&body, call) {
                continue;
            }
            let used = match (closure_use(&body, call), binding_at(&body, call)) {
                (Some(i), _) => Some(i),
                (None, Some((name, stmt_end))) => next_mention(&body, name, stmt_end),
                (None, None) => Some(call),
            }
            .filter(|&i| !in_spelled_argument(&body, i));
            if let Some(i) = used {
                let stmt_start = body[..i].rfind([';', '{', '}']).map_or(0, |s| s + 1);
                let stmt_end = body[i..]
                    .find([';', '{', '}'])
                    .map_or(body.len(), |e| i + e);
                out.push(format!(
                    "{} {}",
                    lines[at].trim(),
                    body[stmt_start..stmt_end].trim()
                ));
            }
        }
    }
    out
}

/// The operand that ends `before`: back to an open bracket, `,`, `;`,
/// `{`, `|`, `&` or `=` outside brackets.
fn left_operand(before: &str) -> &str {
    let mut depth = 0usize;
    for (i, c) in before.char_indices().rev() {
        match c {
            ')' | ']' => depth += 1,
            '(' | '[' if depth == 0 => return &before[i + 1..],
            '(' | '[' => depth -= 1,
            ',' | ';' | '{' | '|' | '&' | '=' if depth == 0 => return &before[i + 1..],
            _ => {}
        }
    }
    before
}

/// The operand that starts `after`: up to a close bracket, `,`, `;`,
/// `{`, `}`, `|` or `&&` outside brackets.
fn right_operand(after: &str) -> &str {
    let mut depth = 0usize;
    for (i, c) in after.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' if depth == 0 => return &after[..i],
            ')' | ']' => depth -= 1,
            ',' | ';' | '{' | '}' | '|' if depth == 0 => return &after[..i],
            '&' if depth == 0 && after[i + 1..].starts_with('&') => return &after[..i],
            _ => {}
        }
    }
    after
}

/// The text up to the `)` that closes a call whose `(` was just read.
fn call_args(rest: &str) -> &str {
    let mut depth = 0usize;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return &rest[..i],
            ')' => depth -= 1,
            _ => {}
        }
    }
    rest
}

const RULES: [Rule; 3] = [
    Rule::OwnNormalization,
    Rule::LowercaseKey,
    Rule::FoldedCompare,
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The code of a source file, without its `#[cfg(test)]` module and
/// files of tests: a test may set up two names alike on purpose.
fn code_lines(text: &str) -> impl Iterator<Item = &str> {
    // Any line end: a Windows checkout has `\r\n`.
    let end = text
        .match_indices("#[cfg(test)]")
        .map(|(at, m)| at + m.len())
        .find(|&after| text[after..].trim_start().starts_with("mod "))
        .map_or(text.len(), |after| after - "#[cfg(test)]".len());
    text[..end]
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .filter(|l| !l.trim().is_empty())
}

/// `lines` joined into statements: a line ending in `;`, `{` or `}`, or in
/// `,` outside brackets (a match arm, a field), ends one. A compare that
/// rustfmt put on two lines (`a.to_lowercase()` / `== b`) is one text.
fn statements<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut stmt = String::new();
    let mut depth = 0i32;
    for line in lines {
        let line = line.trim();
        if !stmt.is_empty() {
            stmt.push(' ');
        }
        stmt.push_str(line);
        for c in line.chars() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                _ => {}
            }
        }
        let ends = line.ends_with([';', '{', '}']) || (line.ends_with(',') && depth <= 0);
        if ends {
            out.push(std::mem::take(&mut stmt));
            depth = 0;
        }
    }
    if !stmt.is_empty() {
        out.push(stmt);
    }
    out
}

/// What one source file `rel` (from the repo root) holding `text` is
/// found to do: `(rule, line)`.
fn scan_file(rel: &str, text: &str) -> Vec<(Rule, String)> {
    let mut found = Vec::new();
    for stmt in statements(code_lines(text)) {
        for rule in RULES {
            if finds(rule, &stmt) {
                found.push((rule, stmt.clone()));
            }
        }
    }
    let lines: Vec<&str> = code_lines(text).collect();
    if WRITES_CHECKED.contains(&rel) {
        for sig in unspelled_writes(&lines) {
            found.push((Rule::UnspelledWrite, sig));
        }
    }
    if NAMES_CHECKED.contains(&rel) {
        for site in unspelled_names(&lines) {
            found.push((Rule::UnspelledName, site));
        }
    }
    if DECIDED_CHECKED.contains(&rel) {
        for site in decided_apart(&lines) {
            found.push((Rule::DecidedApart, site));
        }
    }
    found
}

/// Every hand-written check found: `(file, rule)` → the lines.
fn scan(repo: &Path) -> BTreeMap<(String, Rule), Vec<String>> {
    let mut files = Vec::new();
    for top in ["crates", "apps"] {
        for member in std::fs::read_dir(repo.join(top)).unwrap() {
            rust_files(&member.unwrap().path().join("src"), &mut files);
        }
    }
    assert!(files.len() > 200, "no sources under {}", repo.display());
    let mut found: BTreeMap<(String, Rule), Vec<String>> = BTreeMap::new();
    for file in files {
        let rel = file
            .strip_prefix(repo)
            .unwrap()
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if rel.ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (rule, site) in scan_file(&rel, &text) {
            found.entry((rel.clone(), rule)).or_default().push(site);
        }
    }
    found
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_check_whether_a_name_is_taken_goes_through_name_key() {
    let found = scan(&repo());
    let mut unlisted = Vec::new();
    for ((file, rule), lines) in &found {
        for line in lines {
            let kept = KEPT
                .iter()
                .any(|(f, text, r, _)| f == file && r == rule && line.contains(text));
            if !kept {
                unlisted.push(format!("{file} [{rule:?}]: {line}"));
            }
        }
    }
    assert!(
        unlisted.is_empty(),
        "compare file names with `name_key` (od_contracts::name_key, \
         od_store::paths::name_key), or list the site in KEPT with why it is \
         no check whether a name is taken:\n{}",
        unlisted.join("\n")
    );
}

/// A site in [`KEPT`] that is gone, or no longer found, goes from the
/// list too: a stale entry would let a new check of that text through.
#[test]
fn every_kept_site_is_still_there() {
    let found = scan(&repo());
    let stale: Vec<_> = KEPT
        .iter()
        .filter(|(f, text, r, _)| {
            !found
                .get(&((*f).to_owned(), *r))
                .is_some_and(|lines| lines.iter().any(|l| l.contains(text)))
        })
        .map(|(f, text, r, _)| format!("{f} [{r:?}]: {text}"))
        .collect();
    assert!(
        stale.is_empty(),
        "stale KEPT entries:\n{}",
        stale.join("\n")
    );
}

/// Each rule finds the checks it is for, and leaves alone what is not one.
#[test]
fn each_rule_finds_its_pattern() {
    let cases: &[(Rule, &str, bool)] = &[
        (
            Rule::OwnNormalization,
            "name.nfc().collect::<String>()",
            true,
        ),
        (Rule::OwnNormalization, "s.nfd().count()", true),
        (Rule::OwnNormalization, "name_key(name)", false),
        (
            Rule::LowercaseKey,
            "if let Some(first) = seen.insert(path.to_lowercase(), path) {",
            true,
        ),
        (
            Rule::LowercaseKey,
            "taken.contains(&name.to_ascii_lowercase())",
            true,
        ),
        (
            Rule::LowercaseKey,
            "seen.insert(name_key(path), path)",
            false,
        ),
        (
            Rule::LowercaseKey,
            "let lower = name.to_lowercase(); out.insert(x)",
            false,
        ),
        (
            Rule::FoldedCompare,
            "existing.iter().any(|e| e.eq_ignore_ascii_case(p))",
            true,
        ),
        (
            Rule::FoldedCompare,
            "if file.to_lowercase() == other.to_lowercase() {",
            true,
        ),
        (
            Rule::FoldedCompare,
            "key.eq_ignore_ascii_case(\"t\") && name",
            false,
        ),
        (Rule::FoldedCompare, "a.eq_ignore_ascii_case(b)", true),
        (
            Rule::FoldedCompare,
            "v.to_string_lossy().to_lowercase() == n.to_string_lossy().to_lowercase()",
            true,
        ),
        (Rule::FoldedCompare, "a.to_lowercase() != b", true),
        (Rule::FoldedCompare, "x == y.to_ascii_lowercase()", true),
        (
            Rule::FoldedCompare,
            "p.category == cat).filter(|p| p.name.to_lowercase().contains(&q))",
            false,
        ),
        (
            Rule::FoldedCompare,
            "name_key(&v.to_string_lossy()) == name_key(&n.to_string_lossy())",
            false,
        ),
    ];
    for (rule, line, want) in cases {
        assert_eq!(finds(*rule, line), *want, "{rule:?} on {line}");
    }
}

/// A compare rustfmt split over lines is one statement to the scan.
#[test]
fn a_compare_over_lines_is_one_statement() {
    let text =
        "let same = v.to_string_lossy().to_lowercase()\n    == n.to_lowercase();\nlet x = 1;\n";
    let stmts = statements(code_lines(text));
    assert_eq!(stmts.len(), 2, "{stmts:?}");
    assert!(finds(Rule::FoldedCompare, &stmts[0]));
    assert!(!finds(Rule::FoldedCompare, &stmts[1]));
    // A match arm ends at its `,`, so arms are not read as one.
    let arms = statements(code_lines("A => a.to_lowercase(),\nB => b == c,\n"));
    assert_eq!(arms.len(), 2, "{arms:?}");
    assert!(arms.iter().all(|s| !finds(Rule::FoldedCompare, s)));
}

/// A write in `files.rs` that picks its target before spelling it as on
/// disk is found; spelling first, or no write, is not.
#[test]
fn a_write_before_the_name_is_spelled_is_found() {
    let text = "\
pub fn raw(root: &ProjectRoot, name: &str) -> Result<()> {
    fs::write(root.dir.join(name), b\"x\")?;
    Ok(())
}
pub fn copied(root: &ProjectRoot, name: &str) -> Result<()> {
    fs::copy(root.dir.join(\"a\"), root.dir.join(name))?;
    Ok(())
}
pub fn created(root: &ProjectRoot, name: &str) -> Result<()> {
    File::create(root.dir.join(name))?;
    Ok(())
}
pub fn late(root: &ProjectRoot, name: &str) -> Result<()> {
    let target = root.resolve(name)?;
    let rel = as_on_disk(&root.dir, name);
    Ok(())
}
pub fn spelled(root: &ProjectRoot, name: &str) -> Result<()> {
    let rel = as_on_disk(&root.dir, name);
    let target = root.resolve(&rel)?;
    write_atomic(&target, b\"x\", Place::CreateNew)
}
fn write_atomic(target: &Path, bytes: &[u8], place: Place) -> Result<()> {
    Ok(())
}
";
    let lines: Vec<&str> = code_lines(text).collect();
    assert_eq!(
        unspelled_writes(&lines),
        [
            "pub fn raw(root: &ProjectRoot, name: &str) -> Result<()> {",
            "pub fn copied(root: &ProjectRoot, name: &str) -> Result<()> {",
            "pub fn created(root: &ProjectRoot, name: &str) -> Result<()> {",
            "pub fn late(root: &ProjectRoot, name: &str) -> Result<()> {",
        ]
    );
}

/// A name sent that is used before it is spelled as on disk is found,
/// each one apart: spelling the target does not cover the source.
#[test]
fn a_name_used_before_it_is_spelled_is_found() {
    let text = "\
pub fn direct(root: &ProjectRoot, name: &str) -> Result<()> {
    if through_link(&root.dir, &validate_project_path(name)?) {
        return Err(StoreError::FileNotFound);
    }
    Ok(())
}
pub fn bound(root: &ProjectRoot, path: &str) -> Result<()> {
    let rel = validate_project_path(path)?;
    let full = root.resolve(&rel)?;
    Ok(())
}
pub fn source(root: &ProjectRoot, from: &str, to: &str) -> Result<()> {
    let old = validate_project_path(from)?;
    let new = renamed_as(root, &old, &sanitize_path(to)?);
    Ok(())
}
pub fn spelled(root: &ProjectRoot, from: &str, to: &str) -> Result<()> {
    let old = as_on_disk(&root.dir, &validate_project_path(from)?);
    forget(&as_on_disk(
        &root.dir,
        &validate_project_path(from)?,
    ));
    let new = renamed_as(root, &old, &sanitize_path(to)?);
    let rel = sanitize_path(to)?;
    let rel = as_on_disk(&root.dir, &rel);
    let dir = match to {
        \"\" => String::new(),
        d => {
            check(d)?;
            as_on_disk(&root.dir, &sanitize_path(d)?)
        }
    };
    Ok(())
}
";
    let lines: Vec<&str> = code_lines(text).collect();
    let found = unspelled_names(&lines);
    let fns: Vec<&str> = found
        .iter()
        .map(|f| f.split('(').next().unwrap_or(f))
        .collect();
    assert_eq!(
        fns,
        ["pub fn direct", "pub fn bound", "pub fn source"],
        "{found:#?}"
    );
    assert!(
        found[1].ends_with("let full = root.resolve(&rel)?"),
        "{found:#?}"
    );
    assert!(found[2].contains("renamed_as(root, &old,"), "{found:#?}");
}

/// A file whose writes are checked is scanned by both of its rules, and
/// another file by neither.
#[test]
fn a_checked_file_is_scanned_for_unspelled_writes_and_names() {
    let text = "\
pub fn delete(root: &ProjectRoot, name: &str) -> Result<()> {
    fs::rename(root.dir.join(validate_project_path(name)?), root.dir.join(\"x\"))?;
    Ok(())
}
";
    let rules = |rel: &str| {
        scan_file(rel, text)
            .into_iter()
            .map(|(rule, _)| rule)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rules(WRITES_CHECKED[0]),
        [Rule::UnspelledWrite, Rule::UnspelledName]
    );
    assert_eq!(rules("crates/od-store/src/store.rs"), [Rule::UnspelledName]);
    assert_eq!(rules("crates/od-server/src/routes/files.rs"), []);
    let apart = "\
fn delete(root: &ProjectRoot, path: &str, spelled: &str) -> bool {
    let fate = fate(root, spelled, None, 0);
    if file_ident(root, spelled).is_none() {}
    settle(root, path, fate)
}
";
    let rules = |rel: &str| {
        scan_file(rel, apart)
            .into_iter()
            .map(|(rule, _)| rule)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rules("crates/od-store/src/drafts.rs"),
        [Rule::DecidedApart, Rule::DecidedApart]
    );
    assert_eq!(rules(WRITES_CHECKED[0]), []);
}

/// A name spelled in a closure the call is mapped through is spelled; one
/// the closure uses as sent is not.
#[test]
fn a_name_spelled_in_its_closure_is_spelled() {
    let text = "\
pub fn mapped(root: &ProjectRoot, name: &str) -> Result<()> {
    let safe = sanitize_path(name).ok().map(|safe| as_on_disk(&root.dir, &safe));
    if let Ok(exact) = validate_project_path(name).map(|exact| as_on_disk(&root.dir, &exact)) {}
    let to = sanitize_path(name)?.map(|to| renamed_as(root, &old, &to));
    Ok(())
}
pub fn used(root: &ProjectRoot, name: &str) -> Result<()> {
    let safe = sanitize_path(name).ok().map(|safe| stat_file(root, &safe));
    Ok(())
}
pub fn source(root: &ProjectRoot, name: &str) -> Result<()> {
    let old = sanitize_path(name).ok().map(|old| renamed_as(root, &old, &new));
    Ok(())
}
";
    let lines: Vec<&str> = code_lines(text).collect();
    let fns: Vec<String> = unspelled_names(&lines)
        .iter()
        .map(|f| f.split('(').next().unwrap_or(f).to_owned())
        .collect();
    assert_eq!(fns, ["pub fn used", "pub fn source"]);
}

/// A function that decides on a file and acts on it under two names, or
/// under one not spelled, is found, and so is every call of `fate(`; one
/// name spelled or a parameter is not.
#[test]
fn a_decision_apart_from_its_act_is_found() {
    let text = "\
fn same(root: &ProjectRoot, path: &str) -> bool {
    let spelled = as_on_disk(&root.dir, path);
    if file_ident(root, &spelled).is_some() {}
    let fate = fate_spelled(root, path, &spelled, None, 0);
    settle(root, &spelled, fate)
}
fn apart(root: &ProjectRoot, path: &str) -> bool {
    let spelled = as_on_disk(&root.dir, path);
    let fate = fate_spelled(root, path, &spelled, None, 0);
    settle(root, path, fate)
}
pub fn unspelled(&self, root: &ProjectRoot, name: &str) -> Result<()> {
    let safe = validate_project_path(name)?;
    let (_, now) = files::read_file(&root, &safe)?;
    files::write_file_exact(&root, &safe, now.as_bytes())?;
    Ok(())
}
fn one_name(root: &ProjectRoot, path: &str) -> bool {
    let spelled = as_on_disk(&root.dir, path);
    let fate = fate(root, &spelled, None, 0);
    settle(root, &spelled, fate)
}
fn param(root: &ProjectRoot, name: &str, before: Option<&[u8]>) -> Result<()> {
    match before {
        Some(bytes) => files::write_file_exact(root, name, bytes).map(drop),
        None => files::delete_file(root, name).map(drop),
    }
}
fn history(root: &ProjectRoot, name: &str) -> Result<()> {
    let safe = validate_project_path(name)?;
    versions::ensure_current_version(&root.dir, &safe, \"\", manual())?;
    Ok(())
}
fn decides_only(root: &ProjectRoot, a: &str, b: &str) -> bool {
    files::stat_file(root, a).is_ok() && holds(root, b, \"x\") == Holds::Same
}
";
    let lines: Vec<&str> = code_lines(text).collect();
    let found = decided_apart(&lines);
    let fns: Vec<&str> = found
        .iter()
        .map(|f| f.split('(').next().unwrap_or(f))
        .collect();
    assert_eq!(
        fns,
        ["fn apart", "pub fn unspelled", "fn one_name", "fn history"],
        "{found:#?}"
    );
    assert!(found[0].ends_with(r#"["path", "spelled"]"#), "{found:#?}");
    assert!(found[2].contains("fate(spelled"), "{found:#?}");
}

/// Each file a rule checks by function is there: a moved file would leave
/// the rule checking nothing.
#[test]
fn every_file_whose_writes_are_checked_is_there() {
    for file in WRITES_CHECKED
        .iter()
        .chain(NAMES_CHECKED)
        .chain(DECIDED_CHECKED)
    {
        assert!(repo().join(file).is_file(), "{file} is gone");
    }
}

/// The `#[cfg(test)]` module is left out on a Windows checkout too, whose
/// lines end in `\r\n`: its tests write files by any name.
#[test]
fn a_test_module_is_left_out_whatever_the_line_end() {
    for end in ["\n", "\r\n"] {
        let text = [
            "fn code() {}",
            "#[cfg(test)]",
            "mod tests {",
            "fn t() {}",
            "}",
        ]
        .join(end);
        let lines: Vec<&str> = code_lines(&text).collect();
        assert_eq!(lines, ["fn code() {}"], "{end:?}");
    }
}
