//! Two kinds of write into the folders a form edits and an agent writes
//! too (`crates/od-server/CLAUDE.md`, invariant 16), in `od-content`,
//! `od-server` and the memory index (`od-store/src/memory_index.rs`):
//!
//! - `write`: a `SKILL.md` or `DESIGN.md` is written only through
//!   `od_content::user_content::write_replacing` (a file of its own, renamed
//!   over it). A `std::fs::write`/`File::create` in place lets a read (the
//!   index, a form's `GET`) find it half written, and writes through a link.
//! - `user_root.join`: a folder under the user root is named by an id only
//!   where the code checks the names on disk (`named_exactly`, `entry_like`,
//!   `refuse_case_twin`): on macOS and Windows `user_root.join("ad-creative")`
//!   also opens `Ad-Creative`, another skill.
//!
//! - `tmp`: a function that renames a file it wrote makes it with
//!   `create_new` (`O_EXCL`; `od_content::nofollow::new_file`,
//!   `replace_file`): `fs::write`, `File::create` or `.create(true)` open a
//!   link planted at a temp name and write wherever it points.
//!
//! - `pid`: no name is made from `std::process::id()`: an agent works a
//!   pid out and plants a folder or link at that name first (install
//!   staging `.installing-<id>-<pid>` was written through, 2026-11-27).
//!   A name in a folder an agent can write takes
//!   `od_content::nofollow::random_tag()`, and a folder filled there is
//!   made new and written through its descriptor (`nofollow::NewDir`).
//!
//! What stays is in `KEPT`, by file, function and count, with why.
//!
//! The sources are parsed as Rust (`syn`): comments and strings are no
//! code, items built only for tests are left out. A path to the file is
//! followed through string literals (`"SKILL.md"`, `format!("{d}/SKILL.md")`),
//! constants holding one, `let` bindings and `for` loops over them; the
//! user root through `let` bindings, `user_root(&state)`, `.clone()`,
//! `.to_path_buf()`, `&`, and `Path::join(user_root, …)`; calls through
//! `use … as` and `type`. A path built in a closure's argument (a writer
//! passed in) is out of reach: the caller is what is checked.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use proc_macro2::{TokenStream, TokenTree};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::ext::IdentExt;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, Meta, Token, UseTree};

/// The folders and files scanned, relative to this crate. The memory
/// index (`<data>/memory`, a folder an agent can write) is written outside
/// `od-content` and `od-server`.
const SCANNED: &[&str] = &[
    "../od-content/src",
    "src",
    "../od-store/src/memory_index.rs",
];

/// The files a form edits.
const USER_FILES: &[&str] = &["SKILL.md", "DESIGN.md"];

/// Writes that stay: file (relative to this crate), function, rule, how
/// many, why.
const KEPT: &[(&str, &str, &str, usize, &str)] = &[
    (
        "src/routes/library/design_systems.rs",
        "import",
        "write",
        1,
        "into a temporary folder of its own (`tempfile::tempdir`), installed as a whole",
    ),
    (
        "src/routes/file_ops.rs",
        "save_template",
        "write",
        1,
        "into the template folder claimed with `create_dir` just before; nothing reads it yet",
    ),
    (
        "../od-content/src/user_content.rs",
        "create_skill",
        "user_root.join",
        1,
        "the folder is claimed with `create_dir`: an entry of that name in any case is `AlreadyExists` on macOS and Windows",
    ),
    (
        "../od-content/src/user_content.rs",
        "skill_write_dir",
        "user_root.join",
        1,
        "taken only when `named_exactly` finds that very name on disk",
    ),
    (
        "../od-content/src/user_content.rs",
        "save_skill",
        "user_root.join",
        1,
        "after the case twin check (`named_exactly`, `entry_like`) under `REPLACE_LOCK`",
    ),
    (
        "../od-content/src/user_content.rs",
        "install_plugin",
        "user_root.join",
        1,
        "after `refuse_case_twin`",
    ),
    (
        "../od-content/src/user_content.rs",
        "recover_incoming",
        "user_root.join",
        1,
        "only when `entry_like` finds no entry of that name in any case",
    ),
    (
        "../od-content/src/user_content.rs",
        "recover_old",
        "user_root.join",
        1,
        "only when `entry_like` finds no entry of that name in any case",
    ),
    (
        "../od-content/src/user_content.rs",
        "recover_swap",
        "user_root.join",
        1,
        "only when `entry_like` finds no entry of that name in any case",
    ),
    (
        "../od-content/src/user_content.rs",
        "user_skill_dir",
        "user_root.join",
        1,
        "the name as it is on disk, found by `entry_like`",
    ),
    (
        "src/routes/library/design_systems.rs",
        "copy_revisable",
        "write",
        1,
        "into the revision's temporary working folder (`tempfile`), never the package",
    ),
    (
        "../od-content/src/user_content.rs",
        "save_skill_again",
        "user_root.join",
        1,
        "after `entry_like` finds no entry of that name in another case, under `REPLACE_LOCK`",
    ),
    (
        "src/routes/w3/reference.rs",
        "hold",
        "pid",
        1,
        "the pid is written in the mark's body (who holds it), never a name",
    ),
    (
        "src/routes/library/design_systems.rs",
        "copy",
        "user_root.join",
        3,
        "a free folder name: `exists()` in any case skips a taken one, never opens it",
    ),
];

/// Whether a `cfg` predicate holds only when `test` does.
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
        path.segments.last().is_some_and(|s| s.ident == "test")
    })
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    use syn::Item;
    match item {
        Item::Const(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

fn name(ident: &syn::Ident) -> String {
    ident.unraw().to_string()
}

/// The names a file gives paths (`use … as`, `type`), and the constants
/// and statics that hold a user file's name.
#[derive(Default)]
struct Names {
    aliases: HashMap<String, Vec<String>>,
    file_consts: HashSet<String>,
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
            UseTree::Glob(_) => {}
            UseTree::Group(g) => {
                for t in &g.items {
                    self.add_use(t, prefix);
                }
            }
        }
    }

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
            syn::Item::Const(c) if mentions_a_file(&c.expr, &HashSet::new()) => {
                self.file_consts.insert(name(&c.ident));
            }
            syn::Item::Static(s) if mentions_a_file(&s.expr, &HashSet::new()) => {
                self.file_consts.insert(name(&s.ident));
            }
            _ => {}
        }
        visit::visit_item(self, item);
    }
}

/// Whether `tokens` name a user file: a string literal holding one, or a
/// name in `known` (a constant or a binding that holds one).
fn names_a_file(tokens: &TokenStream, known: &HashSet<String>) -> bool {
    tokens.clone().into_iter().any(|t| match t {
        TokenTree::Group(g) => names_a_file(&g.stream(), known),
        TokenTree::Literal(l) => syn::parse_str::<syn::LitStr>(&l.to_string())
            .is_ok_and(|s| USER_FILES.iter().any(|f| s.value().contains(f))),
        TokenTree::Ident(i) => known.contains(&name(&i)),
        TokenTree::Punct(_) => false,
    })
}

/// [`names_a_file`] for an expression: its string literals, the names in
/// it, and the tokens of the macros in it.
fn mentions_a_file(e: &Expr, known: &HashSet<String>) -> bool {
    struct Mentions<'a> {
        known: &'a HashSet<String>,
        hit: bool,
    }
    impl<'ast> Visit<'ast> for Mentions<'_> {
        fn visit_lit_str(&mut self, l: &'ast syn::LitStr) {
            self.hit |= USER_FILES.iter().any(|f| l.value().contains(f));
        }
        fn visit_ident(&mut self, i: &'ast syn::Ident) {
            self.hit |= self.known.contains(&name(i));
        }
        // The macros that build a path or a list of names; not data
        // (`json!({"files": {"design": "DESIGN.md"}})` names no file
        // written).
        fn visit_macro(&mut self, m: &'ast syn::Macro) {
            let builds =
                m.path.segments.last().is_some_and(|s| {
                    matches!(name(&s.ident).as_str(), "format" | "concat" | "vec")
                });
            if builds {
                self.hit |= names_a_file(&m.tokens, self.known);
            }
            visit::visit_macro(self, m);
        }
    }
    let mut m = Mentions { known, hit: false };
    m.visit_expr(e);
    m.hit
}

/// The bindings of a function body: those that hold a user file's path,
/// and those that hold the user root.
#[derive(Default)]
struct Bindings {
    files: HashSet<String>,
    roots: HashSet<String>,
}

/// The names a pattern binds.
fn pat_names(pat: &syn::Pat, out: &mut Vec<String>) {
    match pat {
        syn::Pat::Ident(p) => out.push(name(&p.ident)),
        syn::Pat::Reference(r) => pat_names(&r.pat, out),
        syn::Pat::Type(t) => pat_names(&t.pat, out),
        syn::Pat::Tuple(t) => t.elems.iter().for_each(|p| pat_names(p, out)),
        syn::Pat::TupleStruct(t) => t.elems.iter().for_each(|p| pat_names(p, out)),
        _ => {}
    }
}

/// `let`s and `for`s of a body, in source order.
struct Binds<'a> {
    consts: &'a HashSet<String>,
    out: Bindings,
}

impl<'ast> Visit<'ast> for Binds<'_> {
    fn visit_local(&mut self, l: &'ast syn::Local) {
        if let Some(init) = &l.init {
            let mut names = Vec::new();
            pat_names(&l.pat, &mut names);
            let known: HashSet<String> = self.out.files.union(self.consts).cloned().collect();
            if mentions_a_file(&init.expr, &known) {
                self.out.files.extend(names.iter().cloned());
            }
            if is_root(&init.expr, &self.out.roots) {
                self.out.roots.extend(names);
            }
        }
        visit::visit_local(self, l);
    }

    fn visit_expr_for_loop(&mut self, f: &'ast syn::ExprForLoop) {
        let known: HashSet<String> = self.out.files.union(self.consts).cloned().collect();
        if mentions_a_file(&f.expr, &known) {
            let mut names = Vec::new();
            pat_names(&f.pat, &mut names);
            self.out.files.extend(names);
        }
        visit::visit_expr_for_loop(self, f);
    }
}

/// Whether `e` is the user root: `user_root`, a binding of it,
/// `user_root(&state)`, through `&`, `( )`, `.clone()`, `.to_path_buf()`…
fn is_root(e: &Expr, roots: &HashSet<String>) -> bool {
    match e {
        Expr::Reference(r) => is_root(&r.expr, roots),
        Expr::Paren(p) => is_root(&p.expr, roots),
        Expr::MethodCall(m)
            if matches!(
                name(&m.method).as_str(),
                "clone" | "to_path_buf" | "as_path" | "as_ref" | "to_owned" | "into"
            ) =>
        {
            is_root(&m.receiver, roots)
        }
        // `state.store.data_dir().user_skills_dir()`, `paths.user_root()`.
        Expr::MethodCall(m) => matches!(name(&m.method).as_str(), "user_skills_dir" | "user_root"),
        Expr::Path(p) => p.path.get_ident().is_some_and(|i| {
            let n = name(i);
            n == "user_root" || roots.contains(&n)
        }),
        Expr::Call(c) => matches!(&*c.func, Expr::Path(p)
            if p.path.segments.last().is_some_and(|s| s.ident == "user_root")),
        _ => false,
    }
}

/// Whether `arg` of `user_root.join(arg)` names no id: a literal, a
/// constant (`PREVIOUS_DIR`), or a hidden name (`format!(".installing-…")`).
fn names_no_id(arg: &Expr) -> bool {
    match arg {
        Expr::Reference(r) => names_no_id(&r.expr),
        Expr::Paren(p) => names_no_id(&p.expr),
        Expr::Lit(_) => true,
        Expr::Path(p) => p.path.get_ident().is_some_and(|i| {
            let n = name(i);
            n.chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
        }),
        Expr::Macro(m) => {
            m.mac.path.is_ident("format")
                && m.mac.tokens.clone().into_iter().next().is_some_and(|t| {
                    syn::parse_str::<syn::LitStr>(&t.to_string())
                        .is_ok_and(|s| s.value().starts_with('.'))
                })
        }
        _ => false,
    }
}

/// The writes found in one file: rule, function, line.
struct Writes<'n> {
    names: &'n Names,
    func: String,
    binds: Bindings,
    /// In the function being read: a `fs::rename`, a write that is not
    /// `create_new` (`fs::write`, `File::create`, `.create(true)`,
    /// `.truncate(true)`), a `create_new`; the line of the first rename.
    renames: Option<usize>,
    plain_call: bool,
    plain_write: bool,
    create_new: bool,
    found: Vec<(&'static str, String, usize)>,
}

/// The method names of a chain `a.b(…).c(…)`, outermost first, and the
/// expression it starts from.
fn chain(mut e: &Expr) -> (Vec<String>, &Expr) {
    let mut names = Vec::new();
    while let Expr::MethodCall(m) = e {
        names.push(name(&m.method));
        e = &m.receiver;
    }
    (names, e)
}

/// Whether `e` is the literal `true`.
fn is_true(e: Option<&Expr>) -> bool {
    matches!(e, Some(Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(b), .. })) if b.value)
}

impl Writes<'_> {
    fn enter(&mut self, func: &syn::Ident, block: &syn::Block) {
        let mut b = Binds {
            consts: &self.names.file_consts,
            out: Bindings::default(),
        };
        // Twice: a binding made from one bound later in a loop.
        b.visit_block(block);
        b.visit_block(block);
        self.binds = b.out;
        self.func = name(func);
        self.renames = None;
        self.plain_call = false;
        self.plain_write = false;
        self.create_new = false;
    }

    /// Leaving a function: one that renames a file it wrote without
    /// `create_new` wrote a temp file a link planted at its name sends
    /// anywhere (`O_EXCL` is what refuses one).
    fn leave(&mut self) {
        // `fs::write`/`File::create` never make a file new; a
        // `.create(true)` may sit beside a `create_new` in another branch.
        if let Some(line) = self.renames
            && (self.plain_call || (self.plain_write && !self.create_new))
        {
            self.push("tmp", line);
        }
        self.renames = None;
    }

    fn known_files(&self) -> HashSet<String> {
        self.binds
            .files
            .union(&self.names.file_consts)
            .cloned()
            .collect()
    }

    fn line(e: &Expr) -> usize {
        e.span().start().line
    }

    fn push(&mut self, rule: &'static str, line: usize) {
        self.found.push((rule, self.func.clone(), line));
    }
}

impl<'ast> Visit<'ast> for Writes<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !test_only(item_attrs(item)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.enter(&f.sig.ident, &f.block);
        visit::visit_item_fn(self, f);
        self.leave();
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        if !test_only(&f.attrs) {
            self.enter(&f.sig.ident, &f.block);
            visit::visit_impl_item_fn(self, f);
            self.leave();
        }
    }

    fn visit_local(&mut self, l: &'ast syn::Local) {
        if !test_only(&l.attrs) {
            visit::visit_local(self, l);
        }
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let Expr::Path(p) = &*c.func {
            let segs: Vec<String> = p.path.segments.iter().map(|s| name(&s.ident)).collect();
            let full = self.names.expand(segs);
            let last = full.last().map(String::as_str);
            let prev = full.len().checked_sub(2).map(|i| full[i].as_str());
            let writes = matches!(
                (prev, last),
                (Some("fs"), Some("write")) | (Some("File"), Some("create"))
            );
            self.plain_call |= writes;
            if full.len() >= 2 && full[full.len() - 2] == "process" && last == Some("id") {
                self.push("pid", Self::line(&c.func));
            }
            if matches!((prev, last), (Some("fs"), Some("rename" | "hard_link")))
                && self.renames.is_none()
            {
                self.renames = Some(Self::line(&c.func));
            }
            // `fs::copy(from, to)` writes `to`.
            let target = match (prev, last) {
                (Some("fs"), Some("copy")) => c.args.iter().nth(1),
                _ if writes => c.args.first(),
                _ => None,
            };
            if target.is_some_and(|a| mentions_a_file(a, &self.known_files())) {
                self.push("write", Self::line(&c.func));
            }
            // `Path::join(user_root, id)`.
            if last == Some("join")
                && c.args
                    .first()
                    .is_some_and(|a| is_root(a, &self.binds.roots))
                && !c.args.iter().nth(1).is_some_and(names_no_id)
            {
                self.push("user_root.join", Self::line(&c.func));
            }
        }
        visit::visit_expr_call(self, c);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let method = name(&m.method);
        match method.as_str() {
            "create" | "truncate" if is_true(m.args.first()) => self.plain_write = true,
            "create_new" if is_true(m.args.first()) => self.create_new = true,
            _ => {}
        }
        // `OpenOptions::new().write(true)….open(path)`, `File::options()…`,
        // or options bound earlier (`o.open(path)`: its flags unseen, taken
        // for a write).
        if method == "open" {
            let (calls, start) = chain(&m.receiver);
            let writes = calls.iter().any(|c| {
                matches!(
                    c.as_str(),
                    "write" | "append" | "create" | "truncate" | "create_new"
                )
            }) || (calls.is_empty() && matches!(start, Expr::Path(_)));
            if writes
                && m.args
                    .first()
                    .is_some_and(|a| mentions_a_file(a, &self.known_files()))
            {
                self.push("write", m.method.span().start().line);
            }
        }
        if m.method == "join"
            && is_root(&m.receiver, &self.binds.roots)
            && !m.args.first().is_some_and(names_no_id)
        {
            self.push("user_root.join", m.method.span().start().line);
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
        } else if let Ok(stmts) = syn::Block::parse_within.parse2(tokens) {
            for s in &stmts {
                self.visit_stmt(s);
            }
        }
        visit::visit_macro(self, mac);
    }
}

/// The writes in `src` (one file): rule, function, line.
fn writes(src: &str) -> Vec<(&'static str, String, usize)> {
    let file = syn::parse_file(src).unwrap_or_else(|e| panic!("does not parse: {e}\n{src}"));
    let mut names = Names::default();
    names.visit_file(&file);
    let mut out = Writes {
        names: &names,
        func: String::new(),
        binds: Bindings::default(),
        renames: None,
        plain_call: false,
        plain_write: false,
        create_new: false,
        found: Vec::new(),
    };
    out.visit_file(&file);
    out.found
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
fn user_files_are_written_whole_and_named_as_on_disk() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found: BTreeMap<(String, String, &str), Vec<String>> = BTreeMap::new();
    for dir in SCANNED {
        let at = here.join(dir);
        let mut files = Vec::new();
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
            let text = std::fs::read_to_string(&file).unwrap();
            for (rule, func, line) in writes(&text) {
                found
                    .entry((rel.clone(), func, rule))
                    .or_default()
                    .push(format!("{rel}:{line}"));
            }
        }
    }
    let mut wrong = Vec::new();
    for ((file, func, rule), lines) in &found {
        let kept = KEPT
            .iter()
            .find(|(f, n, r, _, _)| f == file && n == func && r == rule)
            .map_or(0, |k| k.3);
        if lines.len() != kept {
            wrong.push(format!(
                "{file} `{func}`: {} `{rule}` (kept {kept}): {lines:?}",
                lines.len()
            ));
        }
    }
    for (file, func, rule, n, _) in KEPT {
        if !found.contains_key(&((*file).to_owned(), (*func).to_owned(), *rule)) {
            wrong.push(format!("{file} `{func}`: `{rule}` kept {n}, none left"));
        }
    }
    assert!(
        wrong.is_empty(),
        "write a SKILL.md or DESIGN.md through `user_content::write_replacing`, name a folder under the \
         user root only after checking the names on disk (`named_exactly`, `entry_like`), or list it in \
         KEPT with why:\n{}",
        wrong.join("\n")
    );
}

/// The rules found in `src`.
fn rules(src: &str) -> Vec<&'static str> {
    writes(src).into_iter().map(|(r, _, _)| r).collect()
}

/// Every way of writing one is caught; a comment, a string, another file,
/// the helper or an item built only for tests is not.
#[test]
fn every_spelling_of_a_write_is_caught() {
    for (src, rule) in [
        // A user file written in place, by path, import, alias, type.
        (
            r#"fn f() { std::fs::write(dir.join("SKILL.md"), b); }"#,
            "write",
        ),
        (
            r#"fn f() { ::std::fs::write(dir.join("DESIGN.md"), b); }"#,
            "write",
        ),
        (
            "fn f() { std::fs\n    ::write(\n        dir.join(\"SKILL.md\"),\n        b,\n    ); }",
            "write",
        ),
        (
            r#"use std::fs; fn f() { fs::write(d.join("SKILL.md"), b); }"#,
            "write",
        ),
        (
            r#"use std::fs::write as put; fn f() { put(d.join("DESIGN.md"), b); }"#,
            "write",
        ),
        (
            r#"use std::fs as f; fn g() { f::write(d.join("SKILL.md"), b); }"#,
            "write",
        ),
        (
            r#"async fn f() { tokio::fs::write(d.join("SKILL.md"), b).await; }"#,
            "write",
        ),
        (
            r#"fn f() { std::fs::File::create(d.join("SKILL.md")); }"#,
            "write",
        ),
        (
            r#"use std::fs::File; type F = File; fn f() { F::create(d.join("DESIGN.md")); }"#,
            "write",
        ),
        // The path built elsewhere: a binding, a clone of it, a constant,
        // a loop over a list of names, `format!`, a helper's argument.
        (
            r#"fn f() { let p = d.join("SKILL.md"); std::fs::write(&p, b); }"#,
            "write",
        ),
        (
            r#"fn f() { let p = d.join("SKILL.md"); let q = p.clone(); std::fs::write(q, b); }"#,
            "write",
        ),
        (
            r#"const NAME: &str = "DESIGN.md"; fn f() { std::fs::write(d.join(NAME), b); }"#,
            "write",
        ),
        (
            r#"const FILES: [&str; 2] = ["DESIGN.md", "tokens.css"]; fn f() { for n in FILES { std::fs::write(d.join(n), b); } }"#,
            "write",
        ),
        (
            r#"fn f() { std::fs::write(format!("{d}/SKILL.md"), b); }"#,
            "write",
        ),
        (
            r#"fn f() { std::fs::write(inside(&d, "DESIGN.md")?, b); }"#,
            "write",
        ),
        // In a macro, an impl, a closure.
        (
            r#"fn f() { assert!(std::fs::write(d.join("SKILL.md"), b).is_ok()); }"#,
            "write",
        ),
        (
            "impl S {\n    fn f() { std::fs::write(d.join(\"SKILL.md\"), b); }\n}",
            "write",
        ),
        (
            r#"fn f() { let w = || std::fs::write(d.join("SKILL.md"), b); }"#,
            "write",
        ),
        // Opened for writing, copied onto (PR #351 review: both passed).
        (
            r#"fn f() { std::fs::OpenOptions::new().write(true).truncate(true).open(inside(&e.dir, "DESIGN.md")?); }"#,
            "write",
        ),
        (
            r#"use std::fs::OpenOptions; fn f() { OpenOptions::new().append(true).open(d.join("SKILL.md")); }"#,
            "write",
        ),
        (
            r#"fn f() { std::fs::File::options().write(true).open(d.join("SKILL.md")); }"#,
            "write",
        ),
        (
            r#"fn f() { let mut o = std::fs::OpenOptions::new(); o.write(true); o.open(d.join("SKILL.md")); }"#,
            "write",
        ),
        (
            r#"fn f() { std::fs::copy(src, d.join("SKILL.md")); }"#,
            "write",
        ),
        (
            r#"use std::fs::copy as dup; fn f() { dup(src, d.join("DESIGN.md")); }"#,
            "write",
        ),
        // A temp file written plainly, then renamed (PR #351 review).
        (
            "fn f() { std::fs::write(&tmp, b); std::fs::rename(&tmp, p); }",
            "tmp",
        ),
        (
            "fn f() { let f = std::fs::File::create(&tmp); std::fs::rename(&tmp, p); }",
            "tmp",
        ),
        (
            "use std::fs; fn f() { let o = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&tmp); fs::rename(&tmp, p); }",
            "tmp",
        ),
        (
            "fn f() { let mut o = std::fs::OpenOptions::new(); o.write(true).create(true); let x = o.open(&tmp); std::fs::rename(&tmp, p); }",
            "tmp",
        ),
        (
            "impl S {\n    fn f() { std::fs::write(&tmp, b).and_then(|()| std::fs::rename(&tmp, p)); }\n}",
            "tmp",
        ),
        (
            "fn f() { std::fs::write(&tmp, b); let _ = std::fs::hard_link(&tmp, p); }",
            "tmp",
        ),
        (
            "fn f() { std::fs::write(&tmp, b); std::fs::hard_link(&tmp, p); OpenOptions::new().write(true).create_new(true).open(p); }",
            "tmp",
        ),
        // A name made from the pid, however written (2026-11-27: install
        // staging `.installing-<id>-<pid>` was written through).
        (
            "fn f(user_root: &Path) { user_root.join(format!(\".installing-{id}-{}\", std::process::id())); }",
            "pid",
        ),
        (
            "fn f() { let p = ::std::process::id(); let n = format!(\".x-{p}\"); }",
            "pid",
        ),
        (
            "fn f() { held.join(format!(\n    \".incoming-{id}-{}\",\n    std::process\n        ::id()\n)); }",
            "pid",
        ),
        (
            "use std::process; fn f() { d.join(process::id().to_string()); }",
            "pid",
        ),
        (
            "use std::process as p; fn f() { d.join(p::id().to_string()); }",
            "pid",
        ),
        (
            "use std::process::id as pid; fn f() { d.join(pid().to_string()); }",
            "pid",
        ),
        ("use std::process::id; fn f() { let n = || id(); }", "pid"),
        (
            "impl S {\n    fn f() { assert!(d.join(std::process::id().to_string()).exists()); }\n}",
            "pid",
        ),
        // The user root through a chain of calls (PR #351 review).
        (
            "fn f(s: &S) { let _p = s.store.data_dir().user_skills_dir().join(&id); }",
            "user_root.join",
        ),
        (
            "fn f(s: &S) { let r = s.store.data_dir().user_skills_dir(); r.join(id); }",
            "user_root.join",
        ),
        // A folder named by an id under the user root, however written.
        (
            "fn f(user_root: &Path) { user_root.join(id); }",
            "user_root.join",
        ),
        (
            "fn f(user_root: &Path) { user_root\n    .join(&id); }",
            "user_root.join",
        ),
        (
            "fn f(user_root: &Path) { let r = user_root; r.join(id); }",
            "user_root.join",
        ),
        (
            "fn f(s: &S) { let root = user_root(&s); root.join(&id); }",
            "user_root.join",
        ),
        (
            "fn f(user_root: &Path) { user_root.clone().join(id); }",
            "user_root.join",
        ),
        (
            "fn f(user_root: &Path) { user_root.to_path_buf().join(id); }",
            "user_root.join",
        ),
        (
            "fn f(user_root: &Path) { (&user_root).join(id); }",
            "user_root.join",
        ),
        (
            "fn f(user_root: &Path) { Path::join(user_root, id); }",
            "user_root.join",
        ),
        (
            "fn f(user_root: &Path) { user_root.join(format!(\"{id}\")); }",
            "user_root.join",
        ),
        (
            "fn f(user_root: &Path) { let s = format!(\"{}\", user_root.join(id).display()); }",
            "user_root.join",
        ),
    ] {
        assert_eq!(rules(src), [rule], "{src}");
    }
    for src in [
        r#"// std::fs::write(d.join("SKILL.md"), b)
fn f() {}"#,
        r#"fn f() { let s = "std::fs::write(d.join(SKILL.md), b)"; }"#,
        r#"fn f() { std::fs::write(d.join("tokens.css"), b); }"#,
        r#"fn f() { write_replacing(&d.join("SKILL.md"), b); }"#,
        r#"fn f() { user_content::write_replacing(&inside(&d, "DESIGN.md")?, b); }"#,
        r#"fn f() { std::fs::read(d.join("SKILL.md")); }"#,
        r#"#[cfg(test)] mod tests { fn t() { std::fs::write(d.join("SKILL.md"), b); } }"#,
        r#"#[test] fn t() { std::fs::write(d.join("SKILL.md"), b); }"#,
        "fn f(user_root: &Path) { user_root.join(PREVIOUS_DIR); }",
        "fn f(user_root: &Path) { user_root.join(\"skills\"); }",
        "fn f(user_root: &Path) { user_root.join(format!(\".installing-{id}\")); }",
        "fn f(user_root: &Path) { user_root.join(PREVIOUS_DIR).join(id); }",
        "fn f(other: &Path) { other.join(id); }",
        r#"fn f() { std::fs::OpenOptions::new().read(true).open(d.join("SKILL.md")); }"#,
        r#"fn f() { std::fs::copy(d.join("SKILL.md"), out); }"#,
        "fn f() { std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp); std::fs::rename(&tmp, p); }",
        "fn f() { let f = nofollow::new_file(&tmp); std::fs::rename(&tmp, p); }",
        "fn f() { OpenOptions::new().write(true).create_new(true).truncate(true).open(&tmp); std::fs::rename(&tmp, p); }",
        "fn f() { std::fs::rename(&a, &b); }",
        "fn f() { std::fs::write(&p, b); }",
        "fn f(s: &S) { s.store.data_dir().user_skills_dir().join(\"x\"); }",
        "// user_root.join(id)\nfn f() {}",
        "// std::process::id()\nfn f() {}",
        "fn f() { let s = \"std::process::id()\"; }",
        "#[cfg(test)] mod tests { fn t() { std::process::id(); } }",
        "fn f() { d.join(format!(\".x-{}\", nofollow::random_tag())); }",
        "fn f() { s.id(); child.id(); }",
    ] {
        assert!(rules(src).is_empty(), "{src}: {:?}", rules(src));
    }
}
