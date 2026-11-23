//! Work the user asked of the server runs to its end whatever the screen
//! does (rule 2 of `src/app/tasks.rs`, invariant 1 of `CLAUDE.md`).
//!
//! `Tasks::spawn` always runs a task's first step, up to its first
//! `await`, and drops the rest once its owner went. A request's `fetch`
//! starts inside the step that calls it (gloo-net's `Request::send` calls
//! `window.fetch` before it awaits), so a write that is the task's first
//! `await` goes out. A write is lost when the task reaches it only later:
//! - after another `await` of the same task (a turn, a snapshot, a timer);
//! - in a loop that awaits (the second upload of a batch);
//! - inside a helper that waits before it sends (`Shell::save_config`
//!   waits for the window's turn).
//!
//! This test finds those in every `.spawn(…)` and `.promise(…)` under
//! `src`. A write is a call of `api::post`/`put`/`patch`/`delete` (and
//! their `_form`, `_bytes`, `_text` forms) or gloo's `Request::post`/…,
//! or of a function whose every definition of that name makes one,
//! transitively. The scan cannot tell work from a UI step that only makes
//! sense while the screen is there, so each site it finds is either moved
//! to `Tasks::spawn_then` / `tasks::detached`, or listed in [`KEPT`] with
//! why it may stay.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// `(file under src, the tasks that spawn it (`ctx.tasks`, `tasks`…), the
/// late writes of the task joined by "+", times, why it may stay)`. The
/// receiver says whose task it is: the same write in the task of a panel
/// that goes first (`tasks` of the draw layer) is not kept.
const KEPT: &[Kept] = &[
    (
        "app/automations.rs",
        "screen",
        "api::post",
        1,
        "apply or reject: two branches, each one request at the task's first await",
    ),
    (
        "app/capture.rs",
        "tasks",
        "mark_exported",
        1,
        "counted after a download that itself needs the Studio: no download, no export",
    ),
    (
        "app/capture.rs",
        "tasks",
        "save_png",
        1,
        "the menu goes only with the preview frame (code view, a file not HTML, a \
         version shown) and the shot reads that frame: no frame, no shot to save",
    ),
    (
        "app/chat_ops.rs",
        "ctx.tasks",
        "new_conversation",
        1,
        "the delete goes first; a fresh conversation after the last one went is \
         the view's, made again on the next visit",
    ),
    (
        "app/chat_ops.rs",
        "ctx.tasks",
        "finish_link",
        1,
        "the folder comes from the desktop's modal picker: the window takes no \
         input while it is open, so the Studio cannot be left before it answers",
    ),
    (
        "app/comments_panel.rs",
        "ctx.tasks",
        "drop_draft_images",
        1,
        "deletes images of a draft cancelled meanwhile; with the Studio gone its \
         draft session is released, which deletes them anyway",
    ),
    (
        "app/files_panel.rs",
        "ctx.tasks",
        "mark_exported",
        2,
        "counted after a ZIP download that itself needs the Studio: no download, no export",
    ),
    (
        "app/manual_edit.rs",
        "self.tasks",
        "manual_save_content",
        1,
        "the upload goes first; putting it in the page reads the panel's selected \
         element, and the image is a draft image released with the page",
    ),
    (
        "app/preview_tools.rs",
        "ctx.tasks",
        "attach_annotation",
        1,
        "the Studio's task: the snapshot reads the Studio's preview and the image \
         lands in its composer; the draw layer going does not stop it",
    ),
];

/// A site that may stay: see [`KEPT`].
type Kept = (
    &'static str,
    &'static str,
    &'static str,
    usize,
    &'static str,
);

/// The calls that send a write to the daemon.
const WRITES: &[&str] = &[
    "api::post",
    "api::put",
    "api::patch",
    "api::delete",
    "Request::post",
    "Request::put",
    "Request::patch",
    "Request::delete",
];

/// The spawners whose task ends with its owner.
const SPAWNERS: &[&str] = &[".spawn(", ".promise("];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `src` with comments and the inside of string and char literals blanked
/// (newlines kept), so braces and names in them count for nothing.
fn mask(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let blank = |out: &mut Vec<u8>, c: u8| out.push(if c == b'\n' { b'\n' } else { b' ' });
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                blank(&mut out, b[i]);
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                blank(&mut out, b[i]);
                i += 1;
            }
            out.extend_from_slice(b"  ");
            i += 2;
        } else if c == b'r'
            && (b.get(i + 1) == Some(&b'"') || b.get(i + 1) == Some(&b'#'))
            && (i == 0 || !is_ident(b[i - 1]))
        {
            let hashes = b[i + 1..].iter().take_while(|&&h| h == b'#').count();
            if b.get(i + 1 + hashes) != Some(&b'"') {
                out.push(c);
                i += 1;
                continue;
            }
            let start = i + 2 + hashes;
            out.resize(out.len() + (start - i), b' ');
            i = start;
            let close: Vec<u8> = std::iter::once(b'"')
                .chain(std::iter::repeat_n(b'#', hashes))
                .collect();
            while i < b.len() && !b[i..].starts_with(&close) {
                blank(&mut out, b[i]);
                i += 1;
            }
            out.resize(out.len() + close.len(), b' ');
            i += close.len();
        } else if c == b'"' {
            out.push(b' ');
            i += 1;
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' {
                    blank(&mut out, b[i]);
                    i += 1;
                }
                if i < b.len() {
                    blank(&mut out, b[i]);
                    i += 1;
                }
            }
            out.push(b' ');
            i += 1;
        } else if c == b'\'' && is_char_literal(&b[i..]) {
            // `'\''` closes after its escape, not at the escaped quote.
            let from = if b.get(i + 1) == Some(&b'\\') { 3 } else { 1 };
            let len = b[i + from..].iter().position(|&x| x == b'\'').unwrap_or(0) + from + 1;
            out.resize(out.len() + len, b' ');
            i += len;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out.truncate(b.len());
    String::from_utf8_lossy(&out).into_owned()
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Whether `s` (starting at a `'`) opens a char literal, not a lifetime.
fn is_char_literal(s: &[u8]) -> bool {
    match s.get(1) {
        Some(b'\\') => true,
        Some(_) => {
            // One char (maybe multibyte) then `'`; a lifetime has no `'`.
            let rest = &s[1..];
            let width = String::from_utf8_lossy(&rest[..rest.len().min(4)])
                .chars()
                .next()
                .map_or(1, char::len_utf8);
            rest.get(width) == Some(&b'\'')
        }
        None => false,
    }
}

/// Where the block opened at `open` (a `{` or `(`) closes.
fn block_end(src: &str, open: usize) -> usize {
    let (o, c) = match src.as_bytes()[open] {
        b'(' => (b'(', b')'),
        _ => (b'{', b'}'),
    };
    let mut depth = 0usize;
    for (i, &x) in src.as_bytes()[open..].iter().enumerate() {
        if x == o {
            depth += 1;
        } else if x == c {
            depth -= 1;
            if depth == 0 {
                return open + i;
            }
        }
    }
    src.len()
}

/// `body` without the `detached(async …)` blocks inside it: those run to
/// their end whatever the screen does.
fn outside_detached(body: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(at) = find_call(rest, "detached").filter(|&at| {
        rest[at + "detached(".len()..]
            .trim_start()
            .starts_with("async")
    }) {
        out.push_str(&rest[..at]);
        let Some(open) = rest[at..].find('{').map(|i| at + i) else {
            break;
        };
        let end = block_end(rest, open);
        rest = &rest[(end + 1).min(rest.len())..];
    }
    out.push_str(rest);
    out
}

/// The first call of `name` in `src` (`name(` or `name::<`, not part of a
/// longer name).
fn find_call(src: &str, name: &str) -> Option<usize> {
    calls_of(src, name).into_iter().next()
}

fn calls_of(src: &str, name: &str) -> Vec<usize> {
    src.match_indices(name)
        .map(|(at, _)| at)
        .filter(|&at| {
            let before = src[..at].bytes().next_back();
            let after = &src[at + name.len()..];
            !before.is_some_and(is_ident) && (after.starts_with('(') || after.starts_with("::<"))
        })
        .collect()
}

/// The writes in `body`, as `(offset, name)`: direct ones, and calls of a
/// function in `writers`.
fn writes_in(body: &str, writers: &BTreeSet<String>) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for w in WRITES {
        for (at, _) in body.match_indices(w) {
            let after = body[at + w.len()..].bytes().next();
            let before = body[..at].bytes().next_back();
            // `api::post_form` is a write too; `api::posted` is not a call.
            if !before.is_some_and(is_ident)
                && after.is_some_and(|c| c == b'(' || c == b':' || c == b'_')
            {
                found.push((at, (*w).to_owned()));
            }
        }
    }
    for name in writers {
        found.extend(
            calls_of(body, name)
                .into_iter()
                .map(|at| (at, name.clone())),
        );
    }
    found
}

/// The writes of `body` its task may never reach: after its first
/// `await`, in a loop that awaits, or in a function in `waiting`.
fn late_writes(
    body: &str,
    writers: &BTreeSet<String>,
    waiting: &BTreeSet<String>,
) -> BTreeSet<String> {
    let body = outside_detached(body);
    let mut late = BTreeSet::new();
    if let Some(first) = body.find(".await") {
        late.extend(
            writes_in(&body, writers)
                .into_iter()
                .filter(|(at, _)| *at > first)
                .map(|(_, n)| n),
        );
    }
    for name in waiting {
        if find_call(&body, name).is_some() {
            late.insert(name.clone());
        }
    }
    for kw in ["for ", "while ", "loop "] {
        for (at, _) in body.match_indices(kw) {
            if body[..at].bytes().next_back().is_some_and(is_ident) {
                continue;
            }
            let Some(open) = body[at..].find('{').map(|i| at + i) else {
                continue;
            };
            let inner = &body[open..block_end(&body, open)];
            if inner.contains(".await") {
                late.extend(writes_in(inner, writers).into_iter().map(|(_, n)| n));
            }
        }
    }
    late
}

/// A function: whether it is `async`, and its body.
struct Def {
    is_async: bool,
    body: String,
}

/// The functions of `sources` by name; `api.rs` defines the writes and is
/// left out.
fn functions(sources: &[(String, String)]) -> BTreeMap<String, Vec<Def>> {
    let mut fns: BTreeMap<String, Vec<Def>> = BTreeMap::new();
    for (file, src) in sources {
        if file.ends_with("api.rs") {
            continue;
        }
        for (at, _) in src.match_indices("fn ") {
            if src[..at].bytes().next_back().is_some_and(is_ident) {
                continue;
            }
            let rest = &src[at + 3..];
            let name: String = rest
                .bytes()
                .take_while(|&c| is_ident(c))
                .map(char::from)
                .collect();
            if name.is_empty() {
                continue;
            }
            let (Some(open), semi) = (rest.find('{'), rest.find(';')) else {
                continue;
            };
            if semi.is_some_and(|s| s < open) {
                continue;
            }
            let open = at + 3 + open;
            let is_async = src[..at].trim_end().ends_with("async");
            fns.entry(name).or_default().push(Def {
                is_async,
                body: src[open..block_end(src, open)].to_owned(),
            });
        }
    }
    fns
}

/// The names every definition of which holds for `test`, grown until
/// nothing more does.
fn fixpoint(
    fns: &BTreeMap<String, Vec<Def>>,
    test: impl Fn(&Def, &BTreeSet<String>) -> bool,
) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    loop {
        let more: Vec<String> = fns
            .iter()
            .filter(|(n, defs)| !set.contains(*n) && defs.iter().all(|f| test(f, &set)))
            .map(|(n, _)| n.clone())
            .collect();
        if more.is_empty() {
            return set;
        }
        set.extend(more);
    }
}

/// The functions that write (`writers`), and the async ones that may wait
/// before they do (`waiting`), of masked `sources`.
fn writers_and_waiting(masked: &[(String, String)]) -> (BTreeSet<String>, BTreeSet<String>) {
    let fns = functions(masked);
    let writers = fixpoint(&fns, |f, w| !writes_in(&f.body, w).is_empty());
    let waiting = fixpoint(&fns, |f, wait| {
        f.is_async && !late_writes(&f.body, &writers, wait).is_empty()
    });
    (writers, waiting)
}

/// A task that may drop a write: `file`, `line`, the tasks that spawn it
/// (`ctx.tasks`), its late writes joined by "+".
type Site = (String, usize, String, String);

/// The expression a method called at `at` (a `.`) is called on: the path
/// of names and dots before it (`ctx.tasks`, `self.tasks`, `tasks`).
fn receiver(src: &str, at: usize) -> String {
    let start = src[..at]
        .bytes()
        .rposition(|c| !(is_ident(c) || c == b'.'))
        .map_or(0, |i| i + 1);
    src[start..at].to_owned()
}

/// Every task that may drop a write.
fn late_tasks(sources: &[(String, String)]) -> Vec<Site> {
    let masked: Vec<(String, String)> = sources.iter().map(|(f, s)| (f.clone(), mask(s))).collect();
    let (writers, waiting) = writers_and_waiting(&masked);
    let mut found = Vec::new();
    for (file, src) in &masked {
        for spawner in SPAWNERS {
            for (at, _) in src.match_indices(spawner) {
                let open = at + spawner.len() - 1;
                let arg = &src[open..block_end(src, open)];
                let late = match arg[1..].trim_start().starts_with("async") {
                    true => arg
                        .find('{')
                        .map(|o| late_writes(&arg[o..block_end(arg, o)], &writers, &waiting))
                        .unwrap_or_default(),
                    // `spawn(ctx.load(shell))`: the async fn's own late writes.
                    false => waiting
                        .iter()
                        .filter(|n| find_call(arg, n).is_some())
                        .cloned()
                        .collect(),
                };
                if !late.is_empty() {
                    let line = src[..at].matches('\n').count() + 1;
                    let names: Vec<String> = late.into_iter().collect();
                    found.push((file.clone(), line, receiver(src, at), names.join("+")));
                }
            }
        }
    }
    found
}

/// Problems with `found` against `kept`: a site not listed, more of one
/// than listed, or a listed one no longer there.
fn unlisted(found: &[Site], kept: &[Kept]) -> Vec<String> {
    let mut by_key: BTreeMap<(&str, &str, &str), Vec<usize>> = BTreeMap::new();
    for (file, line, recv, names) in found {
        by_key.entry((file, recv, names)).or_default().push(*line);
    }
    let listed: BTreeMap<(&str, &str, &str), usize> = kept
        .iter()
        .map(|&(f, r, n, times, _)| ((f, r, n), times))
        .collect();
    let mut problems = Vec::new();
    for (&(file, recv, names), lines) in &by_key {
        let at = lines
            .iter()
            .map(|l| format!("{file}:{l}"))
            .collect::<Vec<_>>()
            .join(", ");
        match listed.get(&(file, recv, names)) {
            None => problems.push(format!("{at}: {recv} {names} (not in KEPT)")),
            Some(&allowed) if lines.len() > allowed => problems.push(format!(
                "{at}: {recv} {names} ({} times, KEPT allows {allowed})",
                lines.len()
            )),
            Some(_) => {}
        }
    }
    for (&(file, recv, names), &allowed) in &listed {
        let times = by_key.get(&(file, recv, names)).map_or(0, Vec::len);
        if times < allowed {
            problems.push(format!(
                "{file}: {recv} {names} (KEPT says {allowed}, src has {times}: update KEPT)"
            ));
        }
    }
    problems
}

fn sources() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 50, "scanned only {} files", files.len());
    files
        .iter()
        .map(|path| {
            let rel = path
                .strip_prefix(&src)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (rel, std::fs::read_to_string(path).unwrap())
        })
        .collect()
}

#[test]
fn work_the_user_asked_for_is_not_dropped_with_its_screen() {
    let problems = unlisted(&late_tasks(&sources()), KEPT);
    assert!(
        problems.is_empty(),
        "these tasks reach a write only if their screen is still there. Send the \
         write with `Tasks::spawn_then` (the view's update in `ui`) or \
         `tasks::detached`, or list the site in KEPT (tests/task_kinds.rs) with \
         why it may stay:\n{}",
        problems.join("\n")
    );
}

/// The config saves wait for the window's turn, so a `spawn` that calls
/// them anywhere is found. A second function of the same name that does
/// not wait would take the name out of `waiting` and hide them; this
/// says so.
#[test]
fn the_config_saves_count_as_waiting() {
    let masked: Vec<(String, String)> = sources()
        .iter()
        .map(|(f, s)| (f.clone(), mask(s)))
        .collect();
    let (_, waiting) = writers_and_waiting(&masked);
    for save in ["save_config", "add_recent_dir"] {
        assert!(waiting.contains(save), "{save} no longer counts as waiting");
    }
}

fn src(file: &str, lines: &[&str]) -> (String, String) {
    (file.to_owned(), lines.join("\n"))
}

fn late(sources: &[(String, String)]) -> Vec<String> {
    late_tasks(sources)
        .into_iter()
        .map(|(f, l, r, n)| format!("{f}:{l} {r} {n}"))
        .collect()
}

#[test]
fn a_write_at_the_first_await_goes_out() {
    let s = src(
        "a.rs",
        &[
            "fn f(ctx: Ctx) {",
            "    ctx.tasks.spawn(async move {",
            "        match api::post::<_, V>(&path, &req).await {",
            "            Ok(r) => ctx.list.set(r),",
            "            Err(e) => ctx.notice.set(Some(e)),",
            "        }",
            "    });",
            "}",
        ],
    );
    assert!(late(&[s]).is_empty());
}

#[test]
fn a_write_after_another_await_is_found() {
    let s = src(
        "a.rs",
        &[
            "fn f(ctx: Ctx) {",
            "    ctx.tasks.spawn(async move {",
            "        let png = shot(ctx).await;",
            "        api::post_form::<V>(&path, png).await;",
            "    });",
            "}",
        ],
    );
    assert_eq!(late(&[s]), ["a.rs:2 ctx.tasks api::post"]);
}

#[test]
fn a_helper_that_waits_before_it_sends_is_found_anywhere() {
    let helper = src(
        "shell.rs",
        &[
            "pub async fn save_config(next: Config) {",
            "    let turn = turns.take().await;",
            "    let _ = api::put::<_, V>(\"/api/config\", &next).await;",
            "}",
            "async fn upload_all(files: Vec<File>) {",
            "    for f in files {",
            "        let _ = upload(&f).await;",
            "    }",
            "}",
            "async fn upload(f: &File) -> R { api::post_form(PATH, form(f)).await }",
        ],
    );
    let task = src(
        "a.rs",
        &[
            "fn f(shell: Shell, tasks: Tasks) {",
            "    tasks.spawn(async move { shell.save_config(next).await; });",
            "    tasks.spawn(async move { upload_all(files).await; });",
            "    tasks.spawn(async move { upload(&f).await; });",
            "    tasks.spawn(shell.save_config(next));",
            "}",
        ],
    );
    assert_eq!(
        late(&[helper, task]),
        [
            "a.rs:2 tasks save_config",
            "a.rs:3 tasks upload_all",
            "a.rs:5 tasks save_config"
        ]
    );
}

#[test]
fn a_detached_write_and_text_in_strings_or_comments_count_for_nothing() {
    let s = src(
        "a.rs",
        &[
            "fn f(tasks: Tasks) {",
            "    tasks.spawn(async move {",
            "        x().await;",
            "        detached(async move { let _ = api::delete::<V>(&p).await; });",
            "        // api::post(&p, &b).await",
            "        let s = \"api::put(\";",
            "        let c = '{';",
            "    });",
            "}",
        ],
    );
    assert!(late(&[s]).is_empty());
}

#[test]
fn a_name_defined_twice_writes_only_if_both_do() {
    // `open` of a terminal posts; `open` of a stream does not.
    let s = src(
        "a.rs",
        &[
            "fn open(p: &str) { tasks.spawn_then(api::post::<_, V>(p, &()), |_| ()); }",
            "fn open(url: &str) -> Stream { Stream::new(url) }",
            "fn f(tasks: Tasks) {",
            "    tasks.spawn(async move { x().await; open(\"/e\"); });",
            "}",
        ],
    );
    assert!(late(&[s]).is_empty());
}

#[test]
fn the_scan_catches_a_new_site_and_a_listed_one_gone() {
    let site = |line, recv: &str| {
        (
            "a.rs".to_owned(),
            line,
            recv.to_owned(),
            "save_png".to_owned(),
        )
    };
    let kept = [("a.rs", "ctx.tasks", "save_png", 1, "why")];
    assert!(unlisted(&[site(3, "ctx.tasks")], &kept).is_empty());
    assert_eq!(
        unlisted(&[site(3, "ctx.tasks"), site(9, "ctx.tasks")], &kept),
        ["a.rs:3, a.rs:9: ctx.tasks save_png (2 times, KEPT allows 1)"]
    );
    let other = [(
        "b.rs".to_owned(),
        1,
        "tasks".to_owned(),
        "upload_all".to_owned(),
    )];
    assert_eq!(
        unlisted(&other, &kept),
        [
            "b.rs:1: tasks upload_all (not in KEPT)",
            "a.rs: ctx.tasks save_png (KEPT says 1, src has 0: update KEPT)"
        ]
    );
}

/// The same write in the task of a panel that goes first (the draw
/// layer's `tasks`) is not the kept one of the Studio (`ctx.tasks`).
#[test]
fn a_kept_site_moved_to_another_owner_is_found() {
    let task = |recv: &str| {
        src(
            "a.rs",
            &[
                "fn f(ctx: Ctx, tasks: Tasks) {",
                &format!("    {recv}.spawn(async move {{"),
                "        snap(ctx).await;",
                "        api::post_form::<V>(&path, png).await;",
                "    });",
                "}",
            ],
        )
    };
    let kept = [("a.rs", "ctx.tasks", "api::post", 1, "why")];
    assert!(unlisted(&late_tasks(&[task("ctx.tasks")]), &kept).is_empty());
    assert_eq!(
        unlisted(&late_tasks(&[task("tasks")]), &kept),
        [
            "a.rs:2: tasks api::post (not in KEPT)",
            "a.rs: ctx.tasks api::post (KEPT says 1, src has 0: update KEPT)"
        ]
    );
}
