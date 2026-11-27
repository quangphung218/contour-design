//! A skill or design system whose folder in `<data>/skills` or
//! `<data>/design-systems` is the user's own symlink (the index lists it):
//! its form opens with a tag and saves over it, through the link at the
//! entry, as before `od_fs::user_store`; a link below the entry is never
//! followed (`crates/od-server/CLAUDE.md`, invariant 16). From the PR #359
//! review probes.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use common::{Daemon, Options, daemon_with};
use serde_json::{Value, json};
use std::os::unix::fs::symlink;
use std::path::Path;

async fn daemon() -> Daemon {
    daemon_with(Options {
        content_root: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")),
        ..Options::default()
    })
    .await
}

/// `GET path`: its status, body and `ETag`.
async fn tagged(d: &Daemon, path: &str) -> (u16, Value, String) {
    let res = d
        .http
        .get(format!("{}{path}", d.base))
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    let tag = res
        .headers()
        .get("etag")
        .map(|t| t.to_str().unwrap().to_owned())
        .unwrap_or_default();
    (status, res.json().await.unwrap_or(Value::Null), tag)
}

async fn send(d: &Daemon, method: reqwest::Method, path: &str, body: &Value, tag: &str) -> u16 {
    let res = d
        .http
        .request(method, format!("{}{path}", d.base))
        .header("if-match", tag)
        .json(body)
        .send()
        .await
        .unwrap();
    res.status().as_u16()
}

/// Moves `dir` out of the data folder and links it back in its place.
fn linked_back(d: &Daemon, dir: &Path) -> std::path::PathBuf {
    let away = d.data.join("elsewhere");
    std::fs::create_dir_all(&away).unwrap();
    let real = away.join(dir.file_name().unwrap());
    std::fs::rename(dir, &real).unwrap();
    symlink(&real, dir).unwrap();
    real
}

#[tokio::test]
async fn a_skill_whose_folder_is_the_users_link_is_saved_where_it_points() {
    let d = daemon().await;
    let skill = json!({ "name": "mine", "description": "Mine.", "body": "First." });
    let post = reqwest::Method::POST;
    assert_eq!(
        send(&d, post.clone(), "/api/skills", &skill, "*").await,
        200
    );
    let real = linked_back(&d, &d.data.join("skills/mine"));
    // The index reads the skills again on a write.
    let other = json!({ "name": "reload", "description": "R.", "body": "R." });
    assert_eq!(send(&d, post, "/api/skills", &other, "*").await, 200);

    let (status, shown, tag) = tagged(&d, "/api/skills/mine").await;
    assert_eq!(status, 200, "{shown}");
    let typed = json!({ "name": "mine", "description": "Mine.", "body": "Typed." });
    let put = reqwest::Method::PUT;
    assert_eq!(
        send(&d, put.clone(), "/api/skills/mine", &typed, &tag).await,
        200
    );
    assert!(
        std::fs::read_to_string(real.join("SKILL.md"))
            .unwrap()
            .contains("Typed.")
    );
    // The tag of the next GET is the next Save's, again and again.
    let (_, _, tag) = tagged(&d, "/api/skills/mine").await;
    let again = json!({ "name": "mine", "description": "Mine.", "body": "Again." });
    assert_eq!(
        send(&d, put.clone(), "/api/skills/mine", &again, &tag).await,
        200
    );
    assert_eq!(
        send(&d, put.clone(), "/api/skills/mine", &typed, "*").await,
        200
    );
    assert!(
        std::fs::symlink_metadata(d.data.join("skills/mine"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the user's link stays"
    );

    // Below the entry, a `SKILL.md` that is a link is replaced, never
    // written through.
    let victim = d.data.join("victim.md");
    std::fs::write(&victim, "the user's own").unwrap();
    std::fs::remove_file(real.join("SKILL.md")).unwrap();
    symlink(&victim, real.join("SKILL.md")).unwrap();
    assert_eq!(send(&d, put, "/api/skills/mine", &typed, "*").await, 200);
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "the user's own");
    assert!(
        std::fs::symlink_metadata(real.join("SKILL.md"))
            .unwrap()
            .is_file()
    );
}

#[tokio::test]
async fn a_design_system_whose_folder_is_the_users_link_saves_its_design_md() {
    let d = daemon().await;
    let res = d
        .http
        .post(format!("{}/api/design-systems/airbnb/copy", d.base))
        .json(&json!({ "name": "Mine" }))
        .send()
        .await
        .unwrap();
    let v: Value = res.json().await.unwrap();
    let id = v["id"].as_str().unwrap().to_owned();
    // Its folder is named by the slug of the name, its id may differ.
    let real = linked_back(&d, &d.data.join("design-systems/mine"));
    std::fs::write(real.join("DESIGN.md"), "# On disk\n").unwrap();

    let kit = format!("/api/design-systems/{id}/kit");
    let (status, shown, tag) = tagged(&d, &kit).await;
    assert_eq!(status, 200, "{shown}");
    assert_eq!(shown["body"], "# On disk\n", "the kit shows the file");
    let save = format!("/api/design-systems/{id}/design-md");
    let put = reqwest::Method::PUT;
    let typed = json!({ "body": "# Typed\n" });
    assert_eq!(send(&d, put.clone(), &save, &typed, &tag).await, 200);
    assert_eq!(
        std::fs::read_to_string(real.join("DESIGN.md")).unwrap(),
        "# Typed\n"
    );
    let (_, _, tag) = tagged(&d, &kit).await;
    assert_eq!(
        send(
            &d,
            put.clone(),
            &save,
            &json!({ "body": "# Again\n" }),
            &tag
        )
        .await,
        200
    );

    // Below the entry: a DESIGN.md that is a link is replaced, never
    // written through.
    let victim = d.data.join("victim.md");
    std::fs::write(&victim, "the user's own").unwrap();
    std::fs::remove_file(real.join("DESIGN.md")).unwrap();
    symlink(&victim, real.join("DESIGN.md")).unwrap();
    assert_eq!(send(&d, put, &save, &typed, "*").await, 200);
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "the user's own");
    assert!(
        std::fs::symlink_metadata(real.join("DESIGN.md"))
            .unwrap()
            .is_file()
    );
}
