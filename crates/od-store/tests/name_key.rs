//! Whether a user's file or folder name is taken is decided one way, by
//! `od_contracts::name_key` (re-exported as `od_store::paths::name_key`):
//! NFC and case ignored, as macOS and Windows disks do. A check written by
//! hand (`to_lowercase` as a set key, `eq_ignore_ascii_case` on a path, an
//! own NFC) differs from the disk on some OS: on Linux two files that are
//! one on a Mac, on a Mac a file written over (#225, #242, #94).
//!
//! This test scans every `.rs` under `crates/*/src` and `apps/*/src` for
//! such checks. Each one found either goes through `name_key`, or is
//! listed in [`KEPT`] with why it is no check whether a name is taken.

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
    /// Two names folded by hand: `a.eq_ignore_ascii_case(b)`,
    /// `a.to_lowercase() == b…`, on a line about a path, name or file.
    FoldedCompare,
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
        "crates/od-ui/src/app/api.rs",
        "self.message.to_lowercase().contains(&hint.to_lowercase())",
        Rule::LowercaseKey,
        "finds a hint in an error message, not a file",
    ),
];

/// Words on a line that make a folded compare one about files.
const FILE_WORDS: &[&str] = &[
    "path", "name", "file", "dir", "folder", "segment", "existing", "entry", "rel", "have",
];

/// What `rule` finds on `line` (comments left out by the caller).
fn finds(rule: Rule, line: &str) -> bool {
    let lower_call = |s: &str| s.contains("to_lowercase()") || s.contains("to_ascii_lowercase()");
    match rule {
        Rule::OwnNormalization => [".nfc()", ".nfd()", ".nfkc()", ".nfkd()"]
            .iter()
            .any(|n| line.contains(n)),
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
            line.match_indices(call)
                .any(|(at, _)| lower_call(call_args(&line[at + call.len()..])))
        }),
        Rule::FoldedCompare => {
            let fold = line.match_indices("eq_ignore_ascii_case(").any(|(at, m)| {
                // A fixed string (`"index.html"`, `"t"`) is no user name.
                !line[at + m.len()..].starts_with('"')
            }) || (lower_call(line) && (line.contains("==") || line.contains("!=")));
            let lower = line.to_lowercase();
            fold && FILE_WORDS.iter().any(|w| lower.contains(w))
        }
    }
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
    let end = text.find("#[cfg(test)]\nmod ").unwrap_or(text.len());
    text[..end]
        .lines()
        .map(|l| l.split("//").next().unwrap_or(""))
        .filter(|l| !l.trim().is_empty())
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
        for line in code_lines(&text) {
            for rule in RULES {
                if finds(rule, line) {
                    found
                        .entry((rel.clone(), rule))
                        .or_default()
                        .push(line.trim().to_owned());
                }
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
        (Rule::FoldedCompare, "a.eq_ignore_ascii_case(b)", false),
    ];
    for (rule, line, want) in cases {
        assert_eq!(finds(*rule, line), *want, "{rule:?} on {line}");
    }
}
