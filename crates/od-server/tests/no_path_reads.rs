//! Code that reads the folders an agent can write (the content index in
//! `od-content`, the Library routes: skills, plugins, design systems,
//! templates) reads a file only through `od_content::nofollow`: opened
//! with `O_NONBLOCK` (a FIFO never blocks the read), checked as a regular
//! file on the opened descriptor, and read up to a limit. A plain
//! `std::fs::read`/`read_to_string`, or an open of its own, waits on a FIFO
//! for good and reads past any size (`crates/od-server/CLAUDE.md`,
//! invariant 15). This test reads the sources as tokens (comments and
//! strings left out, so line breaks, full paths, `use … as` and imports
//! in braces all count) and refuses every such read outside the helper.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The folders scanned, relative to this crate.
const SCANNED: &[&str] = &["../od-content/src", "src/routes/library"];

/// The helper itself (relative to this crate).
const HELPER: &str = "../od-content/src/nofollow.rs";

/// Reads that stay: file (relative to this crate), rule, how many, why.
const ALLOWED: &[(&str, &str, usize, &str)] = &[
    (
        "src/routes/library/design_systems.rs",
        "OpenOptions",
        2,
        "writes only (`create_new` of the sent names' temp file and of an uploaded asset), never a read",
    ),
    (
        "src/routes/library/templates.rs",
        "read_file",
        1,
        "`Store::read_file` of a project's own files (Save as template): the project-file reads in `od-store` are a class of their own, not the Library's folders",
    ),
    (
        "src/routes/library/assets.rs",
        "read_file",
        1,
        "`Store::read_file` of a project's media (assets chat): project files, as above",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tok {
    text: String,
    line: usize,
}

/// The tokens of `src`: identifiers, `::`, and single punctuation;
/// comments, strings, chars and numbers left out.
fn tokens(src: &str) -> Vec<Tok> {
    let b = src.as_bytes();
    let (mut i, mut line, mut out) = (0, 1, Vec::new());
    let ident = |c: u8| c == b'_' || c.is_ascii_alphanumeric();
    // Skips a string body starting after its opening quote(s).
    let skip_str = |i: &mut usize, line: &mut usize, hashes: Option<usize>| {
        while *i < b.len() {
            match (b[*i], hashes) {
                (b'\n', _) => *line += 1,
                (b'\\', None) => *i += 1,
                (b'"', None) => {
                    *i += 1;
                    return;
                }
                (b'"', Some(n)) if b[*i + 1..].iter().take(n).all(|&c| c == b'#') => {
                    *i += 1 + n;
                    return;
                }
                _ => {}
            }
            *i += 1;
        }
    };
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            i += 1;
        } else if c.is_ascii_whitespace() {
            i += 1;
        } else if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            let mut depth = 0;
            while i < b.len() {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if b[i] == b'\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if c == b'"' {
            i += 1;
            skip_str(&mut i, &mut line, None);
        } else if (c == b'r' || c == b'b')
            && (b[i..].starts_with(b"r\"")
                || b[i..].starts_with(b"r#")
                || b[i..].starts_with(b"br\"")
                || b[i..].starts_with(b"br#")
                || b[i..].starts_with(b"b\"")
                || b[i..].starts_with(b"b'"))
            && (i == 0 || !ident(b[i - 1]))
        {
            if b[i] == b'b' {
                i += 1;
            }
            if b[i] == b'\'' {
                // A byte char: b'x', b'\n'.
                i += if b[i + 1] == b'\\' { 4 } else { 3 };
                while i < b.len() && b[i - 1] != b'\'' {
                    i += 1;
                }
            } else if b[i] == b'"' {
                i += 1;
                skip_str(&mut i, &mut line, None);
            } else {
                i += 1; // the `r`
                let n = b[i..].iter().take_while(|&&c| c == b'#').count();
                i += n + 1; // the hashes and the quote
                skip_str(&mut i, &mut line, Some(n));
            }
        } else if c == b'\'' {
            // A char ('x', '\n', '\u{..}') or a lifetime ('a).
            if b.get(i + 1) == Some(&b'\\') {
                i += 2;
                while i < b.len() && b[i] != b'\'' {
                    i += 1;
                }
                i += 1;
            } else {
                let ch = src[i + 1..].chars().next().map_or(1, char::len_utf8);
                if b.get(i + 1 + ch) == Some(&b'\'') {
                    i += 2 + ch;
                } else {
                    i += 1; // a lifetime: its name is read as an identifier
                }
            }
        } else if ident(c) {
            let start = i;
            while i < b.len() && ident(b[i]) {
                i += 1;
            }
            if !c.is_ascii_digit() {
                out.push(Tok {
                    text: src[start..i].to_owned(),
                    line,
                });
            }
        } else if b[i..].starts_with(b"::") {
            out.push(Tok {
                text: "::".to_owned(),
                line,
            });
            i += 2;
        } else {
            let ch = src[i..].chars().next().map_or(1, char::len_utf8);
            out.push(Tok {
                text: src[i..i + ch].to_owned(),
                line,
            });
            i += ch;
        }
    }
    out
}

/// `toks` without the items under `#[cfg(test)]` (or `cfg(all(test, …))`):
/// tests read their own fixtures.
fn without_tests(toks: &[Tok]) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if toks[i].text == "#" && toks.get(i + 1).is_some_and(|t| t.text == "[") {
            // The attribute's tokens, to its `]`.
            let mut j = i + 2;
            let mut depth = 1;
            while j < toks.len() && depth > 0 {
                match toks[j].text.as_str() {
                    "[" => depth += 1,
                    "]" => depth -= 1,
                    _ => {}
                }
                j += 1;
            }
            let attr: Vec<&str> = toks[i + 2..j - 1].iter().map(|t| t.text.as_str()).collect();
            let test_only =
                attr.first() == Some(&"cfg") && attr.contains(&"test") && !attr.contains(&"not");
            if !test_only {
                out.extend_from_slice(&toks[i..j]);
                i = j;
                continue;
            }
            // Skip the item: to a `;` at depth 0, or past its `{ … }`.
            let mut depth = 0;
            while j < toks.len() {
                match toks[j].text.as_str() {
                    "{" | "(" | "[" => depth += 1,
                    "}" | ")" | "]" => {
                        depth -= 1;
                        if depth == 0 && toks[j].text == "}" {
                            j += 1;
                            break;
                        }
                    }
                    ";" if depth == 0 => {
                        j += 1;
                        break;
                    }
                    _ => {}
                }
                j += 1;
            }
            i = j;
            continue;
        }
        out.push(toks[i].clone());
        i += 1;
    }
    out
}

/// The reads in `toks`, by rule, with their lines.
fn reads(toks: &[Tok]) -> Vec<(&'static str, usize)> {
    let at = |i: usize, s: &str| toks.get(i).is_some_and(|t| t.text == s);
    // Whether the `{ … }` group starting at `i` names one of `names`.
    let group_names = |i: usize, names: &[&str]| {
        let mut depth = 0;
        for u in &toks[i..] {
            match u.text.as_str() {
                "{" => depth += 1,
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                w if names.contains(&w) => return true,
                _ => {}
            }
        }
        false
    };
    let mut found = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        let rule = match t.text.as_str() {
            // `std::fs::read(…)`, `tokio::fs::read`, `.and_then(fs::read)`,
            // `use std::fs::read [as r]`, and a glob of `fs`.
            "fs" if at(i + 1, "::") && (at(i + 2, "read") || at(i + 2, "*")) => "fs::read",
            // `std::fs::copy`, `tokio::fs::copy`, `use std::fs::copy [as c]`:
            // it opens the source by path, waits on a FIFO, has no limit.
            "fs" if at(i + 1, "::") && at(i + 2, "copy") => "fs::copy",
            // `use std::fs as f` / `use std::{fs as f}`: then `f::read`.
            "fs" if at(i + 1, "as") => "fs as",
            // `use std::fs::{read, copy, …}` / `{self as f}`.
            "fs" if at(i + 1, "::") && at(i + 2, "{") => {
                if !group_names(i + 2, &["read", "copy", "as"]) {
                    continue;
                }
                "fs::{read}"
            }
            // `od_store::files::read_file` (a `fs::read` behind it), by its
            // path, imported (`files::read_file [as r]`, `files::{read_file}`,
            // `files::*`, `files as f`), or the `Store` method
            // (`s.read_file(…)`, `Store::read_file`). `nofollow::read_file`
            // is the helper.
            "read_file"
                if i > 0
                    && (toks[i - 1].text == "."
                        || (toks[i - 1].text == "::"
                            && i > 1
                            && toks[i - 2].text != "nofollow")) =>
            {
                "read_file"
            }
            "files" if at(i + 1, "::") && at(i + 2, "*") => "read_file",
            "files" if at(i + 1, "::") && at(i + 2, "{") => {
                if !group_names(i + 2, &["read_file", "as"]) {
                    continue;
                }
                "read_file"
            }
            "files" if at(i + 1, "as") => "read_file",
            // `fs::read_to_string`, `io::read_to_string`, `file.read_to_string`.
            "read_to_string" | "read_to_end" => "read_to_string",
            // An open of its own (no `O_NONBLOCK`, no check on the handle).
            "File" if at(i + 1, "::") && (at(i + 2, "open") || at(i + 2, "options")) => {
                "File::open"
            }
            "File" if at(i + 1, "as") => "File as",
            "OpenOptions" => "OpenOptions",
            _ => continue,
        };
        found.push((rule, t.line));
    }
    found
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn library_code_reads_files_only_through_nofollow() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found: BTreeMap<(String, &str), Vec<String>> = BTreeMap::new();
    for dir in SCANNED {
        let mut files = Vec::new();
        rust_files(&here.join(dir), &mut files);
        assert!(files.len() > 3, "no sources under {dir}");
        for file in files {
            let rel = format!(
                "{dir}/{}",
                file.strip_prefix(here.join(dir))
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            );
            if rel == HELPER {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for (rule, line) in reads(&without_tests(&tokens(&text))) {
                found
                    .entry((rel.clone(), rule))
                    .or_default()
                    .push(format!("{rel}:{line}"));
            }
        }
    }
    let mut wrong = Vec::new();
    for ((file, rule), lines) in &found {
        let allowed = ALLOWED
            .iter()
            .find(|(f, r, _, _)| f == file && r == rule)
            .map_or(0, |a| a.2);
        if lines.len() != allowed {
            wrong.push(format!(
                "{file}: {} `{rule}` (allowed {allowed}): {lines:?}",
                lines.len()
            ));
        }
    }
    for (file, rule, n, _) in ALLOWED {
        if !found.contains_key(&((*file).to_owned(), *rule)) {
            wrong.push(format!("{file}: `{rule}` allowed {n}, none left"));
        }
    }
    assert!(
        wrong.is_empty(),
        "read these through `od_content::nofollow` (read_file, read_text) or list them in ALLOWED with why:\n{}",
        wrong.join("\n")
    );
}

/// Every way of writing a read is caught; a comment, a string or a test
/// that names one is not.
#[test]
fn every_spelling_of_a_read_is_caught() {
    let rules = |src: &str| -> Vec<&'static str> {
        reads(&without_tests(&tokens(src)))
            .into_iter()
            .map(|(r, _)| r)
            .collect()
    };
    for (src, rule) in [
        ("let t = std::fs::read_to_string(p);", "read_to_string"),
        ("let b = std::fs::read(p);", "fs::read"),
        ("let b = std::fs\n    ::read(p);", "fs::read"),
        ("let b = ::std::fs::read(p)?;", "fs::read"),
        ("let b = tokio::fs::read(&p).await;", "fs::read"),
        ("x.and_then(std::fs::read)", "fs::read"),
        ("use std::fs::read;", "fs::read"),
        ("use std::fs::read as slurp;", "fs::read"),
        ("use std::fs::*;", "fs::read"),
        ("use std::{fs::{read, write}};", "fs::{read}"),
        ("use std::fs::{\n    self as f,\n};", "fs::{read}"),
        ("use std::fs as f;", "fs as"),
        ("use std::{fs as f, io};", "fs as"),
        ("std::io::read_to_string(file)", "read_to_string"),
        ("file.read_to_end(&mut v)", "read_to_string"),
        ("let f = std::fs::File::open(p)?;", "File::open"),
        ("let f = File::options().read(true).open(p);", "File::open"),
        ("let f = tokio::fs::File::open(p).await;", "File::open"),
        ("use std::fs::File as F;", "File as"),
        ("let o = std::fs::OpenOptions::new();", "OpenOptions"),
        ("use std::fs::OpenOptions as O;", "OpenOptions"),
        ("let _ = std::fs::copy(from, to);", "fs::copy"),
        ("let _ = std::fs\n    ::copy(from, to);", "fs::copy"),
        ("tokio::fs::copy(&a, &b).await?;", "fs::copy"),
        ("use std::fs::copy;", "fs::copy"),
        ("use std::fs::copy as dup;", "fs::copy"),
        ("use std::fs::{copy, write};", "fs::{read}"),
        (
            "let (_, b) = od_store::files::read_file(&root, p)?;",
            "read_file",
        ),
        ("od_store::files\n    ::read_file(&root, p)", "read_file"),
        ("use od_store::files::read_file;", "read_file"),
        ("use od_store::files::read_file as rf;", "read_file"),
        (
            "use od_store::files::{ProjectRoot, read_file};",
            "read_file",
        ),
        ("use od_store::files::*;", "read_file"),
        ("use od_store::files as f;", "read_file"),
        ("use od_store::{files::{self as f}};", "read_file"),
        ("let (_, b) = s.read_file(id, name)?;", "read_file"),
        ("let (_, b) = s\n    .read_file(id, name)?;", "read_file"),
        ("Store::read_file(&s, id, name)", "read_file"),
        // After a lifetime and a char, still read.
        (
            "fn f<'a>(c: char) { if c == 'x' { std::fs::read(p); } }",
            "fs::read",
        ),
    ] {
        assert_eq!(rules(src), [rule], "{src}");
    }
    for src in [
        "// std::fs::read_to_string(p)",
        "/* std::fs::read(p) */",
        "/* outer /* std::fs::read(p) */ still a comment */",
        r#"let s = "std::fs::read(p)";"#,
        r##"let s = r#"std::fs::read_to_string(p)"#;"##,
        r##"let s = r#"one " std::fs::read(p)"#;"##,
        r##"let s = br#"one " std::fs::read(p)"#;"##,
        "let c = '\"'; let s = \"fs::read(\";",
        "std::fs::read_dir(p)",
        "nofollow::read_file(p, 10)",
        "od_content::nofollow::read_file(p, 10)",
        "use od_content::nofollow::{read_file, read_text};",
        "crate::routes::export::read_page_file(&root, p)",
        "od_store::files::stat_file(&root, p)",
        "use od_store::files::{ProjectRoot, stat_file};",
        "copy_dir(&from, &to, 10)",
        "// std::fs::copy(a, b) and s.read_file(id, n)",
        "#[cfg(test)]\nmod tests {\n    fn t() { std::fs::read(p); }\n}",
        "#[cfg(all(test, unix))]\nfn t() { std::fs::read_to_string(p); }",
        "#[cfg(test)]\nuse std::fs::read;",
        "std::fs::write(p, b)",
        "std::fs::File::create(p)",
    ] {
        assert!(rules(src).is_empty(), "{src}: {:?}", rules(src));
    }
    // Code outside the test item is still read.
    assert_eq!(
        rules("#[cfg(test)]\nfn t() {}\nfn f() { std::fs::read(p); }"),
        ["fs::read"]
    );
    assert_eq!(
        rules("#[cfg(not(test))]\nfn f() { std::fs::read(p); }"),
        ["fs::read"]
    );
}
