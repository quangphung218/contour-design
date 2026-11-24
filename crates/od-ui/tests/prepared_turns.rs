//! A turn a button writes (Save as skill, Finalize, Continue, a lint
//! report, a media retry, a form's answers, a Retry of one of those) is not
//! the user's draft. `StudioCtx::send` takes the composer's files, attached
//! comments and @ chips with the text and clears them once it is out; only
//! the composer's Send (and a Retry of the user's own message) may do that.
//! Every other turn goes through `StudioCtx::send_prepared`, which leaves
//! the draft where it is (#315 follow-up: four buttons still took it).
//!
//! This test finds every `.send(…)` with arguments and every `send_at(…)`
//! under `src` and checks each against [`KEPT`]: a new button that calls
//! `ctx.send(` fails here, and so does a listed site that went away.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `(file under src, enclosing fn, call, times, why it may take the draft)`.
type Kept = (
    &'static str,
    &'static str,
    &'static str,
    usize,
    &'static str,
);

const KEPT: &[Kept] = &[
    (
        "app/chat_view.rs",
        "ChatPane",
        "send",
        2,
        "the composer's Send (and Enter): the draft is what the user sends",
    ),
    (
        "app/chat_view.rs",
        "NextSteps",
        "send",
        1,
        "Retry of the user's own message (`Retry::Own`), with its files again; \
         a prepared one goes through `send_prepared`",
    ),
    (
        "app/studio.rs",
        "send",
        "send_at",
        1,
        "`StudioCtx::send` itself: the Send path",
    ),
    (
        "app/studio.rs",
        "send_prepared",
        "send_at",
        1,
        "the prepared path: always passes `shown`, so `send_at` sends bare",
    ),
    (
        "app/studio.rs",
        "send_next",
        "send_at",
        1,
        "the queue: a prepared turn keeps `shown` (`Queued::prepared`)",
    ),
    (
        "app/studio.rs",
        "load",
        "send_at",
        1,
        "the brief from Home: the files and chips it puts in the composer are \
         that brief's own (`take_pending`), sent with it",
    ),
    (
        "app/agent_w3.rs",
        "SideChatView",
        "send",
        1,
        "the side chat's own composer and request (`SideChat::send`), not the \
         Studio's draft",
    ),
];

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

/// The name of a `fn` declared on `line`, if any.
fn declared_fn(line: &str) -> Option<&str> {
    let rest = line.trim_start();
    let at = rest.find("fn ")?;
    let head = &rest[..at];
    // Only qualifiers before `fn`: `pub`, `pub(crate)`, `async`, `const`.
    if !head
        .split_whitespace()
        .all(|w| w == "pub" || w.starts_with("pub(") || w == "async" || w == "const")
    {
        return None;
    }
    let name: String = rest[at + 3..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then(|| &rest[at + 3..at + 3 + name.len()])
}

/// The calls a line makes: `send` for `.send(<args>` (not `.send()`, a
/// request's), `send_at` for `send_at(` that is not its definition.
fn calls(line: &str) -> Vec<&'static str> {
    let code = line.split("//").next().unwrap_or("");
    let mut out = Vec::new();
    for (i, _) in code.match_indices(".send(") {
        if !code[i + ".send(".len()..].starts_with(')') {
            out.push("send");
        }
    }
    for (i, _) in code.match_indices("send_at(") {
        if !code[..i].trim_end().ends_with("fn") {
            out.push("send_at");
        }
    }
    out
}

/// `(file, fn, call) → times` over every file under `src`.
fn scan(src: &Path) -> BTreeMap<(String, String, String), usize> {
    let mut files = Vec::new();
    rust_files(src, &mut files);
    let mut found = BTreeMap::new();
    for path in files {
        let rel = path
            .strip_prefix(src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let mut current = String::from("(top)");
        for line in std::fs::read_to_string(&path).unwrap().lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if let Some(name) = declared_fn(line) {
                current = name.to_owned();
            }
            for call in calls(line) {
                *found
                    .entry((rel.clone(), current.clone(), call.to_owned()))
                    .or_default() += 1;
            }
        }
    }
    found
}

#[test]
fn only_send_and_a_retry_of_the_users_message_take_the_draft() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let found = scan(&src);
    let kept: BTreeMap<_, _> = KEPT
        .iter()
        .map(|(file, f, call, n, _)| {
            (
                ((*file).to_owned(), (*f).to_owned(), (*call).to_owned()),
                *n,
            )
        })
        .collect();
    let mut wrong = Vec::new();
    for (site, n) in &found {
        match kept.get(site) {
            Some(k) if k == n => {}
            Some(k) => wrong.push(format!("{site:?}: {n} calls, KEPT lists {k}")),
            None => wrong.push(format!(
                "{site:?}: {n} call(s) not in KEPT; a prepared turn uses `send_prepared`"
            )),
        }
    }
    for site in kept.keys() {
        if !found.contains_key(site) {
            wrong.push(format!("{site:?}: in KEPT but gone; drop the entry"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_scan_sees_a_send_with_arguments_only() {
    assert_eq!(calls("ctx.send(text, shell);"), ["send"]);
    assert_eq!(
        calls("on:click=move |_| ctx.send(p.clone(), ctx.shell)>"),
        ["send"]
    );
    assert!(calls("let res = req.send().await?;").is_empty());
    assert!(calls("ctx.send_prepared(text, None, shell);").is_empty());
    assert_eq!(
        calls("self.send_at(text, shell, now, false, None, None);"),
        ["send_at"]
    );
    assert!(calls("    fn send_at(").is_empty());
    assert!(calls("x // ctx.send(text, shell)").is_empty());
    assert_eq!(
        declared_fn("    pub async fn load(self, shell: Shell) {"),
        Some("load")
    );
    assert_eq!(
        declared_fn("pub(super) fn finalize(ctx: StudioCtx) {"),
        Some("finalize")
    );
    assert_eq!(declared_fn("    let f = move |_| ctx.send(x);"), None);
}
