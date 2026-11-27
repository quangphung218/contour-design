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
//! - `swap`: no function both removes a folder (`fs::remove_dir_all`)
//!   and renames one (`fs::rename`): removing the copy there, then
//!   renaming the new one in, loses both when the app stops between the
//!   two (plugin install, 2026-11-27). A tree goes in place through
//!   `od_fs::user_store::Guard::put_tree` (journaled, the old copy removed
//!   last).
//!
//! - `fs`: in a module that reads and writes a user root ([`ZONE`]), the
//!   disk changes only through `od_fs::user_store` (`UserRoot`, `Guard`):
//!   no `std::fs`/`tokio::fs` write, remove, rename, copy, link or bits,
//!   no `File::create`, no open option that writes, no `nix` `…at` call, no
//!   `libc`, no `use …::fs::*`, whatever the arguments; and no call of a
//!   function in another module ([`HELPER_SOURCES`]) that does any of
//!   these, itself or through one it calls. A path call is matched by its
//!   last two names (`nofollow::replace_file`, `NewDir::create`), a
//!   `crate::f`/`super::f` by a name only writing functions bear; a method
//!   call (`x.f()`) is followed only to a type of the same file, as the
//!   type of `x` is unknown: a value of a writing type is refused where it
//!   is made (`NewDir::create`). The visibility of `od-fs` does the rest:
//!   its low-level writers are private but those with a caller outside it.
//!
//! What stays is in `KEPT`, by file, function and count, with why; `PR 6`
//! marks what the next PR moves to `user_store` (design systems, Copy to
//! Claude Code).
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

/// The modules that read and write a user root (`<data>/skills`,
/// `<data>/design-systems`, `<data>/plugins`, `<data>/memory`): rule `fs`.
/// A path ending in `/` takes every file under it.
const ZONE: &[&str] = &[
    "../od-content/src/user_content.rs",
    "../od-content/src/skills.rs",
    "../od-content/src/plugins.rs",
    "../od-content/src/index.rs",
    "../od-content/src/design_system_gen.rs",
    "../od-store/src/memory_index.rs",
    "src/routes/library/",
    "src/routes/manage/memory.rs",
    "src/routes/c2/memory.rs",
    "src/routes/content.rs",
    "src/skill_export.rs",
    "src/routes/platform/evolution.rs",
    "src/routes/file_ops.rs",
];

/// Where a helper a zone module calls may live (rule `fs`: a helper that
/// writes, in any module of these, is a write where it is called).
/// `od-fs/src/user_store.rs` is the one door, never a helper.
const HELPER_SOURCES: &[&str] = &[
    "../od-content/src",
    "src",
    "../od-store/src",
    "../od-fs/src/nofollow.rs",
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
        "../od-content/src/user_content.rs",
        "copy_dir_into_place",
        "swap",
        1,
        "removes only its own `.copying-<name>-<tag>` when the rename fails; renames into a name that must be free, never over a copy",
    ),
    (
        "../od-content/src/design_system_gen.rs",
        "build_and_place",
        "swap",
        1,
        "removes only the folder it claimed with `create_dir` in this call when a move into it fails; no copy there before",
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
    (
        "../od-content/src/user_content.rs",
        "recover_incoming",
        "fs",
        1,
        "puts back what a replace of a version before the journal left in `.previous` (once at start-up, under the lock); goes when that recovery does",
    ),
    (
        "../od-content/src/user_content.rs",
        "recover_old",
        "fs",
        3,
        "as `recover_incoming`: an older version's `.old-<id>-*` moved back, never removed while it is the only copy",
    ),
    (
        "../od-content/src/user_content.rs",
        "recover_swap",
        "fs",
        1,
        "as `recover_incoming`: an older version's `.swap-<id>-*` moved back to a free name",
    ),
    (
        "../od-content/src/user_content.rs",
        "copy_dir",
        "fs",
        4,
        "PR 6: `Place::New` (a new folder, never over one there); the override's first copy and Copy design system",
    ),
    (
        "../od-content/src/user_content.rs",
        "copy_dir_into_place",
        "fs",
        2,
        "PR 6: as `copy_dir`, its `.copying-<name>-<tag>` renamed to a free name",
    ),
    (
        "../od-content/src/user_content.rs",
        "export_skill_with",
        "fs",
        4,
        "PR 6: Copy to Claude Code writes `HOME/.claude/skills`, a root outside `<data>`",
    ),
    (
        "../od-content/src/user_content.rs",
        "write_into",
        "fs",
        1,
        "PR 6: only `export_skill_with` writes through it",
    ),
    (
        "../od-content/src/user_content.rs",
        "write_replacing",
        "fs",
        1,
        "PR 6: its one caller is `design_systems.rs:put_back` (a revision accept undone)",
    ),
    (
        "../od-content/src/design_system_gen.rs",
        "install_checked",
        "fs",
        3,
        "PR 6: a generated design system is placed as Copy design system is (a free folder name)",
    ),
    (
        "../od-content/src/design_system_gen.rs",
        "build_and_place",
        "fs",
        4,
        "PR 6: as `install_checked`",
    ),
    (
        "src/routes/library/design_systems.rs",
        "copy",
        "fs",
        3,
        "PR 6: Copy design system",
    ),
    (
        "src/routes/library/design_systems.rs",
        "copy_revisable",
        "fs",
        1,
        "PR 6: a revision's temporary working folder (`tempfile`)",
    ),
    (
        "src/routes/library/design_systems.rs",
        "import",
        "fs",
        2,
        "PR 6: an import's temporary folder (`tempfile`)",
    ),
    (
        "src/routes/library/design_systems.rs",
        "patch_metadata",
        "fs",
        1,
        "PR 6: `metadata.json` written in place",
    ),
    (
        "src/routes/library/design_systems.rs",
        "write_revision",
        "fs",
        3,
        "PR 6: an accepted revision",
    ),
    (
        "src/routes/library/design_systems.rs",
        "upload_assets",
        "fs",
        3,
        "PR 6: asset upload",
    ),
    (
        "src/routes/library/design_systems.rs",
        "write_new_asset",
        "fs",
        3,
        "PR 6: asset upload",
    ),
    (
        "src/routes/library/design_systems.rs",
        "write_sent_names",
        "fs",
        5,
        "PR 6: asset upload",
    ),
    (
        "src/routes/library/design_systems.rs",
        "delete_asset",
        "fs",
        1,
        "PR 6: asset delete",
    ),
    (
        "src/routes/library/design_systems.rs",
        "put_back",
        "fs",
        1,
        "PR 6: the files of a revision accept that failed, put back",
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
    /// The lines of `use …::fs::*` (every write of it unnamed).
    globs: Vec<usize>,
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
            UseTree::Glob(g) => {
                if prefix.last().is_some_and(|p| p == "fs") {
                    self.globs.push(g.star_token.span.start().line);
                }
            }
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

/// Whether the call `full` (a path, aliases expanded) changes the file
/// system by itself: `std::fs`/`tokio::fs` writes, removes, renames, copies,
/// links, bits; `File::create`; `nix` `…at` calls; anything in `libc`.
fn raw_write(full: &[String]) -> bool {
    let last = full.last().map(String::as_str);
    let prev = full.len().checked_sub(2).map(|i| full[i].as_str());
    matches!(
        (prev, last),
        (
            Some("fs"),
            Some(
                "write"
                    | "create_dir"
                    | "create_dir_all"
                    | "remove_file"
                    | "remove_dir"
                    | "remove_dir_all"
                    | "rename"
                    | "copy"
                    | "hard_link"
                    | "soft_link"
                    | "symlink"
                    | "symlink_file"
                    | "symlink_dir"
                    | "set_permissions"
            )
        ) | (Some("File"), Some("create" | "create_new"))
    ) || matches!(
        last,
        Some(
            "openat"
                | "renameat"
                | "renameat2"
                | "unlinkat"
                | "mkdirat"
                | "symlinkat"
                | "linkat"
                | "fchmodat"
        )
    ) || full.iter().any(|s| s == "libc")
}

/// Whether a method call sets an open option that makes or changes a
/// file (`OpenOptions`, `File::options()`, options bound earlier):
/// `.write(true)`, `.append(true)`, `.create(true)`, `.truncate(true)`,
/// `.create_new(true)`.
fn sets_write(m: &syn::ExprMethodCall) -> bool {
    matches!(
        name(&m.method).as_str(),
        "write" | "append" | "create" | "truncate" | "create_new"
    ) && is_true(m.args.first())
}

/// The writes found in one file: rule, function, line.
struct Writes<'n> {
    names: &'n Names,
    /// In [`ZONE`]: rule `fs` applies.
    zone: bool,
    /// The functions elsewhere that write ([`helpers`]).
    helpers: &'n Helpers,
    /// The `impl`s the function being read is in.
    impls: Vec<String>,
    func: String,
    binds: Bindings,
    /// In the function being read: a `fs::rename`, a write that is not
    /// `create_new` (`fs::write`, `File::create`, `.create(true)`,
    /// `.truncate(true)`), a `create_new`; the line of the first rename.
    renames: Option<usize>,
    /// A `fs::remove_dir_all` in the function being read.
    removes: bool,
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
        self.removes = false;
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
        if let Some(line) = self.renames
            && self.removes
        {
            self.push("swap", line);
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

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        let ty = match &*i.self_ty {
            syn::Type::Path(p) => p.path.segments.last().map(|s| name(&s.ident)),
            _ => None,
        };
        self.impls.push(ty.unwrap_or_default());
        visit::visit_item_impl(self, i);
        self.impls.pop();
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
            if self.zone
                && (raw_write(&full)
                    || callee(&full, self.impls.last().map(String::as_str))
                        .is_some_and(|c| self.helpers.has(&c)))
            {
                self.push("fs", Self::line(&c.func));
            }
            if full.len() >= 2 && full[full.len() - 2] == "process" && last == Some("id") {
                self.push("pid", Self::line(&c.func));
            }
            self.removes |= matches!((prev, last), (Some("fs"), Some("remove_dir_all")));
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
        if self.zone && sets_write(m) {
            self.push("fs", m.method.span().start().line);
        }
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

/// The writes in `src` (one file): rule, function, line; rule `fs` when
/// `zone`, `helpers` the functions elsewhere that write.
fn writes_in(src: &str, zone: bool, helpers: &Helpers) -> Vec<(&'static str, String, usize)> {
    let file = syn::parse_file(src).unwrap_or_else(|e| panic!("does not parse: {e}\n{src}"));
    let mut names = Names::default();
    names.visit_file(&file);
    let mut out = Writes {
        names: &names,
        zone,
        helpers,
        impls: Vec::new(),
        func: String::new(),
        binds: Bindings::default(),
        renames: None,
        removes: false,
        plain_call: false,
        plain_write: false,
        create_new: false,
        found: Vec::new(),
    };
    out.visit_file(&file);
    if zone {
        for line in names.globs.iter().copied() {
            out.found.push(("fs", "use".to_owned(), line));
        }
    }
    out.found
}

/// [`writes_in`] outside the zone.
fn writes(src: &str) -> Vec<(&'static str, String, usize)> {
    writes_in(src, false, &Helpers::default())
}

/// A function, by what qualifies it (its `impl`'s type, else its module)
/// and its name.
type Key = (String, String);

/// What a call names: a function by [`Key`] (`nofollow::replace_file`,
/// `NewDir::create`), or by its name alone (a method, `crate::…::f`).
#[derive(PartialEq, Eq, Hash)]
enum Callee {
    Key(Key),
    Name(String),
    /// A method call, `.f(…)`: taken for a method of a type of the same
    /// file (`NewDir::create_dir` in `write_tree`), its type unknown.
    Method(String),
}

/// What a path call (aliases expanded) names, `here` qualifying `Self`;
/// `None` for a function of this module (one name).
fn callee(full: &[String], here: Option<&str>) -> Option<Callee> {
    let [.., q, f] = full else {
        return None;
    };
    Some(match q.as_str() {
        "Self" => Callee::Key((here?.to_owned(), f.clone())),
        "crate" | "super" | "self" => Callee::Name(f.clone()),
        _ => Callee::Key((q.clone(), f.clone())),
    })
}

/// The `open` flags that make or change a file.
const WRITE_FLAGS: &[&str] = &["O_CREAT", "O_WRONLY", "O_RDWR", "O_TRUNC", "O_APPEND"];

/// One function of a helper source: whether it writes by itself, and what
/// it calls.
struct Func {
    /// The file's module.
    file: String,
    key: Key,
    direct: bool,
    calls: HashSet<Callee>,
}

/// The functions of one file, for [`helpers`].
struct Calls<'n> {
    names: &'n Names,
    /// The file's module, then the inline `mod`s and `impl`s it is in.
    module: Vec<String>,
    impls: Vec<String>,
    open: Vec<Func>,
    out: Vec<Func>,
}

impl Calls<'_> {
    fn enter(&mut self, ident: &syn::Ident) {
        let q = self
            .impls
            .last()
            .or(self.module.last())
            .cloned()
            .unwrap_or_default();
        self.open.push(Func {
            file: self.module.first().cloned().unwrap_or_default(),
            key: (q, name(ident)),
            direct: false,
            calls: HashSet::new(),
        });
    }
    fn leave(&mut self) {
        if let Some(f) = self.open.pop() {
            self.out.push(f);
        }
    }
}

impl<'ast> Visit<'ast> for Calls<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if !test_only(item_attrs(item)) {
            visit::visit_item(self, item);
        }
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        self.module.push(name(&m.ident));
        visit::visit_item_mod(self, m);
        self.module.pop();
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        let ty = match &*i.self_ty {
            syn::Type::Path(p) => p.path.segments.last().map(|s| name(&s.ident)),
            _ => None,
        };
        self.impls.push(ty.unwrap_or_default());
        visit::visit_item_impl(self, i);
        self.impls.pop();
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.enter(&f.sig.ident);
        visit::visit_item_fn(self, f);
        self.leave();
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        if !test_only(&f.attrs) {
            self.enter(&f.sig.ident);
            visit::visit_impl_item_fn(self, f);
            self.leave();
        }
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let Expr::Path(p) = &*c.func {
            let segs: Vec<String> = p.path.segments.iter().map(|s| name(&s.ident)).collect();
            let full = self.names.expand(segs);
            let here = self.impls.last().cloned();
            let module = self.module.last().cloned().unwrap_or_default();
            if let Some(f) = self.open.last_mut() {
                // An `openat` that only reads is no write (`nofollow::read_*`).
                f.direct |= raw_write(&full) && full.last().is_some_and(|l| l != "openat");
                match callee(&full, here.as_deref()) {
                    Some(c) => f.calls.insert(c),
                    None => f.calls.insert(Callee::Key((module, full[0].clone()))),
                };
            }
        }
        visit::visit_expr_call(self, c);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        if let Some(f) = self.open.last_mut() {
            f.direct |= sets_write(m);
            f.calls.insert(Callee::Method(name(&m.method)));
        }
        visit::visit_expr_method_call(self, m);
    }

    fn visit_ident(&mut self, i: &'ast syn::Ident) {
        if let Some(f) = self.open.last_mut() {
            f.direct |= WRITE_FLAGS.contains(&name(i).as_str());
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        use syn::parse::Parser;
        if let Ok(exprs) =
            Punctuated::<Expr, Token![,]>::parse_terminated.parse2(mac.tokens.clone())
        {
            for e in &exprs {
                self.visit_expr(e);
            }
        } else if let Ok(stmts) = syn::Block::parse_within.parse2(mac.tokens.clone()) {
            for s in &stmts {
                self.visit_stmt(s);
            }
        }
        visit::visit_macro(self, mac);
    }
}

/// The functions that write, found in helper sources: by [`Key`], and the
/// names only writing functions bear (a method call names no type).
#[derive(Default)]
struct Helpers {
    keys: HashSet<Key>,
    names: HashSet<String>,
}

impl Helpers {
    fn has(&self, c: &Callee) -> bool {
        match c {
            Callee::Key(k) => self.keys.contains(k),
            Callee::Name(n) => self.names.contains(n),
            Callee::Method(_) => false,
        }
    }
}

/// The module a source file is: its stem, a `mod.rs` its folder's name,
/// a `lib.rs` its crate's.
fn module_of(rel: &str) -> String {
    let path = Path::new(rel);
    let stem = path.file_stem().unwrap().to_string_lossy();
    match &*stem {
        "mod" => path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        // `src/lib.rs` is this crate's; `../od-x/src/lib.rs` is `od_x`'s.
        "lib" => path
            .ancestors()
            .nth(2)
            .and_then(Path::file_name)
            .map_or_else(
                || "od_server".to_owned(),
                |n| n.to_string_lossy().replace('-', "_"),
            ),
        s => s.to_owned(),
    }
}

/// The functions of `srcs` (module, text) that write: by a raw call
/// ([`raw_write`], [`opens_to_write`], an `open` flag that makes a file) or
/// by calling one that does, to a fixed point.
fn helpers(srcs: &[(String, &str)]) -> Helpers {
    let mut funcs = Vec::new();
    for (module, src) in srcs {
        let file = syn::parse_file(src).unwrap_or_else(|e| panic!("does not parse: {e}\n{src}"));
        let mut names = Names::default();
        names.visit_file(&file);
        let mut calls = Calls {
            names: &names,
            module: vec![module.clone()],
            impls: Vec::new(),
            open: Vec::new(),
            out: Vec::new(),
        };
        calls.visit_file(&file);
        funcs.extend(calls.out);
    }
    let mut writes: Vec<bool> = funcs.iter().map(|f| f.direct).collect();
    loop {
        let mut h = Helpers::default();
        let mut not = HashSet::new();
        for (f, w) in funcs.iter().zip(&writes) {
            if *w {
                h.keys.insert(f.key.clone());
                h.names.insert(f.key.1.clone());
            } else {
                not.insert(f.key.1.clone());
            }
        }
        h.names.retain(|n| !not.contains(n));
        // The writing methods of each file's types.
        let methods: HashSet<(&str, &str)> = funcs
            .iter()
            .zip(&writes)
            .filter(|(f, w)| **w && f.key.0 != f.file)
            .map(|(f, _)| (f.file.as_str(), f.key.1.as_str()))
            .collect();
        let next: Vec<bool> = funcs
            .iter()
            .zip(&writes)
            .map(|(f, w)| {
                *w || f.calls.iter().any(|c| match c {
                    Callee::Method(m) => methods.contains(&(f.file.as_str(), m.as_str())),
                    c => h.has(c),
                })
            })
            .collect();
        if next == writes {
            return h;
        }
        writes = next;
    }
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

/// Whether `rel` (relative to this crate) is in [`ZONE`].
fn in_zone(rel: &str) -> bool {
    ZONE.iter()
        .any(|z| rel == *z || (z.ends_with('/') && rel.starts_with(z)))
}

/// The Rust files under each of `dirs` (a folder or one file), relative
/// to this crate, with their text.
fn sources(dirs: &[&str]) -> Vec<(String, String)> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = Vec::new();
    for dir in dirs {
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
            out.push((rel, std::fs::read_to_string(&file).unwrap()));
        }
    }
    out
}

#[test]
fn user_files_are_written_whole_and_named_as_on_disk() {
    let outside: Vec<(String, String)> = sources(HELPER_SOURCES)
        .into_iter()
        .filter(|(rel, _)| !in_zone(rel))
        .collect();
    let helpers = helpers(
        &outside
            .iter()
            .map(|(rel, text)| (module_of(rel), text.as_str()))
            .collect::<Vec<_>>(),
    );
    let scanned = sources(SCANNED);
    for z in ZONE {
        assert!(
            scanned
                .iter()
                .any(|(rel, _)| rel == z || (z.ends_with('/') && rel.starts_with(z))),
            "zone {z} scans no file"
        );
    }
    let mut found: BTreeMap<(String, String, &str), Vec<String>> = BTreeMap::new();
    for (rel, text) in &scanned {
        for (rule, func, line) in writes_in(text, in_zone(rel), &helpers) {
            found
                .entry((rel.clone(), func, rule))
                .or_default()
                .push(format!("{rel}:{line}"));
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
        // A folder removed and one renamed in one function, however
        // written (2026-11-27: plugin install lost both).
        (
            "fn f() { std::fs::remove_dir_all(&dest); std::fs::rename(&tmp, &dest); }",
            "swap",
        ),
        (
            "use std::fs; fn f() { if dest.exists() { fs::remove_dir_all(&dest)?; }\n    fs::rename(\n        &tmp,\n        &dest,\n    )?; }",
            "swap",
        ),
        (
            "use std::fs::remove_dir_all as rm; use std::fs::rename as mv; fn f() { rm(&d); mv(&t, &d); }",
            "swap",
        ),
        (
            "async fn f() { tokio::fs::remove_dir_all(&d).await; tokio::fs::rename(&t, &d).await; }",
            "swap",
        ),
        (
            "fn f() { std::fs::rename(&t, &d).map_err(|e| { let _ = ::std::fs::remove_dir_all(&t); e }); }",
            "swap",
        ),
        (
            "impl S {\n    fn f() { let undo = || std::fs::remove_dir_all(&t); std::fs::rename(&t, &d); }\n}",
            "swap",
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
        "fn f() { std::fs::remove_dir_all(&d); }",
        "fn f() { std::fs::remove_dir_all(&d); } fn g() { std::fs::rename(&t, &d); }",
        "// std::fs::remove_dir_all(&d)\nfn f() { std::fs::rename(&t, &d); }",
        "#[cfg(test)] mod tests { fn t() { std::fs::remove_dir_all(&d); std::fs::rename(&t, &d); } }",
    ] {
        assert!(rules(src).is_empty(), "{src}: {:?}", rules(src));
    }
}

/// The rules found in `src`, a zone module, with `others` (module, text)
/// the rest of the crates.
fn zone_rules(src: &str, others: &[(&str, &str)]) -> Vec<&'static str> {
    let others: Vec<(String, &str)> = others.iter().map(|(m, t)| ((*m).to_owned(), *t)).collect();
    writes_in(src, true, &helpers(&others))
        .into_iter()
        .map(|(r, _, _)| r)
        .collect()
}

/// A module elsewhere with a helper that writes, one that writes through
/// it, a type whose method writes, and one that only reads.
const UTIL: (&str, &str) = (
    "util",
    "pub fn put(p: &Path) { std::fs::write(p, b\"x\").unwrap(); }
     pub fn put_twice(p: &Path) { put(p); put(p); }
     pub fn peek(p: &Path) { std::fs::read(p).unwrap(); }
     pub struct Box2;
     impl Box2 {
         pub fn fill(&self, p: &Path) { self.drop_in(p) }
         fn drop_in(&self, p: &Path) { std::fs::remove_file(p).unwrap(); }
     }",
);

/// `od-fs`'s `nofollow`, as the guard reads it: its writers are helpers too.
const NOFOLLOW: (&str, &str) = (
    "nofollow",
    "pub fn replace_file(p: &Path) { replace_file_through(p) }
     fn replace_file_through(p: &Path) { nix::fcntl::renameat(a, b, c, d); }
     pub fn read_file(p: &Path) { let fd = nix::fcntl::openat(d, p, OFlag::O_RDONLY, m); }
     pub struct NewDir;
     impl NewDir {
         pub fn create(p: &Path) -> Self { std::fs::create_dir(p).unwrap(); NewDir }
         pub fn new_file(&self, p: &Path) { openat(d, p, OFlag::O_CREAT | OFlag::O_EXCL, m); }
     }
     pub fn write_tree(to: &NewDir) { to.new_file(p); }",
);

/// Every way of changing the file system in a zone module is caught: raw,
/// through an alias, a helper in another module, open options, `tokio`,
/// `nix`, `libc`; a read, `user_store`, a comment or a test is not.
#[test]
fn every_spelling_of_a_write_in_the_zone_is_caught() {
    let others = [UTIL, NOFOLLOW];
    for src in [
        // Raw, however named.
        "fn f() { std::fs::write(p, b); }",
        "fn f() { ::std::fs::write(p, b); }",
        "use std::fs; fn f() { fs::write(p, b); }",
        "use std::fs as f; fn g() { f::write(p, b); }",
        "use std::fs::write as put; fn f() { put(p, b); }",
        "fn f() { std::fs\n    ::create_dir_all(\n        p,\n    ); }",
        "fn f() { std::fs::create_dir(p); }",
        "fn f() { std::fs::remove_file(p); }",
        "fn f() { std::fs::remove_dir(p); }",
        "fn f() { std::fs::remove_dir_all(p); }",
        "fn f() { std::fs::rename(a, b); }",
        "fn f() { std::fs::copy(a, b); }",
        "fn f() { std::fs::hard_link(a, b); }",
        "fn f() { std::fs::set_permissions(p, m); }",
        "fn f() { std::os::unix::fs::symlink(a, b); }",
        "use std::fs::*; fn f() { write(p, b); }",
        // A file opened to write.
        "fn f() { std::fs::File::create(p); }",
        "use std::fs::File; type F = File; fn f() { F::create(p); }",
        "fn f() { std::fs::File::create_new(p); }",
        "fn f() { std::fs::OpenOptions::new().write(true).open(p); }",
        "use std::fs::OpenOptions; fn f() { OpenOptions::new().append(true).open(p); }",
        "fn f() { std::fs::File::options().create(true).open(p); }",
        "fn f() { let mut o = std::fs::OpenOptions::new(); o.truncate(true); o.open(p); }",
        // `tokio::fs`.
        "async fn f() { tokio::fs::write(p, b).await; }",
        "use tokio::fs; async fn f() { fs::remove_dir_all(p).await; }",
        "async fn f() { tokio::fs::rename(a, b).await; }",
        // Syscalls.
        "fn f() { nix::fcntl::renameat(a, b, c, d); }",
        "use nix::unistd::unlinkat; fn f() { unlinkat(d, n, fl); }",
        "fn f() { nix::sys::stat::mkdirat(d, n, m); }",
        "fn f() { nix::fcntl::openat(d, n, fl, m); }",
        "fn f() { unsafe { libc::unlink(p) }; }",
        "fn f() { unsafe { nix::libc::rmdir(p) }; }",
        // A helper in another module that writes, however reached.
        "fn f() { util::put(p); }",
        "fn f() { crate::routes::util::put(p); }",
        "use crate::util::put; fn f() { put(p); }",
        "use crate::util::put as store; fn f() { store(p); }",
        "use crate::util; fn f() { util::put_twice(p); }",
        "fn f() { util::Box2::fill(&b, p); }",
        "fn f() { super::put_twice(p); }",
        "fn f() { od_content::nofollow::replace_file(p); }",
        "use od_fs::nofollow; fn f() { nofollow::write_tree(&d); }",
        "use od_fs::nofollow::NewDir; fn f() { let d = NewDir::create(p); }",
        // In a macro, a closure, an impl.
        "fn f() { assert!(std::fs::write(p, b).is_ok()); }",
        "fn f() { let w = || std::fs::write(p, b); }",
        "impl S {\n    fn f(&self) { std::fs::remove_file(p); }\n}",
    ] {
        assert_eq!(zone_rules(src, &others), ["fs"], "{src}");
    }
    for src in [
        "// std::fs::write(p, b)\nfn f() {}",
        "fn f() { let s = \"std::fs::write(p, b)\"; }",
        "fn f() { std::fs::read(p); std::fs::read_dir(p); std::fs::symlink_metadata(p); }",
        "fn f() { std::fs::OpenOptions::new().read(true).open(p); }",
        "fn f() { std::fs::File::open(p); }",
        "fn f() { util::peek(p); }",
        "fn f() { od_content::nofollow::read_file(p); }",
        "fn f() { let one = root.lock(); one.write(rel, b, Expect::Any); one.remove(rel); }",
        "fn f() { root.make_root(); one.make_dir(n); one.put_copy(r, k, &t, None); }",
        "fn f() { UserRoot::open(p, n).lock().remove_hidden(r); }",
        "fn f() { local_helper(p); }",
        "#[cfg(test)] mod tests { fn t() { std::fs::write(p, b); } }",
        "#[test] fn t() { std::fs::remove_dir_all(p); }",
        "#[cfg(test)] use std::fs::*;",
    ] {
        assert!(
            zone_rules(src, &others).is_empty(),
            "{src}: {:?}",
            zone_rules(src, &others)
        );
    }
    // Outside the zone, the rule is off.
    assert!(rules("fn f() { std::fs::remove_file(p); }").is_empty());
}
