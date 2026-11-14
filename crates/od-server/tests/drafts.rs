//! Images uploaded for drafts (a comment not sent yet) over HTTP: kept out
//! of what ships until a comment takes them, deleted when their page's
//! session is released, even when they land after it; comments sent
//! follow their file renamed or moved.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use common::{Daemon, daemon};
use serde_json::{Value, json};

const BOUNDARY: &str = "XDRAFTBOUNDARY";

/// Uploads `name` into `comments/`, for draft session `draft` if any.
async fn upload(d: &Daemon, project: &str, name: &str, draft: Option<&str>) -> Value {
    let mut body =
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"dir\"\r\n\r\ncomments\r\n");
    if let Some(s) = draft {
        body.push_str(&format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"draft\"\r\n\r\n{s}\r\n"
        ));
    }
    body.push_str(&format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: image/png\r\n\r\nPNGDATA\r\n--{BOUNDARY}--\r\n"
    ));
    let res = d
        .http
        .post(format!("{}/api/projects/{project}/upload", d.base))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    res.json().await.unwrap()
}

async fn files(d: &Daemon, project: &str) -> Vec<String> {
    let v = d.get_json(&format!("/api/projects/{project}/files")).await;
    v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_owned())
        .collect()
}

async fn zip_names(d: &Daemon, project: &str) -> Vec<String> {
    let bytes = d
        .http
        .get(format!("{}/api/projects/{project}/archive", d.base))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let mut names: Vec<String> = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..]))
        .unwrap()
        .file_names()
        .map(|n| n.unwrap().into_owned())
        .collect();
    names.sort();
    names
}

async fn release(d: &Daemon, project: &str, session: &str) -> u16 {
    d.post(
        &format!("/api/projects/{project}/drafts/{session}/release"),
        &json!({}),
    )
    .await
    .status()
    .as_u16()
}

async fn comment(
    d: &Daemon,
    project: &str,
    conversation: &str,
    file: &str,
    images: &[&str],
) -> u16 {
    d.post(
        &format!("/api/projects/{project}/conversations/{conversation}/comments"),
        &json!({"filePath": file, "selector": "h1", "note": "Like this", "attachments": images}),
    )
    .await
    .status()
    .as_u16()
}

#[tokio::test]
async fn a_draft_image_ships_only_once_a_comment_takes_it() {
    let d = daemon().await;
    let (project, conversation) = d.project().await;
    let res = d
        .post(
            &format!("/api/projects/{project}/files"),
            &json!({"name": "index.html", "content": "<h1>A</h1>"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200);
    let up = upload(&d, &project, "ref.png", Some("s1")).await;
    let img = up["files"][0]["path"].as_str().unwrap().to_owned();
    assert!(
        files(&d, &project).await.contains(&img),
        "listed for the draft"
    );
    assert!(
        !zip_names(&d, &project).await.contains(&img),
        "not in the ZIP while a draft's"
    );
    let assets = d
        .get_json(&format!("/api/library/assets?projectId={project}"))
        .await;
    assert!(
        !assets.to_string().contains(&img),
        "not a Library asset: {assets}"
    );
    let selection = d
        .post(
            &format!("/api/projects/{project}/archive/selection"),
            &json!({"files": ["index.html", img]}),
        )
        .await;
    assert_eq!(selection.status().as_u16(), 200);
    let bytes = selection.bytes().await.unwrap();
    let zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes[..])).unwrap();
    assert!(
        zip.file_names().all(|n| n.unwrap() != img),
        "not in a selection ZIP"
    );
    let dup = d
        .post(&format!("/api/projects/{project}/duplicate"), &json!({}))
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert!(
        !dup["copiedFiles"].to_string().contains(&img),
        "not copied by Duplicate: {dup}"
    );

    assert_eq!(
        comment(&d, &project, &conversation, "index.html", &[&img]).await,
        200
    );
    assert!(
        zip_names(&d, &project).await.contains(&img),
        "the comment's now"
    );
    assert_eq!(release(&d, &project, "s1").await, 200);
    assert!(
        files(&d, &project).await.contains(&img),
        "a release leaves it"
    );
}

#[tokio::test]
async fn a_release_deletes_the_session_images_and_any_landing_later() {
    let d = daemon().await;
    let (project, _) = d.project().await;
    let mine = upload(&d, &project, "mine.png", Some("s1")).await["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let theirs = upload(&d, &project, "theirs.png", Some("s2")).await["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let plain = upload(&d, &project, "plain.png", None).await["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(release(&d, &project, "s1").await, 200);
    let listed = files(&d, &project).await;
    assert!(!listed.contains(&mine), "{listed:?}");
    assert!(listed.contains(&theirs), "another page's draft stays");
    assert!(listed.contains(&plain), "an upload of the user's own stays");
    // Upload held past the release: deleted at once, not listed.
    let late = upload(&d, &project, "late.png", Some("s1")).await;
    assert_eq!(late["files"], json!([]));
    assert!(
        files(&d, &project)
            .await
            .iter()
            .all(|f| !f.contains("late")),
        "the late image is gone"
    );
    assert_eq!(
        release(&d, &project, "a%20b").await,
        400,
        "a bad session id"
    );
}

#[tokio::test]
async fn a_comment_sent_follows_its_file_renamed_or_moved() {
    let d = daemon().await;
    let (project, conversation) = d.project().await;
    let list = format!("/api/projects/{project}/conversations/{conversation}/comments");
    let res = d
        .post(
            &format!("/api/projects/{project}/files"),
            &json!({"name": "a.html", "content": "<h1>A</h1>"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(
        comment(&d, &project, &conversation, "a.html", &[]).await,
        200
    );
    let res = d
        .post(
            &format!("/api/projects/{project}/files/rename"),
            &json!({"from": "a.html", "to": "c.html"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200);
    let path = |v: &Value| v["comments"][0]["filePath"].as_str().unwrap().to_owned();
    assert_eq!(path(&d.get_json(&list).await), "c.html", "its pin follows");
    let res = d
        .post(
            &format!("/api/projects/{project}/files/rename"),
            &json!({"from": "c.html", "to": "pages/c.html"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200);
    let res = d
        .post(
            &format!("/api/projects/{project}/move-folder"),
            &json!({"from": "pages", "to": "site"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(path(&d.get_json(&list).await), "site/c.html");
}

#[tokio::test]
async fn opening_a_project_sweeps_draft_images_a_day_old() {
    let d = daemon().await;
    let (project, _) = d.project().await;
    let old = upload(&d, &project, "old.png", Some("s1")).await["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let fresh = upload(&d, &project, "fresh.png", Some("s2")).await["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    // A page that crashed a day ago never released `s1`.
    d.state
        .store
        .with_conn(|c| {
            c.execute(
                "UPDATE draft_uploads SET uploaded_at = uploaded_at - ?1 WHERE path = ?2",
                rusqlite::params![od_store::drafts::DRAFT_MAX_AGE_MS + 1, old],
            )?;
            Ok(())
        })
        .unwrap();
    let full = d.state.store.stat_file(&project, &old).unwrap().1;
    let then = std::time::SystemTime::now()
        - std::time::Duration::from_millis(
            u64::try_from(od_store::drafts::DRAFT_MAX_AGE_MS).unwrap() + 1000,
        );
    std::fs::File::options()
        .write(true)
        .open(full)
        .and_then(|f| f.set_modified(then))
        .unwrap();
    d.get_json(&format!("/api/projects/{project}")).await;
    let listed = files(&d, &project).await;
    assert!(!listed.contains(&old), "{listed:?}");
    assert!(listed.contains(&fresh), "another page's draft of an hour");
}

/// Writes `<img>` into the page with `src`, taking `claim` (a Manual edit
/// image applied).
async fn write_src(d: &Daemon, project: &str, src: &str, claim: &[&str]) -> u16 {
    d.post(
        &format!("/api/projects/{project}/source-edits"),
        &json!({
            "file": "index.html",
            "edits": [{"selector": "html > body:nth-of-type(1) > img:nth-of-type(1)",
                       "expectTag": "img", "attributes": {"src": src}}],
            "label": "Manual edit",
            "claim": claim,
        }),
    )
    .await
    .status()
    .as_u16()
}

#[tokio::test]
async fn a_manual_write_takes_its_draft_image_in_the_same_step() {
    let d = daemon().await;
    let (project, _) = d.project().await;
    let res = d
        .post(
            &format!("/api/projects/{project}/files"),
            &json!({"name": "index.html", "content": "<!doctype html><html><body><img src=\"x.png\"></body></html>"}),
        )
        .await;
    assert_eq!(res.status().as_u16(), 200);
    let img = upload(&d, &project, "hero.png", Some("s1")).await["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(write_src(&d, &project, &img, &[&img]).await, 200);
    // The release after the write: the page's image stays and ships.
    assert_eq!(release(&d, &project, "s1").await, 200);
    assert!(files(&d, &project).await.contains(&img));
    assert!(zip_names(&d, &project).await.contains(&img));

    // Released before the write: the write is refused, the page untouched.
    let late = upload(&d, &project, "late.png", Some("s2")).await["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(release(&d, &project, "s2").await, 200);
    assert_eq!(write_src(&d, &project, &late, &[&late]).await, 404);
    let page = d
        .http
        .get(format!("{}/api/projects/{project}/raw/index.html", d.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(page.contains(&img) && !page.contains(&late), "{page}");

    // A write that fails gives its image back to the draft.
    let other = upload(&d, &project, "other.png", Some("s3")).await["files"][0]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let res = d
        .post(
            &format!("/api/projects/{project}/source-edits"),
            &json!({"file": "index.html", "edits": [{"selector": "h9", "attributes": {"src": other}}],
                    "claim": [other]}),
        )
        .await;
    assert!(res.status().as_u16() >= 400);
    assert!(
        !zip_names(&d, &project).await.contains(&other),
        "still a draft's"
    );
}
