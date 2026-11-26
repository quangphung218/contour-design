//! The forms that edit a user's file save only over what they opened on
//! (`src/cas.rs`): the `GET` answers an `ETag`, a `PUT` with that tag as
//! `If-Match` writes, a `PUT` with an older tag is a 412 and writes
//! nothing, and a `PUT` with no `If-Match` (an agent, `od`) writes as
//! before.

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
    assert_eq!(put(&d, "/api/skills/tea", &elsewhere, None).await, 200);
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
    assert_eq!(std::fs::read_to_string(dir.join("SKILL.md")).unwrap(), theirs);
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
        put(&d, &path, &json!({ "body": "# Elsewhere\n" }), None).await,
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
    // No If-Match: written as before.
    assert_eq!(put(&d, "/api/memory/index", &late, None).await, 200);
}
