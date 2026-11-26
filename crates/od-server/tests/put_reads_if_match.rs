//! Every `PUT` route reads `If-Match` (`od_server::cas::IfMatch`), so a
//! form that saves a user's file never writes over a change made since it
//! opened (an agent, a replace, another window): `src/cas.rs`. A `PUT`
//! that does not is listed in [`KEPT`] with why.
//!
//! The sources are parsed as Rust (`syn`): comments and strings are not
//! code, line breaks do not matter. A `PUT` is a call of axum's `put`
//! (through `use … as`, or a full path), a `.put(h)` on a method router,
//! or `on(MethodFilter::PUT, h)` / `.on(…PUT…, h)`. Its handler is found
//! by name (through `use … as`, module paths) among the `fn`s of `src/`; a
//! closure, or a name not found, fails. The handler reads `If-Match` when
//! one of its arguments has the type `IfMatch` (through `use … as` and
//! `type … =`) under a name that does not start with `_` (an unused one
//! fails clippy's `unused_variables` anyway). A `PUT` outside a
//! `.route("…", …)` has no path to be listed under, so it must read it.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use syn::visit::{self, Visit};
use syn::{Expr, FnArg, Pat, UseTree};

/// `PUT` routes that do not read `If-Match`: path, why.
const KEPT: &[(&str, &str)] = &[
    (
        "/api/app-config",
        "an RFC 7396 merge patch of only the leaves changed (`patch_since`), in call-order turns; not a file the user types",
    ),
    (
        "/api/project-locations",
        "its own route under `CONFIG_LOCK`, changed one location at a time (#246)",
    ),
    (
        "/api/agent-cli-env",
        "app config (agent environment variables), not a file the user types; saved whole from Settings",
    ),
    (
        "/api/mcp/servers",
        "app config (the MCP server list), not a file the user types; replaced whole (od-server trap, #246)",
    ),
    (
        "/api/media/config",
        "a separate config store replaced whole (od-server trap, #246); not a file the user types",
    ),
    (
        "/api/projects/{id}/critique",
        "one switch (`enabled`), nothing typed to lose",
    ),
    (
        "/api/projects/{id}/conversations/{cid}/messages/{mid}",
        "an upsert of one chat message by its id for API clients (no form in the app opens on it); not a user's file",
    ),
    (
        "/api/projects/{id}/content/{*path}",
        "the Studio editors' autosave (Markdown, sketch, speaker notes): unsaved edits win over the agent's by design (`doc_view.rs`), an HTML write keeps a version",
    ),
];

#[derive(Clone)]
struct FnDef {
    module: Vec<String>,
    file: PathBuf,
    item: syn::ItemFn,
}

/// A file's `use` and `type` names: name → full path.
#[derive(Default)]
struct Names {
    uses: HashMap<String, Vec<String>>,
    types: HashMap<String, syn::Type>,
}

fn add_use(tree: &UseTree, prefix: &mut Vec<String>, out: &mut HashMap<String, Vec<String>>) {
    match tree {
        UseTree::Path(p) => {
            prefix.push(p.ident.to_string());
            add_use(&p.tree, prefix, out);
            prefix.pop();
        }
        UseTree::Name(n) => {
            let mut full = prefix.clone();
            full.push(n.ident.to_string());
            out.insert(n.ident.to_string(), full);
        }
        UseTree::Rename(r) => {
            let mut full = prefix.clone();
            full.push(r.ident.to_string());
            out.insert(r.rename.to_string(), full);
        }
        UseTree::Group(g) => g.items.iter().for_each(|t| add_use(t, prefix, out)),
        UseTree::Glob(_) => {}
    }
}

/// Whether attributes leave the item out of a non-test build.
fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || (a.path().is_ident("cfg")
                && a.parse_args::<syn::Path>()
                    .is_ok_and(|p| p.is_ident("test")))
    })
}

fn collect_names(items: &[syn::Item], names: &mut Names) {
    for item in items {
        match item {
            syn::Item::Use(u) if !test_only(&u.attrs) => {
                add_use(&u.tree, &mut Vec::new(), &mut names.uses);
            }
            syn::Item::Type(t) if !test_only(&t.attrs) => {
                names.types.insert(t.ident.to_string(), (*t.ty).clone());
            }
            syn::Item::Mod(m) if !test_only(&m.attrs) => {
                if let Some((_, inner)) = &m.content {
                    collect_names(inner, names);
                }
            }
            _ => {}
        }
    }
}

fn collect_fns(items: &[syn::Item], module: &[String], file: &Path, out: &mut Vec<FnDef>) {
    for item in items {
        match item {
            syn::Item::Fn(f) if !test_only(&f.attrs) => out.push(FnDef {
                module: module.to_vec(),
                file: file.to_path_buf(),
                item: f.clone(),
            }),
            syn::Item::Mod(m) if !test_only(&m.attrs) => {
                if let Some((_, inner)) = &m.content {
                    let mut sub = module.to_vec();
                    sub.push(m.ident.to_string());
                    collect_fns(inner, &sub, file, out);
                }
            }
            _ => {}
        }
    }
}

/// The module path of source file `rel` (relative to `src`).
fn module_of(rel: &Path) -> Vec<String> {
    let mut parts: Vec<String> = rel
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let last = parts.pop().unwrap();
    let stem = last.trim_end_matches(".rs");
    if stem != "mod" && stem != "lib" && stem != "main" {
        parts.push(stem.to_owned());
    }
    parts
}

fn segments(path: &syn::Path) -> Vec<String> {
    path.segments.iter().map(|s| s.ident.to_string()).collect()
}

/// `path` with its first name expanded through the file's `use`s.
fn resolve(path: &syn::Path, names: &Names) -> Vec<String> {
    let segs = segments(path);
    match names.uses.get(&segs[0]) {
        Some(full) => full
            .iter()
            .cloned()
            .chain(segs[1..].iter().cloned())
            .collect(),
        None => segs,
    }
}

fn is_routing(full: &[String], name: &str) -> bool {
    full.last().is_some_and(|l| l == name) && full.iter().any(|s| s == "routing")
}

/// A `PUT` found: the route path it was given under, its handler.
struct Put {
    route: Option<String>,
    handler: Expr,
    line: usize,
}

struct Finder<'a> {
    names: &'a Names,
    route: Option<String>,
    found: Vec<Put>,
}

impl Finder<'_> {
    fn push(&mut self, handler: &Expr) {
        let line = proc_macro2_line(handler);
        self.found.push(Put {
            route: self.route.clone(),
            handler: handler.clone(),
            line,
        });
    }

    /// Whether `e` is (a method chain on) a call of an axum routing fn.
    fn method_router(&self, e: &Expr) -> bool {
        match e {
            Expr::MethodCall(m) => self.method_router(&m.receiver),
            Expr::Call(c) => match &*c.func {
                Expr::Path(p) => resolve(&p.path, self.names).iter().any(|s| s == "routing"),
                _ => false,
            },
            Expr::Paren(p) => self.method_router(&p.expr),
            _ => false,
        }
    }
}

fn proc_macro2_line(e: &Expr) -> usize {
    use syn::spanned::Spanned;
    e.span().start().line
}

/// Whether `e` names `PUT` (`MethodFilter::PUT`, `….or(MethodFilter::PUT)`).
fn names_put(e: &Expr) -> bool {
    struct Seen(bool);
    impl<'ast> Visit<'ast> for Seen {
        fn visit_path_segment(&mut self, s: &'ast syn::PathSegment) {
            self.0 |= s.ident == "PUT";
            visit::visit_path_segment(self, s);
        }
    }
    let mut seen = Seen(false);
    seen.visit_expr(e);
    seen.0
}

impl<'ast> Visit<'ast> for Finder<'_> {
    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if !test_only(&m.attrs) {
            visit::visit_item_mod(self, m);
        }
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        if !test_only(&f.attrs) {
            visit::visit_item_fn(self, f);
        }
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let name = m.method.to_string();
        if name == "route" && m.args.len() == 2 {
            self.visit_expr(&m.receiver);
            let path = match &m.args[0] {
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) => Some(s.value()),
                _ => None,
            };
            let outer = std::mem::replace(&mut self.route, path);
            self.visit_expr(&m.args[1]);
            self.route = outer;
            return;
        }
        if name == "put" && m.args.len() == 1 && self.method_router(&m.receiver) {
            self.push(&m.args[0]);
        }
        if name == "on"
            && m.args.len() == 2
            && names_put(&m.args[0])
            && self.method_router(&m.receiver)
        {
            self.push(&m.args[1]);
        }
        visit::visit_expr_method_call(self, m);
    }

    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        if let Expr::Path(p) = &*c.func {
            let full = resolve(&p.path, self.names);
            if is_routing(&full, "put") && c.args.len() == 1 {
                self.push(&c.args[0]);
            }
            if is_routing(&full, "on") && c.args.len() == 2 && names_put(&c.args[0]) {
                self.push(&c.args[1]);
            }
        }
        visit::visit_expr_call(self, c);
    }
}

/// The type `ty` names, through the file's `use`s and `type`s, last name
/// first; and every type inside its generics.
fn type_names(ty: &syn::Type, names: &Names, depth: usize, out: &mut Vec<String>) {
    if depth > 8 {
        return;
    }
    if let syn::Type::Path(p) = ty {
        let segs = segments(&p.path);
        if segs.len() == 1
            && let Some(aliased) = names.types.get(&segs[0])
        {
            type_names(aliased, names, depth + 1, out);
            return;
        }
        if let Some(last) = resolve(&p.path, names).last() {
            out.push(last.clone());
        }
        for seg in &p.path.segments {
            if let syn::PathArguments::AngleBracketed(a) = &seg.arguments {
                for arg in &a.args {
                    if let syn::GenericArgument::Type(t) = arg {
                        type_names(t, names, depth + 1, out);
                    }
                }
            }
        }
    }
}

fn binds_a_name(pat: &Pat) -> bool {
    match pat {
        Pat::Ident(i) => !i.ident.to_string().starts_with('_'),
        Pat::TupleStruct(t) => t.elems.iter().any(binds_a_name),
        Pat::Tuple(t) => t.elems.iter().any(binds_a_name),
        Pat::Type(t) => binds_a_name(&t.pat),
        _ => false,
    }
}

fn reads_if_match(f: &FnDef, names: &Names) -> bool {
    f.item.sig.inputs.iter().any(|arg| {
        let FnArg::Typed(t) = arg else { return false };
        let mut found = Vec::new();
        type_names(&t.ty, names, 0, &mut found);
        found.iter().any(|n| n == "IfMatch") && binds_a_name(&t.pat)
    })
}

/// The `fn` handler `path` (written in file `file` of module `module`).
fn find_handler<'a>(
    path: &syn::Path,
    names: &Names,
    module: &[String],
    file: &Path,
    fns: &'a [FnDef],
) -> Result<&'a FnDef, String> {
    let full = resolve(path, names);
    let name = full.last().unwrap();
    let mut qualifier: Vec<String> = Vec::new();
    let mut base = module.to_vec();
    for seg in &full[..full.len() - 1] {
        match seg.as_str() {
            "crate" => base.clear(),
            "self" => {}
            "super" => {
                base.pop();
            }
            other => qualifier.push(other.to_owned()),
        }
    }
    let candidates: Vec<&FnDef> = fns
        .iter()
        .filter(|f| f.item.sig.ident == name.as_str())
        .filter(|f| f.module.ends_with(&qualifier))
        .collect();
    let same_file: Vec<&FnDef> = candidates
        .iter()
        .copied()
        .filter(|f| qualifier.is_empty() && f.file == file)
        .collect();
    let under_base: Vec<&FnDef> = candidates
        .iter()
        .copied()
        .filter(|f| {
            let mut want = base.clone();
            want.extend(qualifier.iter().cloned());
            f.module == want
        })
        .collect();
    match (
        same_file.as_slice(),
        under_base.as_slice(),
        candidates.as_slice(),
    ) {
        ([one], _, _) | (_, [one], _) | (_, _, [one]) => Ok(one),
        _ => Err(format!(
            "handler `{}` not found ({} candidates)",
            full.join("::"),
            candidates.len()
        )),
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

/// Parsed sources: (relative path, file).
type Sources = Vec<(PathBuf, syn::File)>;

/// What breaks the rule in `sources`: `file:line route: why`.
fn violations(sources: &Sources) -> Vec<String> {
    let mut fns = Vec::new();
    let mut names_of = BTreeMap::new();
    for (rel, file) in sources {
        collect_fns(&file.items, &module_of(rel), rel, &mut fns);
        let mut names = Names::default();
        collect_names(&file.items, &mut names);
        names_of.insert(rel.clone(), names);
    }
    let kept: HashMap<&str, &str> = KEPT.iter().copied().collect();
    let mut out = Vec::new();
    for (rel, file) in sources {
        let names = &names_of[rel];
        let mut finder = Finder {
            names,
            route: None,
            found: Vec::new(),
        };
        finder.visit_file(file);
        for put in finder.found {
            let at = format!(
                "{}:{} {}",
                rel.display(),
                put.line,
                put.route.as_deref().unwrap_or("(no route path)")
            );
            let handler = match &put.handler {
                Expr::Path(p) => find_handler(&p.path, names, &module_of(rel), rel, &fns)
                    .map(|f| (f, &names_of[&f.file])),
                _ => Err("a closure or expression handler, not a `fn`".to_owned()),
            };
            match handler {
                Ok((f, f_names)) if reads_if_match(f, f_names) => {}
                Ok(_) if put.route.as_deref().is_some_and(|r| kept.contains_key(r)) => {}
                Ok(f) => out.push(format!(
                    "{at}: `{}` reads no `IfMatch` (add one, or list the route in KEPT with why)",
                    f.0.item.sig.ident
                )),
                Err(why) => out.push(format!("{at}: {why}")),
            }
        }
    }
    out
}

fn parse(text: &str) -> syn::File {
    syn::parse_file(text).unwrap()
}

fn src() -> Sources {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            (p.strip_prefix(&root).unwrap().to_path_buf(), parse(&text))
        })
        .collect()
}

#[test]
fn every_put_route_reads_if_match() {
    let bad = violations(&src());
    assert!(
        bad.is_empty(),
        "PUT routes that do not read If-Match:\n{}",
        bad.join("\n")
    );
}

#[test]
fn every_kept_route_is_still_a_put() {
    // A listed route that went (or moved to a handler that reads it)
    // leaves the list.
    let sources = src();
    let mut puts = Vec::new();
    for (rel, file) in &sources {
        let mut names = Names::default();
        collect_names(&file.items, &mut names);
        let mut finder = Finder {
            names: &names,
            route: None,
            found: Vec::new(),
        };
        finder.visit_file(file);
        let _ = rel;
        puts.extend(finder.found.into_iter().filter_map(|p| p.route));
    }
    for (route, _) in KEPT {
        assert!(
            puts.iter().any(|p| p == route),
            "{route} is listed in KEPT but is no PUT route"
        );
    }
}

/// The rule, on sources written every way it may meet: each case is a
/// `routes/x.rs` beside a `cas.rs`.
fn check(x: &str) -> Vec<String> {
    let sources = vec![
        (
            PathBuf::from("cas.rs"),
            parse("pub struct IfMatch(pub Option<String>);"),
        ),
        (PathBuf::from("routes/x.rs"), parse(x)),
        (
            PathBuf::from("routes/other.rs"),
            parse("pub async fn save(t: crate::cas::IfMatch) {} pub async fn bare() {}"),
        ),
    ];
    violations(&sources)
}

const READS: &str = "use crate::cas::IfMatch; async fn h(if_match: IfMatch) {}";
const READS_NOT: &str = "async fn h(body: String) {}";

#[test]
fn the_rule_finds_a_put_written_any_way() {
    let cases: &[(&str, &str)] = &[
        (
            "bare",
            "use axum::routing::put; fn r() { R::new().route(\"/a\", put(h)); }",
        ),
        (
            "grouped",
            "use axum::routing::{get, put}; fn r() { R::new().route(\"/a\", get(g).put(h)); }",
        ),
        (
            "full path",
            "fn r() { R::new().route(\"/a\", axum::routing::put(h)); }",
        ),
        (
            "renamed",
            "use axum::routing::put as p; fn r() { R::new().route(\"/a\", p(h)); }",
        ),
        (
            "routing module",
            "use axum::routing; fn r() { R::new().route(\"/a\", routing::put(h)); }",
        ),
        (
            "method",
            "use axum::routing::get; fn r() { R::new().route(\"/a\", get(g).put(h)); }",
        ),
        (
            "broken lines",
            "use axum::routing::get; fn r() { R::new().route(\n\"/a\",\nget(g)\n.put(\nh,\n),\n); }",
        ),
        (
            "on PUT",
            "use axum::routing::{on, MethodFilter}; fn r() { R::new().route(\"/a\", on(MethodFilter::PUT, h)); }",
        ),
        (
            "on PUT or",
            "use axum::routing::{on, MethodFilter}; fn r() { R::new().route(\"/a\", on(MethodFilter::POST.or(MethodFilter::PUT), h)); }",
        ),
        (
            "method on",
            "use axum::routing::{get, MethodFilter}; fn r() { R::new().route(\"/a\", get(g).on(MethodFilter::PUT, h)); }",
        ),
        (
            "variable",
            "use axum::routing::put; fn r() { let m = put(h); R::new().route(\"/a\", m); }",
        ),
        (
            "self path",
            "use axum::routing::put; fn r() { R::new().route(\"/a\", put(self::h)); }",
        ),
    ];
    for (what, router) in cases {
        let bad = check(&format!("{router} {READS_NOT}"));
        assert_eq!(bad.len(), 1, "{what}: {bad:?}");
        let ok = check(&format!("{router} {READS}"));
        assert!(ok.is_empty(), "{what}: {ok:?}");
    }
}

#[test]
fn the_rule_reads_the_handler_however_it_names_if_match() {
    let route = "use axum::routing::put; fn r() { R::new().route(\"/a\", put(h)); }";
    let ok: &[&str] = &[
        "async fn h(t: crate::cas::IfMatch) {}",
        "use crate::cas::IfMatch as Tag; async fn h(t: Tag) {}",
        "use crate::cas; async fn h(t: cas::IfMatch) {}",
        "use crate::cas::IfMatch; type Tag = IfMatch; async fn h(t: Tag) {}",
        "use crate::cas::IfMatch; async fn h(IfMatch(t): IfMatch) {}",
        "use crate::cas::IfMatch; async fn h(s: State<S>, t: Option<IfMatch>, b: Json<B>) {}",
    ];
    for handler in ok {
        assert!(check(&format!("{route} {handler}")).is_empty(), "{handler}");
    }
    let bad: &[&str] = &[
        "async fn h(body: String) {}",
        "use crate::cas::IfMatch; async fn h(_t: IfMatch) {}",
        "use crate::cas::IfMatch; async fn h(_: IfMatch) {}",
        "use crate::cas::IfMatch; async fn h(IfMatch(_): IfMatch) {}",
        // A comment or a string is not an argument.
        "async fn h(b: String) { /* IfMatch */ let _ = \"IfMatch\"; }",
        // Only built for tests.
        "async fn h(b: String) {} #[cfg(test)] mod t { async fn h(t: crate::cas::IfMatch) {} }",
    ];
    for handler in bad {
        assert_eq!(check(&format!("{route} {handler}")).len(), 1, "{handler}");
    }
}

#[test]
fn the_rule_follows_the_handler_to_its_module() {
    let in_other = "use axum::routing::put; use super::other; fn r() { R::new().route(\"/a\", put(other::save)); }";
    assert!(check(in_other).is_empty());
    let full = "use axum::routing::put; fn r() { R::new().route(\"/a\", put(crate::routes::other::save)); }";
    assert!(check(full).is_empty());
    let imported = "use axum::routing::put; use super::other::save; fn r() { R::new().route(\"/a\", put(save)); }";
    assert!(check(imported).is_empty());
    let bare =
        "use axum::routing::put; fn r() { R::new().route(\"/a\", put(super::other::bare)); }";
    assert_eq!(check(bare).len(), 1);
    let closure = "use axum::routing::put; fn r() { R::new().route(\"/a\", put(|| async {})); }";
    assert_eq!(check(closure).len(), 1);
    let missing = "use axum::routing::put; fn r() { R::new().route(\"/a\", put(nowhere)); }";
    assert_eq!(check(missing).len(), 1);
}

#[test]
fn a_kept_route_needs_no_if_match_and_nothing_else_does() {
    let kept = format!(
        "use axum::routing::put; fn r() {{ R::new().route(\"/api/app-config\", put(h)); }} {READS_NOT}"
    );
    assert!(check(&kept).is_empty());
    let other = format!(
        "use axum::routing::put; fn r() {{ R::new().route(\"/api/app-config/x\", put(h)); }} {READS_NOT}"
    );
    assert_eq!(check(&other).len(), 1);
    // Not axum's: a `put` of something else, a method `.put` on no router.
    let not_routes = format!("fn r() {{ map.put(h); store::put(h); }} {READS_NOT}");
    assert!(check(&not_routes).is_empty());
}
