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
//! on Linux: [`Rule::UnspelledWrite`].

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
        "fn delete_file(",
        Rule::UnspelledWrite,
        "deletes the file at the name; makes no second file",
    ),
    (
        "crates/od-store/src/files.rs",
        "fn delete_folder(",
        Rule::UnspelledWrite,
        "deletes the folder at the name; makes no second folder",
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
        Rule::UnspelledWrite => false,
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
        for stmt in statements(code_lines(&text)) {
            for rule in RULES {
                if finds(rule, &stmt) {
                    found
                        .entry((rel.clone(), rule))
                        .or_default()
                        .push(stmt.clone());
                }
            }
        }
        if WRITES_CHECKED.contains(&rel.as_str()) {
            let lines: Vec<&str> = code_lines(&text).collect();
            for sig in unspelled_writes(&lines) {
                found
                    .entry((rel.clone(), Rule::UnspelledWrite))
                    .or_default()
                    .push(sig);
            }
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
    let arms = "A => a.to_lowercase(),\nB => b == c,\n";
    assert!(
        statements(code_lines(arms))
            .iter()
            .all(|s| !finds(Rule::FoldedCompare, s))
    );
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
            "pub fn late(root: &ProjectRoot, name: &str) -> Result<()> {",
        ]
    );
}

/// Each file [`Rule::UnspelledWrite`] checks is there: a moved file would
/// leave the rule checking nothing.
#[test]
fn every_file_whose_writes_are_checked_is_there() {
    for file in WRITES_CHECKED {
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
