//! Code that reads the folders an agent can write (the content index in
//! `od-content`, the Library routes: skills, plugins, design systems,
//! templates; the content routes: design template examples, craft) reads a file only through `od_content::nofollow`: opened
//! with `O_NONBLOCK` (a FIFO never blocks the read), checked as a regular
//! file on the opened descriptor, and read up to a limit. A plain
//! `std::fs::read`/`read_to_string`/`copy`, an open of its own, or
//! `od_store::files::read_file` (a `fs::read` behind it) waits on a FIFO
//! for good and reads past any size (`crates/od-server/CLAUDE.md`,
//! invariant 15).
//!
//! The sources are parsed as Rust (`syn`), so comments, strings, raw
//! identifiers and line breaks are what the compiler sees. Every path in
//! an expression (a call, a function passed as a value, `<File>::open`) is
//! expanded through the file's `use … [as …]` and `type … = …;` before it
//! is matched; items only built for tests (`#[cfg(test)]`,
//! `#[cfg(all(test, …))]`, `#[test]`) are left out, `cfg(any(test, …))`
//! is not. Code inside a macro call is read when its tokens parse as
//! expressions or statements (`format!`, `vec!`, `assert!` do; `view!`
//! markup does not).

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use syn::ext::IdentExt;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Expr, Meta, Token, UseTree};

/// The folders and files scanned, relative to this crate: the content
/// routes read the design templates' examples (a user template is the
/// agent's to write) and the craft sections.
const SCANNED: &[&str] = &[
    "../od-content/src",
    "src/routes/library",
    "src/routes/content.rs",
];

/// The helper itself (relative to this crate).
const HELPER: &str = "../od-fs/src/nofollow.rs";

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

/// Whether a `cfg` predicate holds only when `test` does: `test`,
/// `all(…, test, …)`, `any(…)` of such predicates only; never `not(…)`.
fn test_only_cfg(meta: &Meta) -> bool {
    match meta {
        Meta::Path(p) => p.is_ident("test"),
        Meta::List(list) => {
            let Ok(nested) = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
            else {
                return false;
            };
            if list.path.is_ident("all") {
                nested.iter().any(test_only_cfg)
            } else if list.path.is_ident("any") {
                !nested.is_empty() && nested.iter().all(test_only_cfg)
            } else {
                false
            }
        }
        Meta::NameValue(_) => false,
    }
}

/// Whether `attrs` make their item one built only for tests.
fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        let path = a.path();
        if path.is_ident("cfg") {
            return a.parse_args::<Meta>().is_ok_and(|m| test_only_cfg(&m));
        }
        // `#[test]`, `#[tokio::test]`.
        path.segments.last().is_some_and(|s| s.ident == "test")
    })
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    use syn::Item;
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

fn name(ident: &syn::Ident) -> String {
    ident.unraw().to_string()
}

/// The names a file gives paths: `use a::b as c` (c → a::b), `use a::b`
/// (b → a::b), `use a::{self as m}` (m → a), `type F = std::fs::File`
/// (F → std::fs::File); and the globs it imports (`use std::fs::*`).
#[derive(Default)]
struct Names {
    aliases: HashMap<String, Vec<String>>,
    globs: Vec<(Vec<String>, usize)>,
}

impl Names {
    fn add_use(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(p) => {
                prefix.push(name(&p.ident));
                self.add_use(&p.tree, prefix);
                prefix.pop();
            }
            UseTree::Name(n) => {
                let full = if n.ident == "self" {
                    prefix.clone()
                } else {
                    [prefix.as_slice(), &[name(&n.ident)]].concat()
                };
                if let Some(last) = full.last().cloned() {
                    self.aliases.insert(last, full);
                }
            }
            UseTree::Rename(r) => {
                let full = if r.ident == "self" {
                    prefix.clone()
                } else {
                    [prefix.as_slice(), &[name(&r.ident)]].concat()
                };
                self.aliases.insert(name(&r.rename), full);
            }
            UseTree::Glob(g) => self
                .globs
                .push((prefix.clone(), g.star_token.span.start().line)),
            UseTree::Group(g) => {
                for t in &g.items {
                    self.add_use(t, prefix);
                }
            }
        }
    }

    /// `segs` with its first segment replaced by what the file names it,
    /// again until it names nothing (at most 8 times: `use a as a`).
    fn expand(&self, mut segs: Vec<String>) -> Vec<String> {
        for _ in 0..8 {
            let Some(full) = segs.first().and_then(|f| self.aliases.get(f)) else {
                break;
            };
            if full.len() == 1 && full[0] == segs[0] {
                break;
            }
            segs = [full.as_slice(), &segs[1..]].concat();
        }
        segs
    }
}

impl<'ast> Visit<'ast> for Names {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if test_only(item_attrs(item)) {
            return;
        }
        match item {
            syn::Item::Use(u) => self.add_use(&u.tree, &mut Vec::new()),
            syn::Item::Type(t) => {
                if let syn::Type::Path(p) = &*t.ty {
                    let segs = p.path.segments.iter().map(|s| name(&s.ident)).collect();
                    self.aliases.insert(name(&t.ident), segs);
                }
            }
            _ => {}
        }
        visit::visit_item(self, item);
    }
}

/// The rule a path (expanded) breaks, if any.
fn path_rule(segs: &[String]) -> Option<&'static str> {
    let last = segs.last()?.as_str();
    let prev = segs.len().checked_sub(2).map(|i| segs[i].as_str());
    Some(match (prev, last) {
        (Some("fs"), "read") => "fs::read",
        (Some("fs"), "read_to_string") => "fs::read_to_string",
        (Some("fs"), "copy") => "fs::copy",
        (_, "read_to_string") => "read_to_string",
        (Some("File"), "open" | "options") => "File::open",
        (Some("fcntl" | "libc"), "open" | "openat") => "open(2)",
        (Some(p), "read_file") if p != "nofollow" => "read_file",
        _ if segs.iter().any(|s| s == "OpenOptions") => "OpenOptions",
        _ => return None,
    })
}

/// The reads found in one file: rule, line, what was written.
struct Reads<'n> {
    names: &'n Names,
    found: Vec<(&'static str, usize, String)>,
}

impl Reads<'_> {
    fn check(&mut self, segs: Vec<String>, line: usize) {
        let full = self.names.expand(segs);
        if let Some(rule) = path_rule(&full) {
            self.found.push((rule, line, full.join("::")));
        }
    }
}

impl<'ast> Visit<'ast> for Reads<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !test_only(item_attrs(item)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        if !test_only(&f.attrs) {
            visit::visit_impl_item_fn(self, f);
        }
    }

    fn visit_local(&mut self, l: &'ast syn::Local) {
        if !test_only(&l.attrs) {
            visit::visit_local(self, l);
        }
    }

    fn visit_expr_path(&mut self, e: &'ast syn::ExprPath) {
        let line = e
            .path
            .segments
            .first()
            .map_or(0, |s| s.ident.span().start().line);
        let mut segs: Vec<String> = Vec::new();
        // `<std::fs::File>::open`: the type's path, then what follows it.
        let rest = match &e.qself {
            Some(q) => {
                if let syn::Type::Path(t) = &*q.ty {
                    segs.extend(t.path.segments.iter().map(|s| name(&s.ident)));
                }
                q.position
            }
            None => 0,
        };
        segs.extend(e.path.segments.iter().skip(rest).map(|s| name(&s.ident)));
        self.check(segs, line);
        visit::visit_expr_path(self, e);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let rule = match name(&m.method).as_str() {
            "read_to_string" | "read_to_end" => Some("read_to_string"),
            // `Store::read_file` (`s.read_file(…)`).
            "read_file" => Some("read_file"),
            _ => None,
        };
        if let Some(rule) = rule {
            let line = m.method.span().start().line;
            self.found.push((rule, line, format!(".{}(…)", m.method)));
        }
        visit::visit_expr_method_call(self, m);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        use syn::parse::Parser;
        let tokens = mac.tokens.clone();
        if let Ok(exprs) = Punctuated::<Expr, Token![,]>::parse_terminated.parse2(tokens.clone()) {
            for e in &exprs {
                self.visit_expr(e);
            }
        } else if let Ok(stmts) = syn::Block::parse_within.parse2(tokens.clone()) {
            for s in &stmts {
                self.visit_stmt(s);
            }
        } else {
            // Tokens that are no expression (`json!({"k": v})`, `m!(=> …)`):
            // read them as tokens, every path and method call in them.
            self.scan_tokens(tokens);
        }
        visit::visit_macro(self, mac);
    }
}

impl Reads<'_> {
    /// Every `a::b::c` path and `.method(` call in `tokens`, at any depth.
    fn scan_tokens(&mut self, tokens: proc_macro2::TokenStream) {
        use proc_macro2::{Delimiter, TokenTree};
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        let colons = |i: usize| {
            matches!((trees.get(i), trees.get(i + 1)),
                (Some(TokenTree::Punct(a)), Some(TokenTree::Punct(b)))
                    if a.as_char() == ':' && b.as_char() == ':')
        };
        let mut i = 0;
        while i < trees.len() {
            match &trees[i] {
                TokenTree::Group(g) => self.scan_tokens(g.stream()),
                TokenTree::Punct(p) if p.as_char() == '.' => {
                    if let (Some(TokenTree::Ident(m)), Some(TokenTree::Group(g))) =
                        (trees.get(i + 1), trees.get(i + 2))
                    {
                        let rule = match name(m).as_str() {
                            "read_to_string" | "read_to_end" => Some("read_to_string"),
                            "read_file" => Some("read_file"),
                            _ => None,
                        };
                        if let (Some(rule), Delimiter::Parenthesis) = (rule, g.delimiter()) {
                            let line = m.span().start().line;
                            self.found.push((rule, line, format!(".{m}(…)")));
                        }
                        // The method name is no path: on to its arguments.
                        i += 2;
                        continue;
                    }
                }
                TokenTree::Ident(first) => {
                    let line = first.span().start().line;
                    let mut segs = vec![name(first)];
                    let mut j = i + 1;
                    while colons(j) {
                        let Some(TokenTree::Ident(next)) = trees.get(j + 2) else {
                            break;
                        };
                        segs.push(name(next));
                        j += 3;
                    }
                    self.check(segs, line);
                    i = j;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
    }
}

/// The reads in `src` (one file): rule, line, what was written.
fn reads(src: &str) -> Vec<(&'static str, usize, String)> {
    let file = syn::parse_file(src).unwrap_or_else(|e| panic!("does not parse: {e}\n{src}"));
    let mut names = Names::default();
    names.visit_file(&file);
    let mut out = Reads {
        names: &names,
        found: Vec::new(),
    };
    out.visit_file(&file);
    let mut found = out.found;
    // A glob of a module that holds a read (`use std::fs::*`): every
    // name in it could be one.
    for (prefix, line) in &names.globs {
        let modules = ["fs", "files", "fcntl", "libc", "File", "OpenOptions"];
        if prefix.last().is_some_and(|m| modules.contains(&m.as_str())) {
            found.push(("glob", *line, format!("{}::*", prefix.join("::"))));
        }
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
        let at = here.join(dir);
        if at.is_file() {
            files.push(at.clone());
        } else {
            rust_files(&at, &mut files);
            assert!(files.len() > 3, "no sources under {dir}");
        }
        // A file named alone is scanned too, or the guard checks nothing there.
        assert!(!files.is_empty(), "nothing scanned at {dir}");
        for file in files {
            let under = file.strip_prefix(&at).unwrap().to_string_lossy();
            let rel = if under.is_empty() {
                (*dir).to_owned()
            } else {
                format!("{dir}/{}", under.replace('\\', "/"))
            };
            if rel == HELPER {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for (rule, line, what) in reads(&text) {
                found
                    .entry((rel.clone(), rule))
                    .or_default()
                    .push(format!("{rel}:{line} {what}"));
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

/// The rules found in `src`.
fn rules(src: &str) -> Vec<&'static str> {
    reads(src).into_iter().map(|(r, _, _)| r).collect()
}

/// Every way of writing a read is caught; a comment, a string or an item
/// built only for tests that names one is not.
#[test]
fn every_spelling_of_a_read_is_caught() {
    for (src, rule) in [
        // Paths, full or not, over lines, as a value.
        (
            "fn f() { let t = std::fs::read_to_string(p); }",
            "fs::read_to_string",
        ),
        ("fn f() { let b = std::fs::read(p); }", "fs::read"),
        ("fn f() { let b = std::fs\n    ::read(p); }", "fs::read"),
        ("fn f() { let b = ::std::fs::read(p); }", "fs::read"),
        (
            "async fn f() { let b = tokio::fs::read(&p).await; }",
            "fs::read",
        ),
        ("fn f() { x.and_then(std::fs::read); }", "fs::read"),
        ("fn f() { let _ = std::fs::copy(a, b); }", "fs::copy"),
        (
            "async fn f() { tokio::fs::copy(&a, &b).await; }",
            "fs::copy",
        ),
        (
            "fn f() { std::io::read_to_string(file); }",
            "read_to_string",
        ),
        ("fn f() { file.read_to_end(&mut v); }", "read_to_string"),
        ("fn f() { file.read_to_string(&mut s); }", "read_to_string"),
        // Inside a macro whose tokens are not expressions (`json!` objects).
        (
            "fn f() { let v = json!({\"k\": std::fs::read_to_string(p)}); }",
            "fs::read_to_string",
        ),
        (
            "fn f() { let v = json!({\"k\": {\"n\": tokio::fs::read(p).await}}); }",
            "fs::read",
        ),
        ("fn f() { m!(=> std::fs::copy(a, b)); }", "fs::copy"),
        (
            "fn f() { m!(=> file.read_to_string(&mut s)); }",
            "read_to_string",
        ),
        (
            "use std::fs::read as slurp;\nfn f() { m!(=> slurp(p)); }",
            "fs::read",
        ),
        // Imports, renamed, in braces, of the module.
        ("use std::fs::read;\nfn f() { read(p); }", "fs::read"),
        (
            "use std::fs::read as slurp;\nfn f() { slurp(p); }",
            "fs::read",
        ),
        (
            "use std::{fs::{read, write}};\nfn f() { read(p); }",
            "fs::read",
        ),
        (
            "use std::fs::{self as f};\nfn g() { f::read(p); }",
            "fs::read",
        ),
        (
            "use std::fs as f;\nfn g() { f::read_to_string(p); }",
            "fs::read_to_string",
        ),
        (
            "use std::{fs as f, io};\nfn g() { f::copy(a, b); }",
            "fs::copy",
        ),
        ("use std::fs;\nfn g() { fs::read(p); }", "fs::read"),
        (
            "use std::fs::copy as dup;\nfn g() { dup(a, b); }",
            "fs::copy",
        ),
        ("use std::fs::*;", "glob"),
        ("use od_store::files::*;", "glob"),
        // Opens of its own.
        ("fn f() { let f = std::fs::File::open(p); }", "File::open"),
        (
            "use std::fs::File;\nfn f() { File::options().read(true).open(p); }",
            "File::open",
        ),
        (
            "async fn f() { tokio::fs::File::open(p).await; }",
            "File::open",
        ),
        (
            "use std::fs::File as F;\nfn f() { F::open(p); }",
            "File::open",
        ),
        ("fn f() { <std::fs::File>::open(p); }", "File::open"),
        (
            "use std::fs::File;\nfn f() { <File>::open(p); }",
            "File::open",
        ),
        (
            "type F = std::fs::File;\nfn f() { F::open(p); }",
            "File::open",
        ),
        (
            "use std::fs::File;\ntype F = File;\nfn f() { F::open(p); }",
            "File::open",
        ),
        (
            "fn f() { let o = std::fs::OpenOptions::new(); }",
            "OpenOptions",
        ),
        (
            "use std::fs::OpenOptions as O;\nfn f() { O::new(); }",
            "OpenOptions",
        ),
        ("fn f() { nix::fcntl::open(p, flags, mode); }", "open(2)"),
        (
            "use nix::fcntl::{openat, OFlag};\nfn f() { openat(&d, n, flags, mode); }",
            "open(2)",
        ),
        (
            "use nix::fcntl;\nfn f() { fcntl::open(p, flags, mode); }",
            "open(2)",
        ),
        ("fn f() { unsafe { libc::open(p, 0) }; }", "open(2)"),
        (
            "use nix::libc::openat as raw;\nfn f() { unsafe { raw(d, p, 0) }; }",
            "open(2)",
        ),
        // `od_store`'s reader, by path, import, method.
        (
            "fn f() { let (_, b) = od_store::files::read_file(&root, p)?; }",
            "read_file",
        ),
        (
            "use od_store::files::read_file as rf;\nfn f() { rf(&root, p); }",
            "read_file",
        ),
        (
            "use od_store::files::{ProjectRoot, read_file};\nfn f() { read_file(&r, p); }",
            "read_file",
        ),
        (
            "use od_store::{files::{self as f}};\nfn g() { f::read_file(&r, p); }",
            "read_file",
        ),
        (
            "fn f() { let (_, b) = s.read_file(id, name)?; }",
            "read_file",
        ),
        ("fn f() { Store::read_file(&s, id, name); }", "read_file"),
        // Raw identifiers and the code after them.
        ("fn f() { let r#type = 1; std::fs::read(p); }", "fs::read"),
        ("fn r#match() {}\nfn f() { std::fs::read(p); }", "fs::read"),
        ("fn f() { std::r#fs::r#read(p); }", "fs::read"),
        // Built for tests and more: still read.
        (
            "#[cfg(any(test, unix))]\nfn f() { std::fs::read(p); }",
            "fs::read",
        ),
        (
            "#[cfg(not(test))]\nfn f() { std::fs::read(p); }",
            "fs::read",
        ),
        (
            "#[cfg(all(unix, any(test, debug_assertions)))]\nfn f() { std::fs::read(p); }",
            "fs::read",
        ),
        (
            "#[cfg(test)]\nfn t() {}\nfn f() { std::fs::read(p); }",
            "fs::read",
        ),
        // In an impl.
        ("impl S {\n    fn f() { std::fs::read(p); }\n}", "fs::read"),
        // In a macro call.
        (
            "fn f() { let s = format!(\"{}\", std::fs::read_to_string(p).unwrap()); }",
            "fs::read_to_string",
        ),
        ("fn f() { assert!(std::fs::read(p).is_ok()); }", "fs::read"),
    ] {
        assert_eq!(rules(src), [rule], "{src}");
    }
    for src in [
        "// std::fs::read_to_string(p)\nfn f() {}",
        "/* std::fs::read(p) */\nfn f() {}",
        "/* outer /* std::fs::read(p) */ still a comment */\nfn f() {}",
        r#"fn f() { let s = "std::fs::read(p)"; }"#,
        r##"fn f() { let s = r#"std::fs::read_to_string(p)"#; }"##,
        r##"fn f() { let s = r#"one " std::fs::read(p)"#; }"##,
        "fn f() { let c = '\"'; let s = \"fs::read(\"; }",
        "fn f() { std::fs::read_dir(p); }",
        "fn f() { nofollow::read_file(p, 10); }",
        "fn f() { od_content::nofollow::read_file(p, 10); }",
        "use od_content::nofollow::{read_file, read_text};\nfn f() { read_file(p, 1); }",
        "fn f() { crate::routes::export::read_page_file(&root, p); }",
        "fn f() { od_store::files::stat_file(&root, p); }",
        "fn f() { copy_dir(&from, &to, 10); }",
        "fn f() { std::fs::write(p, b); std::fs::File::create(p); }",
        "fn f() { let r#type = 1; let r#fs = 2; }",
        "#[cfg(test)]\nmod tests {\n    fn t() { std::fs::read(p); }\n}",
        "#[cfg(all(test, unix))]\nfn t() { std::fs::read_to_string(p); }",
        "#[cfg(any(test))]\nfn t() { std::fs::read(p); }",
        "#[cfg(test)]\nuse std::fs::read as slurp;\nfn f() { slurp(p); }",
        "#[test]\nfn t() { std::fs::read(p); }",
        "#[tokio::test]\nasync fn t() { std::fs::read(p); }",
        "impl S {\n    #[cfg(test)]\n    fn t() { std::fs::read(p); }\n}",
        "fn f() {\n    #[cfg(test)]\n    let x = std::fs::read(p);\n}",
    ] {
        assert!(rules(src).is_empty(), "{src}: {:?}", rules(src));
    }
}
