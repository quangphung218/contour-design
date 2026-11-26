//! The forms that edit a user's file save only over what they opened on
//! (`src/cas.rs`): the `GET` answers an `ETag`, a `PUT` with that tag as
//! `If-Match` writes, a `PUT` with an older tag is a 412 and writes
//! nothing, a `PUT` with no `If-Match` is a 428 and writes nothing (a form
//! with no tag yet never writes blind), and `If-Match: *` writes over
//! whatever is there ("Save anyway", an agent that means it).

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use common::{Daemon, Options, daemon_with};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn content_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
}

async fn daemon() -> Daemon {
    daemon_with(Options {
        content_root: Some(content_root()),
        ..Options::default()
    })
    .await
}

/// `GET path`: the body and its `ETag`.
async fn tagged(d: &Daemon, path: &str) -> (Value, String) {
    let res = d
        .http
        .get(format!("{}{path}", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200, "{path}");
    let tag = res
        .headers()
        .get("etag")
        .unwrap_or_else(|| panic!("{path} answers no ETag"))
        .to_str()
        .unwrap()
        .to_owned();
    (res.json().await.unwrap(), tag)
}

/// `PUT path` with `body`, and `If-Match: tag` when given.
async fn put(d: &Daemon, path: &str, body: &Value, tag: Option<&str>) -> u16 {
    let mut req = d.http.put(format!("{}{path}", d.base)).json(body);
    if let Some(tag) = tag {
        req = req.header("if-match", tag);
    }
    req.send().await.unwrap().status().as_u16()
}

async fn post(d: &Daemon, path: &str, body: &Value) -> Value {
    let res = d
        .http
        .post(format!("{}{path}", d.base))
        .json(body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200, "{path}");
    res.json().await.unwrap()
}

#[tokio::test]
async fn a_skill_saved_from_an_old_copy_writes_nothing() {
    let d = daemon().await;
    post(
        &d,
        "/api/skills",
        &json!({ "name": "tea", "description": "Mine.", "body": "First." }),
    )
    .await;
    let file = d.data.join("skills/tea/SKILL.md");
    let (_, opened) = tagged(&d, "/api/skills/tea").await;
    // Changed under the form (an agent, another window).
    let elsewhere = json!({ "name": "tea", "description": "Mine.", "body": "Elsewhere." });
    assert_eq!(put(&d, "/api/skills/tea", &elsewhere, Some("*")).await, 200);
    let before = std::fs::read(&file).unwrap();
    let typed = json!({ "name": "tea", "description": "Mine.", "body": "Typed." });
    assert_eq!(put(&d, "/api/skills/tea", &typed, Some(&opened)).await, 412);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "a 412 writes nothing"
    );
    // On the copy as it is now: written; its tag moves on.
    let (now, tag) = tagged(&d, "/api/skills/tea").await;
    assert!(now["body"].as_str().unwrap().contains("Elsewhere."));
    assert_ne!(tag, opened);
    assert_eq!(put(&d, "/api/skills/tea", &typed, Some(&tag)).await, 200);
    assert!(std::fs::read_to_string(&file).unwrap().contains("Typed."));
    assert_eq!(
        put(&d, "/api/skills/tea", &elsewhere, Some(&tag)).await,
        412
    );
    // A weak tag and `*` read as theirs and as none.
    let (_, tag) = tagged(&d, "/api/skills/tea").await;
    assert_eq!(
        put(&d, "/api/skills/tea", &typed, Some(&format!("W/{tag}"))).await,
        200
    );
    assert_eq!(put(&d, "/api/skills/tea", &elsewhere, Some("*")).await, 200);
}

#[tokio::test]
async fn a_skill_replaced_on_disk_refuses_the_form_opened_before() {
    let d = daemon().await;
    post(
        &d,
        "/api/skills",
        &json!({ "name": "tea", "description": "Mine.", "body": "First." }),
    )
    .await;
    let (_, opened) = tagged(&d, "/api/skills/tea").await;
    // Written on disk without the daemon (an agent's shell): the tag is
    // the file's, not the index's.
    let file = d.data.join("skills/tea/SKILL.md");
    std::fs::write(&file, "---\nname: tea\n---\nOn disk.\n").unwrap();
    let typed = json!({ "name": "tea", "body": "Typed." });
    assert_eq!(put(&d, "/api/skills/tea", &typed, Some(&opened)).await, 412);
    assert!(std::fs::read_to_string(&file).unwrap().contains("On disk."));
    // Opened again, the form shows the file as it is, with its tag.
    let (now, tag) = tagged(&d, "/api/skills/tea").await;
    assert!(now["body"].as_str().unwrap().contains("On disk."), "{now}");
    assert_eq!(put(&d, "/api/skills/tea", &typed, Some(&tag)).await, 200);
    assert!(std::fs::read_to_string(&file).unwrap().contains("Typed."));
}

#[tokio::test]
async fn a_bundled_skill_saved_from_an_old_copy_keeps_the_override_made_since() {
    let d = daemon().await;
    let list = d.get_json("/api/skills").await;
    let id = list["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["source"] != "user" && !s["id"].as_str().unwrap().contains(':'))
        .map(|s| s["id"].as_str().unwrap().to_owned())
        .unwrap();
    let path = format!("/api/skills/{id}");
    let (_, opened) = tagged(&d, &path).await;
    // An override written since (another window): the old form's tag no
    // longer holds.
    let other = json!({ "name": id, "body": "Other window." });
    assert_eq!(put(&d, &path, &other, Some(&opened)).await, 200);
    let typed = json!({ "name": id, "body": "Typed." });
    assert_eq!(put(&d, &path, &typed, Some(&opened)).await, 412);
    let file = d.data.join(format!("skills/{id}/SKILL.md"));
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("Other window.")
    );
}

#[tokio::test]
async fn a_bundled_skill_saved_over_an_override_written_on_disk_writes_nothing() {
    let d = daemon().await;
    let list = d.get_json("/api/skills").await;
    let id = list["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["source"] != "user" && !s["id"].as_str().unwrap().contains(':'))
        .map(|s| s["id"].as_str().unwrap().to_owned())
        .unwrap();
    let path = format!("/api/skills/{id}");
    let (_, opened) = tagged(&d, &path).await;
    // An agent's shell writes an override of it, the daemon not told: the
    // write would go over that file, so that file is what is compared.
    let dir = d.data.join(format!("skills/{id}"));
    std::fs::create_dir_all(&dir).unwrap();
    let theirs = format!("---\nname: {id}\n---\nThe agent's override.\n");
    std::fs::write(dir.join("SKILL.md"), &theirs).unwrap();
    let typed = json!({ "name": id, "body": "Typed." });
    assert_eq!(put(&d, &path, &typed, Some(&opened)).await, 412);
    assert_eq!(
        std::fs::read_to_string(dir.join("SKILL.md")).unwrap(),
        theirs
    );
    // "Reload": the form opens on the agent's override, with the tag the
    // next Save compares, and that Save writes.
    let (now, tag) = tagged(&d, &path).await;
    assert!(
        now["body"]
            .as_str()
            .unwrap()
            .contains("The agent's override."),
        "Reload shows the override: {now}"
    );
    assert_ne!(tag, opened);
    assert_eq!(put(&d, &path, &typed, Some(&tag)).await, 200);
    assert!(
        std::fs::read_to_string(dir.join("SKILL.md"))
            .unwrap()
            .contains("Typed.")
    );
}

#[tokio::test]
async fn a_design_md_saved_from_an_old_copy_writes_nothing() {
    let d = daemon().await;
    let v = post(
        &d,
        "/api/design-systems/airbnb/copy",
        &json!({ "name": "Mine" }),
    )
    .await;
    let id = v["id"].as_str().unwrap().to_owned();
    let kit = format!("/api/design-systems/{id}/kit");
    let path = format!("/api/design-systems/{id}/design-md");
    let (shown, opened) = tagged(&d, &kit).await;
    assert!(!shown["body"].as_str().unwrap().is_empty());
    assert_eq!(
        put(&d, &path, &json!({ "body": "# Elsewhere\n" }), Some("*")).await,
        200
    );
    let typed = json!({ "body": "# Typed\n" });
    assert_eq!(put(&d, &path, &typed, Some(&opened)).await, 412);
    let (now, tag) = tagged(&d, &kit).await;
    assert_eq!(now["body"], "# Elsewhere\n", "a 412 writes nothing");
    assert_eq!(put(&d, &path, &typed, Some(&tag)).await, 200);
    let (now, _) = tagged(&d, &kit).await;
    assert_eq!(now["body"], "# Typed\n");
    assert_eq!(
        put(&d, &path, &json!({ "body": "# Late\n" }), Some(&tag)).await,
        412
    );
}

#[tokio::test]
async fn a_memory_index_saved_from_an_old_copy_writes_nothing() {
    let d = daemon().await;
    post(
        &d,
        "/api/memory",
        &json!({ "kind": "user", "text": "Brand colour is teal" }),
    )
    .await;
    let (shown, opened) = tagged(&d, "/api/memory/index").await;
    let file = d.data.join("memory/MEMORY.md");
    // With no saved index, a memory learnt since changes the index shown:
    // the editor's copy is old.
    post(
        &d,
        "/api/memory",
        &json!({ "kind": "rule", "text": "Headings are serif" }),
    )
    .await;
    let typed = json!({ "index": "# Mine\n" });
    assert_eq!(
        put(&d, "/api/memory/index", &typed, Some(&opened)).await,
        412
    );
    assert!(!file.exists(), "a 412 writes nothing");
    let (now, tag) = tagged(&d, "/api/memory/index").await;
    assert_ne!(now["index"], shown["index"]);
    assert_eq!(put(&d, "/api/memory/index", &typed, Some(&tag)).await, 200);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "# Mine\n");
    // A saved index: a memory learnt links itself in, so the tag moves.
    post(
        &d,
        "/api/memory",
        &json!({ "kind": "feedback", "text": "Short copy" }),
    )
    .await;
    let late = json!({ "index": "# Late\n" });
    assert_eq!(put(&d, "/api/memory/index", &late, Some(&tag)).await, 412);
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("Short copy")
    );
    // `*`: written over whatever is there.
    assert_eq!(put(&d, "/api/memory/index", &late, Some("*")).await, 200);
}

/// 24 Saves from the same opened copy at once, 40 times: exactly one
/// lands each time (each body is new: a save of the same bytes keeps the
/// tag, so a second one would rightly land too) (the compare and the write are one step under the
/// file's lock; without it two or three land within a few rounds).
async fn one_of_many_lands(
    d: &Daemon,
    get: &str,
    put_path: &str,
    body: impl Fn(usize, usize) -> Value,
) {
    for round in 0..40 {
        let (_, tag) = tagged(d, get).await;
        let mut set = tokio::task::JoinSet::new();
        for i in 0..24 {
            let req = d
                .http
                .put(format!("{}{put_path}", d.base))
                .header("if-match", tag.clone())
                .json(&body(round, i));
            set.spawn(async move { req.send().await.unwrap().status().as_u16() });
        }
        let mut landed = 0;
        while let Some(status) = set.join_next().await {
            if status.unwrap() == 200 {
                landed += 1;
            }
        }
        assert_eq!(
            landed, 1,
            "{put_path}, round {round}: {landed} saves landed"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn skill_saves_at_once_from_one_copy_land_once() {
    let d = daemon().await;
    post(
        &d,
        "/api/skills",
        &json!({ "name": "tea", "description": "Mine.", "body": "First." }),
    )
    .await;
    one_of_many_lands(
        &d,
        "/api/skills/tea",
        "/api/skills/tea",
        |r, i| json!({ "name": "tea", "body": format!("Round {r}, writer {i}.") }),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn design_md_saves_at_once_from_one_copy_land_once() {
    let d = daemon().await;
    let v = post(
        &d,
        "/api/design-systems/airbnb/copy",
        &json!({ "name": "Mine" }),
    )
    .await;
    let id = v["id"].as_str().unwrap().to_owned();
    let kit = format!("/api/design-systems/{id}/kit");
    let path = format!("/api/design-systems/{id}/design-md");
    one_of_many_lands(
        &d,
        &kit,
        &path,
        |r, i| json!({ "body": format!("# Round {r}, writer {i}\n") }),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn memory_index_saves_at_once_from_one_copy_land_once() {
    let d = daemon().await;
    post(
        &d,
        "/api/memory",
        &json!({ "kind": "user", "text": "Brand colour is teal" }),
    )
    .await;
    one_of_many_lands(
        &d,
        "/api/memory/index",
        "/api/memory/index",
        |r, i| json!({ "index": format!("# Round {r}, writer {i}\n") }),
    )
    .await;
}

/// No `If-Match` at all (a form whose read is late or failed, a client
/// that forgot it): 428 on every route that writes a user's file, nothing
/// written; `*` writes.
#[tokio::test]
async fn a_put_without_if_match_writes_nothing() {
    let d = daemon().await;
    post(
        &d,
        "/api/skills",
        &json!({ "name": "tea", "description": "Mine.", "body": "First." }),
    )
    .await;
    let v = post(
        &d,
        "/api/design-systems/airbnb/copy",
        &json!({ "name": "Mine" }),
    )
    .await;
    let ds = v["id"].as_str().unwrap().to_owned();
    let skill = d.data.join("skills/tea/SKILL.md");
    let design = d.data.join("design-systems/mine/DESIGN.md");
    let index = d.data.join("memory/MEMORY.md");
    let cases = [
        (
            "/api/skills/tea".to_owned(),
            json!({ "name": "tea", "body": "Blind." }),
            skill,
        ),
        (
            format!("/api/design-systems/{ds}/design-md"),
            json!({ "body": "# Blind\n" }),
            design,
        ),
        (
            "/api/memory/index".to_owned(),
            json!({ "index": "# Blind\n" }),
            index,
        ),
    ];
    for (path, body, file) in &cases {
        let before = std::fs::read(file).ok();
        assert_eq!(put(&d, path, body, None).await, 428, "{path}");
        assert_eq!(
            std::fs::read(file).ok(),
            before,
            "{path}: a 428 writes nothing"
        );
        assert_eq!(put(&d, path, body, Some("*")).await, 200, "{path}");
        assert!(
            std::fs::read_to_string(file).unwrap().contains("Blind"),
            "{path}"
        );
    }
}

/// An agent writes `SKILL.md` again and again (a new file renamed over
/// it) while the form opens the skill: the `ETag` is always of the bytes
/// the body shown came from, never of a later version, so a Save with it
/// cannot pass over a version the user never saw.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_skill_read_while_it_is_written_is_tagged_with_what_it_shows() {
    let d = daemon().await;
    post(
        &d,
        "/api/skills",
        &json!({ "name": "tea", "description": "Mine.", "body": "Body-0" }),
    )
    .await;
    let dir = d.data.join("skills/tea");
    let version = |n: u64| format!("---\nname: tea\ndescription: Mine.\n---\nBody-{n}\n");
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let written = std::sync::Arc::new(AtomicU64::new(0));
    let writer = {
        let (dir, stop, written) = (
            dir.clone(),
            std::sync::Arc::clone(&stop),
            std::sync::Arc::clone(&written),
        );
        std::thread::spawn(move || {
            let mut n = 1u64;
            while !stop.load(Ordering::Relaxed) {
                let tmp = dir.join(".SKILL.md.tmp");
                std::fs::write(&tmp, version(n)).unwrap();
                // Windows refuses a rename over a file being read: the
                // writer tries again, as an agent's editor would.
                if std::fs::rename(&tmp, dir.join("SKILL.md")).is_ok() {
                    written.store(n, Ordering::Relaxed);
                    n += 1;
                }
            }
        })
    };
    // Writes are slow on some runners (Windows): reads start once the
    // writer is under way, and go on until enough saw its versions.
    let started = std::time::Instant::now();
    while written.load(Ordering::Relaxed) < 3
        && started.elapsed() < std::time::Duration::from_secs(20)
    {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let mut checked = 0;
    for _ in 0..300 {
        if checked >= 15 {
            break;
        }
        let (shown, tag) = tagged(&d, "/api/skills/tea").await;
        let body = shown["body"].as_str().unwrap();
        let n: u64 = body
            .trim()
            .rsplit("Body-")
            .next()
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("no version in {body:?}"));
        let expected = format!("\"{}\"", od_server::cas::tag_of(version(n).as_bytes()));
        if n > 0 {
            assert_eq!(tag, expected, "the tag of Body-{n}, the version shown");
            checked += 1;
        }
    }
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    assert!(
        checked > 0,
        "no read saw the writer's versions ({} written)",
        written.load(Ordering::Relaxed)
    );
}

/// An agent's shell writes `SKILL.md` in place (`>`: cut to nothing, then
/// filled) again and again while the form opens the skill (PR #349
/// review): a `GET` then finds the file changed, and reads it again for
/// its own answer, tagged with what it shows, but never puts that read in
/// the index others read (a run's prompt): it may be of part of a write.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_skill_read_while_written_in_place_leaves_the_index_whole() {
    let d = daemon().await;
    post(
        &d,
        "/api/skills",
        &json!({ "name": "tea", "description": "Mine.", "body": "Body-0" }),
    )
    .await;
    let indexed = || {
        d.state
            .content
            .snapshot()
            .find_skill("tea")
            .map(|e| e.body.clone())
    };
    let before = indexed();
    let file = d.data.join("skills/tea/SKILL.md");
    // Large enough that a read lands half way through a write.
    let filler = "Steep the leaves. ".repeat(12_000);
    let version = move |n: u64| {
        format!("---\nname: tea\ndescription: Mine.\n---\nBody-{n}\n{filler}\nEnd-{n}\n")
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let writer = {
        let (file, stop) = (file.clone(), std::sync::Arc::clone(&stop));
        std::thread::spawn(move || {
            let mut n = 1u64;
            while !stop.load(Ordering::Relaxed) && n < 5_000 {
                // In place, as `>` does; Windows may refuse while read.
                let _ = std::fs::write(&file, version(n));
                n += 1;
            }
        })
    };
    for _ in 0..200 {
        let res = d
            .http
            .get(format!("{}/api/skills/tea", d.base))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status().as_u16(), 200);
        assert!(res.headers().contains_key("etag"));
        assert_eq!(indexed(), before, "a GET put what it read in the index");
    }
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
}

/// A user skill `Ad-Creative` beside the bundled `ad-creative` (PR #349
/// review): the form of `ad-creative` shows and tags the bundled copy, and
/// its Save is refused (409) instead of writing over the other skill,
/// which on macOS and Windows is the folder `skills/ad-creative`.
#[tokio::test]
async fn a_bundled_skill_saved_beside_one_named_alike_writes_nothing() {
    let d = daemon().await;
    let twin = d.data.join("skills/Ad-Creative");
    std::fs::create_dir_all(&twin).unwrap();
    let mine = "---\nname: Ad-Creative\ndescription: Mine.\n---\nMY OWN SKILL\n";
    std::fs::write(twin.join("SKILL.md"), mine).unwrap();
    let (shown, tag) = tagged(&d, "/api/skills/ad-creative").await;
    assert!(
        !shown["body"].as_str().unwrap().contains("MY OWN SKILL"),
        "the bundled copy is shown"
    );
    let bundled = std::fs::read(content_root().join("skills/ad-creative/SKILL.md")).unwrap();
    assert_eq!(
        tag,
        format!("\"{}\"", od_server::cas::tag_of(&bundled)),
        "tagged as the copy shown"
    );
    let status = put(
        &d,
        "/api/skills/ad-creative",
        &json!({ "name": "ad-creative", "body": "Typed." }),
        Some(&tag),
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(std::fs::read_to_string(twin.join("SKILL.md")).unwrap(), mine);
}
