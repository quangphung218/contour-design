//! Track B1-library routes against a real daemon on loopback: skill CRUD
//! and overrides, plugin install/use/uninstall and project-bound plugins,
//! saved templates and Remix, the media library, and design-system kit,
//! import, copy, status, assets, archive, token rebuild and revisions
//! (mock agent). No request leaves loopback.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design
#![cfg_attr(not(unix), allow(dead_code, unused_imports))] // helpers of the `sh` mock tests

mod common;

use common::{Daemon, Options, PROBES, daemon_with, write_mock};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

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

async fn call(d: &Daemon, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let url = format!("{}{path}", d.base);
    let req = match method {
        "GET" => d.http.get(url),
        "PUT" => d.http.put(url),
        "DELETE" => d.http.delete(url),
        _ => d.http.post(url),
    };
    let req = match body {
        Some(b) => req.json(&b),
        None => req,
    };
    let res = req.send().await.unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

/// A `multipart/form-data` body with one `file` part per entry.
fn multipart(files: &[(&str, &[u8])]) -> (String, Vec<u8>) {
    let boundary = "odtestboundary42";
    let mut body = Vec::new();
    for (name, bytes) in files {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

async fn upload(d: &Daemon, path: &str, files: &[(&str, &[u8])]) -> (u16, Value) {
    let (ct, body) = multipart(files);
    let res = d
        .http
        .post(format!("{}{path}", d.base))
        .header("content-type", ct)
        .body(body)
        .send()
        .await
        .unwrap();
    (res.status().as_u16(), res.json().await.unwrap())
}

async fn new_project(d: &Daemon, name: &str) -> String {
    let (s, v) = call(d, "POST", "/api/projects", Some(json!({ "name": name }))).await;
    assert_eq!(s, 200, "{v}");
    v["project"]["id"].as_str().unwrap().to_owned()
}

async fn write_file(d: &Daemon, pid: &str, name: &str, content: &str) {
    let (s, v) = call(
        d,
        "POST",
        &format!("/api/projects/{pid}/files"),
        Some(json!({ "name": name, "content": content })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
}

fn skill<'a>(list: &'a Value, id: &str) -> Option<&'a Value> {
    list["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id)
}

#[tokio::test]
async fn skills_create_edit_override_delete() {
    let d = daemon().await;
    let body = json!({
        "name": "Brand Voice",
        "description": "Warm, short sentences.",
        "triggers": ["voice", "tone"],
        "body": "Write like a friendly barista.",
    });
    let (s, v) = call(&d, "POST", "/api/skills", Some(body.clone())).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["id"], "brand-voice");
    let list = d.get_json("/api/skills").await;
    let mine = skill(&list, "brand-voice").unwrap();
    assert_eq!(mine["source"], "user");
    assert_eq!(mine["description"], "Warm, short sentences.");
    let (s, _) = call(&d, "POST", "/api/skills", Some(body)).await;
    assert_eq!(s, 409, "a second skill with the same name");

    // Editing a user skill rewrites it in place.
    let (s, v) = call(
        &d,
        "PUT",
        "/api/skills/brand-voice",
        Some(json!({ "description": "Calm.", "body": "Be calm." })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["override"], false);
    let detail = d.get_json("/api/skills/brand-voice").await;
    assert!(detail["body"].as_str().unwrap().contains("Be calm."));

    // A bundled skill gets a user override that keeps its metadata.
    let list = d.get_json("/api/skills").await;
    let bundled = list["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["source"] == "built-in" && !s["id"].as_str().unwrap().contains(':'))
        .unwrap()
        .clone();
    let id = bundled["id"].as_str().unwrap();
    let (s, v) = call(
        &d,
        "PUT",
        &format!("/api/skills/{id}"),
        Some(json!({ "description": "My override.", "body": "Override body." })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["override"], true);
    let list = d.get_json("/api/skills").await;
    let over = skill(&list, id).unwrap();
    assert_eq!(over["source"], "user");
    assert_eq!(over["description"], "My override.");
    assert_eq!(over["mode"], bundled["mode"], "metadata is kept");
    assert!(d.data.join("skills").join(id).join("SKILL.md").is_file());

    // Deleting the override brings the bundled skill back; bundled skills
    // themselves cannot be deleted.
    let (s, _) = call(&d, "DELETE", &format!("/api/skills/{id}"), None).await;
    assert_eq!(s, 200);
    let list = d.get_json("/api/skills").await;
    assert_eq!(skill(&list, id).unwrap()["source"], "built-in");
    let (s, _) = call(&d, "DELETE", &format!("/api/skills/{id}"), None).await;
    assert_eq!(s, 403);
    let (s, _) = call(&d, "DELETE", "/api/skills/brand-voice", None).await;
    assert_eq!(s, 200);
    assert!(skill(&d.get_json("/api/skills").await, "brand-voice").is_none());
}

/// Mock `claude` that records whether its prompt carried the plugin block.
fn plugin_agent(d: &Daemon) {
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r###"{PROBES}
IFS= read -r all
case "$all" in *"od-media-generation@0.1.0"*) echo yes > plugin-seen.txt;; *) echo no > plugin-seen.txt;; esac
case "$all" in *"**subject**: green tea"*) echo yes > inputs-seen.txt;; esac
printf '{{"type":"system","subtype":"init","session_id":"s1","model":"mock"}}\n'
printf '{{"type":"assistant","message":{{"id":"m1","content":[{{"type":"text","text":"ok"}}]}}}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
"###
        ),
    );
}

async fn wait_file(path: &Path) -> String {
    for _ in 0..400 {
        if let Ok(t) = std::fs::read_to_string(path)
            && !t.is_empty()
        {
            return t;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{} was not written", path.display());
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn plugins_install_use_and_uninstall() {
    let d = daemon().await;
    plugin_agent(&d);
    // Install from a local folder.
    let src = d.tmp.path().join("src/recipe");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("open-design.json"),
        json!({
            "name": "recipe-cards", "title": "Recipe cards", "version": "0.1.0",
            "description": "Cards for recipes.",
            "od": {
                "kind": "scenario",
                "useCase": { "query": { "en": "Make recipe cards for {{dish}}" } },
                "inputs": [{ "name": "dish", "type": "string", "required": true, "label": "Dish" }]
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        src.join("SKILL.md"),
        "---\nname: recipe-cards\n---\nUse big photos.\n",
    )
    .unwrap();
    let (s, v) = call(
        &d,
        "POST",
        "/api/plugins/install",
        Some(json!({ "path": src.to_string_lossy() })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["plugin"]["source"], "user");
    let list = d.get_json("/api/plugins").await;
    assert!(
        list["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "recipe-cards" && p["source"] == "user")
    );
    let (s, _) = call(
        &d,
        "POST",
        "/api/plugins/install",
        Some(json!({ "path": "relative/x" })),
    )
    .await;
    assert_eq!(s, 400);

    // Use: required inputs are checked, the project carries the plugin.
    let (s, _) = call(&d, "POST", "/api/plugins/recipe-cards/use", Some(json!({}))).await;
    assert_eq!(s, 422);
    let (s, v) = call(
        &d,
        "POST",
        "/api/plugins/recipe-cards/use",
        Some(json!({ "inputs": { "dish": "pho" } })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["project"]["pendingPrompt"], "Make recipe cards for pho");
    assert_eq!(v["project"]["metadata"]["pluginId"], "recipe-cards");

    // A run in a project bound to a plugin gets the plugin block, inputs
    // included, without naming the plugin.
    let (s, v) = call(
        &d,
        "POST",
        "/api/plugins/od-media-generation/use",
        Some(json!({ "inputs": { "subject": "green tea" } })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let pid = v["project"]["id"].as_str().unwrap().to_owned();
    let cid = v["conversationId"].as_str().unwrap().to_owned();
    d.start(
        json!({ "agentId": "claude", "projectId": pid, "conversationId": cid, "message": "go" }),
    )
    .await;
    let dir = d.data.join("projects").join(&pid);
    assert_eq!(wait_file(&dir.join("plugin-seen.txt")).await.trim(), "yes");
    assert_eq!(wait_file(&dir.join("inputs-seen.txt")).await.trim(), "yes");

    // A plain project gets no plugin block.
    let (plain, plain_c) = d.project().await;
    d.start(json!({ "agentId": "claude", "projectId": plain, "conversationId": plain_c, "message": "go" }))
        .await;
    let seen = wait_file(&d.data.join("projects").join(&plain).join("plugin-seen.txt")).await;
    assert_eq!(seen.trim(), "no");

    // Create with agent: a project whose first turn writes and installs.
    let (s, v) = call(
        &d,
        "POST",
        "/api/plugins/author",
        Some(json!({ "brief": "Menu boards" })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let prompt = v["project"]["pendingPrompt"].as_str().unwrap();
    assert!(prompt.contains("Menu boards") && prompt.contains("/api/plugins/install"));
    let (s, v) = call(
        &d,
        "POST",
        "/api/plugins/author",
        Some(json!({ "source": "github:acme/plugins/recipe" })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert!(
        v["project"]["pendingPrompt"]
            .as_str()
            .unwrap()
            .contains("git clone --depth 1")
    );
    let (s, _) = call(
        &d,
        "POST",
        "/api/plugins/author",
        Some(json!({ "source": "file:///etc/passwd" })),
    )
    .await;
    assert_eq!(s, 400);
    let (s, _) = call(&d, "POST", "/api/plugins/author", Some(json!({}))).await;
    assert_eq!(s, 400);

    // Uninstall: user plugins only.
    let (s, _) = call(
        &d,
        "POST",
        "/api/plugins/od-media-generation/uninstall",
        None,
    )
    .await;
    assert_eq!(s, 403);
    let (s, _) = call(&d, "POST", "/api/plugins/recipe-cards/uninstall", None).await;
    assert_eq!(s, 200);
    assert!(!d.data.join("plugins/recipe-cards").exists());
    let (s, _) = call(
        &d,
        "POST",
        "/api/plugins/recipe-cards/use",
        Some(json!({ "inputs": { "dish": "x" } })),
    )
    .await;
    assert_eq!(s, 404);
}

/// A saved template's files come back at exactly their saved names: the
/// project name sanitizer does not rename `My Style.css` (the page links
/// it so) or fold `a~.css` onto `a_.css` (same class as #286).
#[tokio::test]
async fn using_a_saved_template_keeps_file_names_exact() {
    let d = daemon().await;
    let pid = new_project(&d, "Odd names").await;
    write_file(&d, &pid, "index.html", "<h1>Hi</h1>").await;
    let (s, v) = call(
        &d,
        "POST",
        "/api/templates",
        Some(json!({ "name": "Odd names", "sourceProjectId": pid })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let tid = v["template"]["id"].as_str().unwrap().to_owned();
    d.state
        .store
        .with_conn(|c| {
            for (path, content) in [
                ("assets/My Style.css", "h1{color:red}"),
                ("assets/a~.css", "h1{color:green}"),
                ("assets/a_.css", "h1{color:blue}"),
            ] {
                c.execute(
                    "UPDATE templates SET files_json = json_insert(files_json, '$[#]', \
                     json_object('path', ?2, 'content', ?3)) WHERE id = ?1",
                    [&tid, path, content],
                )?;
            }
            Ok(())
        })
        .unwrap();
    let (s, v) = call(&d, "POST", &format!("/api/templates/{tid}/use"), None).await;
    assert_eq!(s, 200, "{v}");
    let copy = v["project"]["id"].as_str().unwrap();
    let files = d.get_json(&format!("/api/projects/{copy}/files")).await;
    let mut names: Vec<&str> = files["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "assets/My Style.css",
            "assets/a_.css",
            "assets/a~.css",
            "index.html"
        ]
    );
}

#[tokio::test]
async fn saved_templates_and_remix() {
    let d = daemon().await;
    let pid = new_project(&d, "Landing").await;
    write_file(&d, &pid, "index.html", "<h1>Hi</h1>").await;
    write_file(&d, &pid, "css/site.css", "h1{color:red}").await;
    let (s, v) = call(
        &d,
        "POST",
        "/api/templates",
        Some(json!({ "name": "My landing", "description": "Hero", "sourceProjectId": pid })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let tid = v["template"]["id"].as_str().unwrap().to_owned();
    assert_eq!(v["template"]["fileCount"], 2);
    // Saving again under the same name updates it.
    write_file(&d, &pid, "about.html", "<p>About</p>").await;
    let (_, v) = call(
        &d,
        "POST",
        "/api/templates",
        Some(json!({ "name": "My landing", "sourceProjectId": pid })),
    )
    .await;
    assert_eq!(v["template"]["id"], tid.as_str());
    assert_eq!(v["template"]["fileCount"], 3);
    let list = d.get_json("/api/templates").await;
    assert_eq!(list["templates"].as_array().unwrap().len(), 1);
    assert!(list["templates"][0].get("files").is_none());

    let (s, v) = call(&d, "POST", &format!("/api/templates/{tid}/use"), None).await;
    assert_eq!(s, 200, "{v}");
    let copy = v["project"]["id"].as_str().unwrap();
    let files = d.get_json(&format!("/api/projects/{copy}/files")).await;
    let names: Vec<&str> = files["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    for n in ["index.html", "css/site.css", "about.html"] {
        assert!(names.contains(&n), "{names:?}");
    }
    // A file that cannot be written (under the page written before it):
    // no project is left half made for Try again to add to.
    d.state
        .store
        .with_conn(|c| {
            c.execute(
                "UPDATE templates SET files_json = json_insert(files_json, '$[#]', \
                 json('{\"path\":\"index.html/x.css\",\"content\":\"x\"}')) WHERE id = ?1",
                [&tid],
            )?;
            Ok(())
        })
        .unwrap();
    let projects = || async {
        d.get_json("/api/projects").await["projects"]
            .as_array()
            .unwrap()
            .len()
    };
    let before = projects().await;
    let (s, v) = call(&d, "POST", &format!("/api/templates/{tid}/use"), None).await;
    assert!(s >= 400, "{s} {v}");
    assert_eq!(projects().await, before, "a half-made project is left");
    let (s, _) = call(
        &d,
        "POST",
        "/api/templates",
        Some(json!({ "name": " ", "sourceProjectId": pid })),
    )
    .await;
    assert_eq!(s, 400);
    let (s, _) = call(&d, "DELETE", &format!("/api/templates/{tid}"), None).await;
    assert_eq!(s, 200);
    let (s, _) = call(&d, "GET", &format!("/api/templates/{tid}"), None).await;
    assert_eq!(s, 404);

    // Remix copies a design template's example into a new project.
    let list = d.get_json("/api/design-templates").await;
    let t = list["designTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["hasExample"] == true)
        .unwrap();
    let id = t["id"].as_str().unwrap();
    let (s, v) = call(
        &d,
        "POST",
        &format!("/api/design-templates/{}/remix", id.replace(':', "%3A")),
        Some(json!({ "prompt": "Make it blue" })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["project"]["skillId"], id);
    assert_eq!(v["project"]["pendingPrompt"], "Make it blue");
    let rpid = v["project"]["id"].as_str().unwrap();
    let html = d
        .http
        .get(format!("{}/api/projects/{rpid}/raw/index.html", d.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let example = d
        .http
        .get(format!(
            "{}/api/design-templates/{}/example",
            d.base,
            id.replace(':', "%3A")
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!example.is_empty());
    assert!(html.contains(&example[..example.len().min(200)]));
}

/// The gallery examples written for Contour (`od.origin: contour`): each
/// is listed with its type, serves a preview page, and Remix turns it into
/// a project of that type holding the same page.
#[tokio::test]
async fn contour_examples_preview_and_remix() {
    let d = daemon().await;
    let list = d.get_json("/api/design-templates").await;
    let listed = list["designTemplates"].as_array().unwrap();
    let mut seen = 0;
    for entry in std::fs::read_dir(content_root().join("design-templates")).unwrap() {
        let dir = entry.unwrap().path();
        let Ok(raw) = std::fs::read_to_string(dir.join("SKILL.md")) else {
            continue; // AGENTS.md
        };
        let (data, _) = od_content::frontmatter::parse(&raw).unwrap();
        if data["od"]["origin"] != "contour" {
            continue;
        }
        seen += 1;
        let id = dir.file_name().unwrap().to_string_lossy().into_owned();
        let mode = data["od"]["mode"].as_str().unwrap();
        let card = listed
            .iter()
            .find(|t| t["id"] == id.as_str())
            .unwrap_or_else(|| panic!("{id} is not listed"));
        assert_eq!(card["mode"], mode, "{id}");
        assert_eq!(card["hasExample"], true, "{id}");

        let res = d
            .http
            .get(format!("{}/api/design-templates/{id}/example", d.base))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status().as_u16(), 200, "{id}");
        let example = res.text().await.unwrap();
        assert!(example.starts_with("<!doctype html>"), "{id}");

        let (s, v) = call(
            &d,
            "POST",
            &format!("/api/design-templates/{id}/remix"),
            None,
        )
        .await;
        assert_eq!(s, 200, "{id}: {v}");
        assert_eq!(v["project"]["skillId"], id.as_str());
        // Named as its card reads, not by its id (QA-056).
        if let Some(en) = data["en_name"].as_str() {
            assert_eq!(v["project"]["name"], en, "{id}");
        }
        let kind = if mode == "deck" { "deck" } else { "prototype" };
        assert_eq!(v["project"]["metadata"]["kind"], kind, "{id}");
        let pid = v["project"]["id"].as_str().unwrap();
        let html = d
            .http
            .get(format!("{}/api/projects/{pid}/raw/index.html", d.base))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(html, example, "{id}: the project holds the example page");
    }
    assert_eq!(seen, 30, "Contour's own gallery examples");
}

/// Every Motion template (Industry "Motion") opens in the preview as a
/// composition `media generate` accepts, and Remix makes a project from it.
#[tokio::test]
async fn motion_templates_preview_and_remix() {
    use od_server::routes::w3::motion::check_composition;
    let d = daemon().await;
    let list = d.get_json("/api/design-templates").await;
    let motion: Vec<&Value> = list["designTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["scenario"] == "motion")
        .collect();
    assert!(motion.len() >= 31, "only {} Motion templates", motion.len());
    let page = list["designTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["mode"] != "video" && t["hasExample"] == true)
        .unwrap();
    assert!(page.get("film").is_none(), "{page}");
    for t in motion {
        let id = t["id"].as_str().unwrap();
        assert_eq!(t["mode"], "video", "{id}");
        assert_eq!(t["hasExample"], true, "{id}");
        let res = d
            .http
            .get(format!("{}/api/design-templates/{id}/example", d.base))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status().as_u16(), 200, "{id}");
        let example = res.text().await.unwrap();
        let (comp, poster) =
            check_composition(&example, &[]).unwrap_or_else(|e| panic!("{id}: {e}"));
        let seconds = comp.duration_ms.unwrap() / 1000;
        assert!((5..=15).contains(&seconds), "{id}: {seconds} s");
        // The listing carries the film's own size and length (QA-055).
        assert_eq!(
            t["film"],
            json!({
                "width": comp.width,
                "height": comp.height,
                "duration": comp.duration_ms.unwrap() as f64 / 1000.0,
            }),
            "{id}"
        );
        assert!(poster > 0.0, "{id}: poster at {poster}");
        let (s, v) = call(
            &d,
            "POST",
            &format!("/api/design-templates/{id}/remix"),
            None,
        )
        .await;
        assert_eq!(s, 200, "{id}: {v}");
        assert_eq!(v["project"]["skillId"], id);
        // A film, as Use template makes it (QA-034).
        let meta = &v["project"]["metadata"];
        assert_eq!(meta["kind"], "video", "{id}");
        assert_eq!(meta["intent"], "motion-design", "{id}");
        assert_eq!(meta["videoModel"], "hyperframes-html", "{id}");
        let pid = v["project"]["id"].as_str().unwrap();
        let html = d
            .http
            .get(format!("{}/api/projects/{pid}/raw/index.html", d.base))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(html.contains("window.__timelines.main"), "{id}");
        assert!(html.contains(&example[..example.len().min(400)]), "{id}");
    }
}

/// Relative `src`/`href` values of an HTML page, read here with a plain
/// pattern so the check does not lean on the code under test.
fn relative_refs(html: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"(?i)\b(?:src|href)\s*=\s*["']([^"']+)["']"#).unwrap();
    let mut out: Vec<String> = re
        .captures_iter(html)
        .map(|c| c[1].split(['#', '?']).next().unwrap_or("").to_owned())
        .filter(|r| {
            !r.is_empty()
                && !r.starts_with('/')
                && !r.contains("{{")
                && !r.contains(':')
                && !r.starts_with("..")
        })
        .map(|r| r.trim_start_matches("./").to_owned())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Remix of a template whose example loads files beside it (`assets/…`)
/// brings those files: the project opens as the template does, and its
/// one-file HTML export is the template's own, byte for byte.
#[tokio::test]
async fn remix_brings_every_file_its_example_uses() {
    let d = daemon().await;
    let mut with_files = Vec::new();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(content_root().join("design-templates"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let Ok(example) = std::fs::read_to_string(dir.join("example.html")) else {
            continue;
        };
        let refs = relative_refs(&example);
        if refs.is_empty() {
            continue;
        }
        let id = dir.file_name().unwrap().to_string_lossy().into_owned();
        with_files.push(id.clone());
        let (s, v) = call(
            &d,
            "POST",
            &format!("/api/design-templates/{id}/remix"),
            None,
        )
        .await;
        assert_eq!(s, 200, "{id}: {v}");
        let pid = v["project"]["id"].as_str().unwrap().to_owned();
        for r in &refs {
            let want = std::fs::read(dir.join(r)).unwrap_or_else(|e| panic!("{id}: {r}: {e}"));
            let res = d
                .http
                .get(format!("{}/api/projects/{pid}/raw/{r}", d.base))
                .send()
                .await
                .unwrap();
            assert_eq!(
                res.status().as_u16(),
                200,
                "{id}: {r} is not in the project"
            );
            assert_eq!(res.bytes().await.unwrap().as_ref(), want, "{id}: {r}");
        }

        // Exported, it is the template's page with the same files inlined.
        let res = d
            .http
            .post(format!("{}/api/projects/{pid}/export/html", d.base))
            .json(&json!({ "fileName": "index.html" }))
            .send()
            .await
            .unwrap();
        let status = res.status().as_u16();
        let got = res.text().await.unwrap();
        assert_eq!(status, 200, "{id}: {got}");
        let root = od_store::files::ProjectRoot {
            dir: dir.clone(),
            imported: true,
        };
        let want = od_export::standalone::bundle(
            "example.html",
            &example,
            &TemplateDir(root),
            &od_export::Options::default(),
        )
        .unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(
            got, want.html,
            "{id}: the export differs from the template's"
        );
    }
    for id in [
        "html-ppt-zhangzara-pin-and-paper",
        "html-ppt-zhangzara-soft-editorial",
        "html-ppt-zhangzara-creative-mode",
        "flowai-live-dashboard-template",
    ] {
        assert!(with_files.iter().any(|w| w == id), "{id}: {with_files:?}");
    }
}

/// Remix copies what the example loads, at any depth, and nothing else:
/// a stylesheet's own `url()`, an iframe's page and its script come; a
/// missing file is skipped without failing the remix; a file out of the
/// template folder, the template's notes and an unused skeleton stay.
#[tokio::test]
async fn remix_copies_only_what_the_example_loads() {
    let root = tempfile::tempdir().unwrap();
    let t = root.path().join("design-templates/side-files");
    std::fs::create_dir_all(t.join("assets/img")).unwrap();
    std::fs::create_dir_all(t.join("assets/frame")).unwrap();
    std::fs::write(
        t.join("SKILL.md"),
        "---\nname: side-files\ndescription: Side files.\nod:\n  mode: deck\n---\n\n# Side files\n",
    )
    .unwrap();
    std::fs::write(
        t.join("example.html"),
        concat!(
            "<!doctype html><link rel=\"stylesheet\" href=\"assets/site.css?v=2\">",
            "<script src=\"./assets/deck.js\"></script>",
            "<iframe src=\"assets/frame/page.html\"></iframe>",
            "<img src=\"assets/gone.png\"><img src=\"../outside.png\">",
            "<a href=\"assets/notes.md\">notes</a><iframe src=\"index.html\"></iframe>",
            "<h1>Deck</h1>",
        ),
    )
    .unwrap();
    std::fs::write(t.join("assets/site.css"), "h1{background:url(img/dot.png)}").unwrap();
    std::fs::write(t.join("assets/img/dot.png"), PNG).unwrap();
    std::fs::write(t.join("assets/deck.js"), "window.deck = 1;").unwrap();
    std::fs::write(
        t.join("assets/frame/page.html"),
        "<script src=\"frame.js\"></script>",
    )
    .unwrap();
    std::fs::write(t.join("assets/frame/frame.js"), "1;").unwrap();
    std::fs::write(t.join("assets/template.html"), "<p>skeleton</p>").unwrap();
    std::fs::write(t.join("assets/notes.md"), "notes").unwrap();
    std::fs::write(t.join("index.html"), "<p>an old index</p>").unwrap();
    std::fs::write(root.path().join("design-templates/outside.png"), PNG).unwrap();

    let d = daemon_with(Options {
        content_root: Some(root.path().to_path_buf()),
        ..Options::default()
    })
    .await;
    let (s, v) = call(&d, "POST", "/api/design-templates/side-files/remix", None).await;
    assert_eq!(s, 200, "{v}");
    let pid = v["project"]["id"].as_str().unwrap();
    let files = d.get_json(&format!("/api/projects/{pid}/files")).await;
    let mut names: Vec<&str> = files["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "assets/deck.js",
            "assets/frame/frame.js",
            "assets/frame/page.html",
            "assets/img/dot.png",
            "assets/site.css",
            "index.html",
        ]
    );
    let png = d
        .http
        .get(format!(
            "{}/api/projects/{pid}/raw/assets/img/dot.png",
            d.base
        ))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(png.as_ref(), PNG, "a binary file comes byte for byte");
    let page = d
        .http
        .get(format!("{}/api/projects/{pid}/raw/index.html", d.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        page.contains("<h1>Deck</h1>"),
        "a side file replaced the page: {page}"
    );
}

/// A content root holding one deck template `id` with these files.
fn one_template(id: &str, files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let t = root.path().join("design-templates").join(id);
    std::fs::create_dir_all(&t).unwrap();
    std::fs::write(
        t.join("SKILL.md"),
        format!("---\nname: {id}\ndescription: A deck.\nod:\n  mode: deck\n---\n\n# {id}\n"),
    )
    .unwrap();
    for (path, bytes) in files {
        let p = t.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    root
}

/// Remixes `id` from `root`; the new project's file names, sorted, and
/// the daemon to read them from.
async fn remix_names(root: &Path, id: &str) -> (Daemon, String, Vec<String>) {
    let d = daemon_with(Options {
        content_root: Some(root.to_path_buf()),
        ..Options::default()
    })
    .await;
    let (s, v) = call(
        &d,
        "POST",
        &format!("/api/design-templates/{id}/remix"),
        None,
    )
    .await;
    assert_eq!(s, 200, "{id}: {v}");
    let pid = v["project"]["id"].as_str().unwrap().to_owned();
    let files = d.get_json(&format!("/api/projects/{pid}/files")).await;
    let mut names: Vec<String> = files["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_owned())
        .collect();
    names.sort_unstable();
    (d, pid, names)
}

/// Copying is not exporting: a page over the export's 2 MiB entry limit
/// and a side file over its 50 MiB asset limit still remix, the file
/// along with it.
#[tokio::test]
async fn remix_copies_past_the_export_limits() {
    let big = vec![7u8; 51 * 1024 * 1024];
    let page = format!(
        "<!doctype html><video src=\"assets/clip.mp4\"></video><h1>Deck</h1><!--{}-->",
        "x".repeat(2 * 1024 * 1024 + 1)
    );
    let root = one_template(
        "big-files",
        &[("example.html", page.as_bytes()), ("assets/clip.mp4", &big)],
    );
    let (d, pid, names) = remix_names(root.path(), "big-files").await;
    assert_eq!(names, ["assets/clip.mp4", "index.html"]);
    let res = d
        .http
        .get(format!("{}/api/projects/{pid}/raw/assets/clip.mp4", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.bytes().await.unwrap().len(), big.len());
}

/// What the walk cannot follow is skipped, and the rest still comes: a
/// file under a folder whose name starts with `.` (refused in a template
/// folder, as in an imported one) and a stylesheet `@import` cycle.
#[tokio::test]
async fn remix_skips_what_it_cannot_follow() {
    let root = one_template(
        "odd-refs",
        &[
            (
                "example.html",
                b"<!doctype html><link rel=\"stylesheet\" href=\".cache/a.css\">\
                  <link rel=\"stylesheet\" href=\"assets/a.css\"><h1>Deck</h1>",
            ),
            (".cache/a.css", b"h1{color:red}"),
            ("assets/a.css", b"@import \"b.css\";"),
            ("assets/b.css", b"@import \"a.css\";"),
        ],
    );
    let (_d, _pid, names) = remix_names(root.path(), "odd-refs").await;
    assert_eq!(names, ["assets/a.css", "assets/b.css", "index.html"]);
}

/// Side files keep the names the page links to: none is renamed by the
/// project name sanitizer, and two that it would map to one name both
/// come (review round 2 of #286).
#[tokio::test]
async fn remix_keeps_side_file_names_exact() {
    let root = one_template(
        "odd-names",
        &[
            (
                "example.html",
                b"<!doctype html><link rel=\"stylesheet\" href=\"assets/My Style.css\">\
                  <link rel=\"stylesheet\" href=\"assets/a~.css\">\
                  <link rel=\"stylesheet\" href=\"assets/a_.css\"><h1>Deck</h1>",
            ),
            ("assets/My Style.css", b"h1{color:red}"),
            ("assets/a~.css", b"h1{color:green}"),
            ("assets/a_.css", b"h1{color:blue}"),
        ],
    );
    let (d, pid, names) = remix_names(root.path(), "odd-names").await;
    assert_eq!(
        names,
        [
            "assets/My Style.css",
            "assets/a_.css",
            "assets/a~.css",
            "index.html"
        ]
    );
    for (name, want) in [
        ("assets/My%20Style.css", "h1{color:red}"),
        ("assets/a~.css", "h1{color:green}"),
        ("assets/a_.css", "h1{color:blue}"),
    ] {
        let res = d
            .http
            .get(format!("{}/api/projects/{pid}/raw/{name}", d.base))
            .send()
            .await
            .unwrap();
        assert_eq!(res.text().await.unwrap(), want, "{name}");
    }
}

/// An example that is not UTF-8 (a template saved in Latin-1) remixes,
/// and the project's page is its bytes as they are.
#[tokio::test]
async fn remix_keeps_an_example_that_is_not_utf8() {
    let page: &[u8] = b"<!doctype html><meta charset=\"latin1\"><h1>Caf\xe9</h1>\
                        <script src=\"assets/deck.js\"></script>";
    let root = one_template(
        "latin1",
        &[("example.html", page), ("assets/deck.js", b"1;")],
    );
    let (d, pid, names) = remix_names(root.path(), "latin1").await;
    assert_eq!(names, ["assets/deck.js", "index.html"]);
    let got = d
        .http
        .get(format!("{}/api/projects/{pid}/raw/index.html", d.base))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(got.as_ref(), page);
}

/// A template folder read as the export reads a project.
struct TemplateDir(od_store::files::ProjectRoot);

impl od_export::AssetSource for TemplateDir {
    fn stat(&self, path: &str) -> std::io::Result<Option<od_export::AssetMeta>> {
        Ok(od_store::files::stat_file(&self.0, path)
            .ok()
            .map(|(f, _)| od_export::AssetMeta {
                mime: f.mime,
                size: f.size,
            }))
    }

    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        od_store::files::read_file(&self.0, path)
            .map(|(_, b)| b)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }
}

/// A 1×1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0,
    1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0a, 0x49, 0x44, 0x41, 0x54,
    0x78, 0x9c, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0d, 0x0a, 0x2d, 0xb4, 0, 0, 0, 0, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

#[tokio::test]
async fn media_library_lists_uploads_and_chats_to_design() {
    let d = daemon().await;
    let pid = new_project(&d, "Poster").await;
    let (s, _) = upload(
        &d,
        &format!("/api/projects/{pid}/upload"),
        &[("hero.png", PNG), ("song.mp3", b"ID3")],
    )
    .await;
    assert_eq!(s, 200);
    write_file(&d, &pid, "index.html", "<p>not media</p>").await;

    let (s, v) = upload(&d, "/api/library/upload", &[("mood.png", PNG)]).await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["projectId"], "media-library");
    let (s, _) = upload(&d, "/api/library/upload", &[("notes.txt", b"x")]).await;
    assert_eq!(s, 400, "only media files");
    // A name too long for the disk is shortened, keeping its extension
    // (QA-106), not refused every time.
    let long = format!("{}.png", "x".repeat(300));
    let (s, v) = upload(&d, "/api/library/upload", &[(long.as_str(), PNG)]).await;
    assert_eq!(s, 200, "{v}");
    let kept = v["files"][0]["path"].as_str().unwrap().to_owned();
    assert!(
        kept.starts_with("uploads/xxx") && kept.ends_with(".png"),
        "{kept}"
    );
    assert!(kept.len() <= "uploads/".len() + od_store::paths::MAX_UPLOAD_NAME_BYTES);
    // The second file cannot be written (a link out of the folder at its
    // name): all or nothing, so Try again does not add `first-1.png`.
    #[cfg(unix)]
    {
        let uploads = d.data.join("projects/media-library/uploads");
        std::os::unix::fs::symlink(d.data.join("nowhere/x.png"), uploads.join("second.png"))
            .unwrap();
        let (s, v) = upload(
            &d,
            "/api/library/upload",
            &[("first.png", PNG), ("second.png", PNG)],
        )
        .await;
        assert!(s >= 400, "{s} {v}");
        assert!(!uploads.join("first.png").exists(), "the first file kept");
        std::fs::remove_file(uploads.join("second.png")).unwrap();
    }
    // The project holding uploads is not one of the user's designs.
    let listed = d.get_json("/api/projects").await;
    let ids: Vec<&str> = listed["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [pid.as_str()], "media library listed as a project");

    let all = d.get_json("/api/library/assets").await;
    let paths: Vec<&str> = all["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths.len(), 4, "{paths:?}");
    assert!(paths.contains(&"uploads/mood.png"));
    let audio = d.get_json("/api/library/assets?kind=audio").await;
    assert_eq!(audio["assets"].as_array().unwrap().len(), 1);
    assert_eq!(audio["assets"][0]["projectName"], "Poster");
    let q = d.get_json("/api/library/assets?q=mood").await;
    assert_eq!(q["assets"][0]["uploaded"], true);

    let (s, v) = call(
        &d,
        "POST",
        "/api/library/chat",
        Some(json!({
            "assets": [{ "projectId": pid, "path": "hero.png" }, { "projectId": "media-library", "path": "uploads/mood.png" }],
            "prompt": "A tea poster",
        })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let prompt = v["project"]["pendingPrompt"].as_str().unwrap();
    assert!(prompt.starts_with("A tea poster"));
    assert!(prompt.contains("assets/hero.png") && prompt.contains("assets/mood.png"));
    let np = v["project"]["id"].as_str().unwrap();
    assert!(
        d.data
            .join("projects")
            .join(np)
            .join("assets/hero.png")
            .is_file()
    );
    let (s, _) = call(
        &d,
        "POST",
        "/api/library/chat",
        Some(json!({ "assets": [{ "projectId": pid, "path": "index.html" }] })),
    )
    .await;
    assert_eq!(s, 400);
}

/// Mock `claude` for design-system jobs: a brief writes a package (noting
/// whether the sources reached it), a revision rewrites the accent.
fn design_agent(d: &Daemon) {
    let mut design = String::from("# Tea House\\n\\n> Category: Food\\n\\n");
    for s in od_content::design_system_gen::SECTIONS {
        design.push_str(&format!("## {s}\\n\\nText.\\n\\n"));
    }
    write_mock(
        &d.mocks,
        "claude",
        &format!(
            r##"{PROBES}
IFS= read -r all
case "$all" in
*"# Design system revision"*)
  printf '%s\n\n## Revised\n\nWarmer.\n' "$(cat DESIGN.md)" > DESIGN.md
  sed 's/--accent: [^;]*;/--accent: #aa5500;/' tokens.css > t.css && mv t.css tokens.css
  ;;
*"# Design system brief"*)
  printf '%b' "{design}" > DESIGN.md
  case "$all" in *"Website: https://tea.example"*) [ -f source/logo.png ] && echo "- **Source** \`#123456\`" >> DESIGN.md;; esac
  printf ':root {{ --bg: #f6efe2; --surface: #fff; --fg: #1d1b16; --accent: #2f6b3a; --font-display: Georgia; --font-body: system-ui; }}\n' > tokens.css
  ;;
*) exit 1;;
esac
printf '{{"type":"system","subtype":"init","session_id":"s1","model":"mock"}}\n'
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
"##
        ),
    );
}

async fn wait<F: Fn(&Value) -> bool>(d: &Daemon, path: &str, done: F) -> Value {
    for _ in 0..400 {
        let v = d.get_json(path).await;
        if done(&v) {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{path} did not settle");
}

/// A user package from pasted CSS, and its folder on disk.
#[cfg(unix)]
async fn css_package(d: &Daemon, name: &str) -> (String, PathBuf) {
    let (s, v) = call(
        d,
        "POST",
        "/api/design-systems/import",
        Some(json!({ "name": name, "css": ":root { --background: 0 0% 100%; --foreground: 240 10% 4%; --primary: 142 70% 40%; }" })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let id = v["id"].as_str().unwrap().to_owned();
    let dir = d.data.join("design-systems").join(&id["user:".len()..]);
    assert!(dir.join("DESIGN.md").is_file(), "{dir:?}");
    (id, dir)
}

/// A symlink planted in a package (an agent editing it in place can make
/// one) never takes a route's read or write outside the package.
// Escapes are planted as symlinks, so Unix only.
#[cfg(unix)]
#[tokio::test]
async fn design_system_routes_stay_inside_the_package() {
    let d = daemon().await;
    let outside = d.tmp.path().join("outside");
    std::fs::create_dir_all(outside.join("revs")).unwrap();
    let secret = |name: &str| {
        let p = outside.join(name);
        std::fs::write(&p, "OUTSIDE").unwrap();
        p
    };
    let unchanged = |p: &Path| assert_eq!(std::fs::read_to_string(p).unwrap(), "OUTSIDE", "{p:?}");
    let link = |target: &Path, at: PathBuf| {
        let _ = std::fs::remove_file(&at);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(target, at).unwrap();
    };
    let (id, dir) = css_package(&d, "Links").await;

    // Reads: a package file, the kit's tokens, the showcase.
    let leak = secret("leak.txt");
    link(&leak, dir.join("assets/leak.txt"));
    let (s, v) = call(
        &d,
        "GET",
        &format!("/api/design-systems/{id}/file/assets/leak.txt"),
        None,
    )
    .await;
    assert_ne!(s, 200, "{v}");
    assert!(!v.to_string().contains("OUTSIDE"));
    let tokens = dir.join("tokens.css");
    let kept_tokens = std::fs::read_to_string(&tokens).unwrap();
    let css = outside.join("leak.css");
    std::fs::write(&css, ":root { --leak: OUTSIDE; }").unwrap();
    link(&css, tokens.clone());
    let (_, v) = call(&d, "GET", &format!("/api/design-systems/{id}/kit"), None).await;
    assert!(!v.to_string().contains("OUTSIDE"), "{v}");
    std::fs::remove_file(&tokens).unwrap();
    std::fs::write(&tokens, kept_tokens).unwrap();
    link(&leak, dir.join("components.html"));
    let (_, v) = call(
        &d,
        "GET",
        &format!("/api/design-systems/{id}/showcase"),
        None,
    )
    .await;
    assert!(!v.to_string().contains("OUTSIDE"));
    std::fs::remove_file(dir.join("components.html")).unwrap();

    // Writes: an asset upload, the status, a revision folder.
    let logo = secret("logo.png");
    link(&logo, dir.join("assets/logo.png"));
    let _ = upload(
        &d,
        &format!("/api/design-systems/{id}/assets"),
        &[("first.png", b"PNG"), ("logo.png", b"PNG")],
    )
    .await;
    unchanged(&logo);
    assert!(
        !dir.join("assets/first.png").exists(),
        "a file before the refused one kept"
    );
    let meta = secret("metadata.json");
    link(&meta, dir.join("metadata.json"));
    let _ = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/status"),
        Some(json!({"status": "published"})),
    )
    .await;
    unchanged(&meta);
    std::fs::remove_file(dir.join("metadata.json")).unwrap();
    link(&outside.join("revs"), dir.join("revisions"));
    let _ = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/token-contract/rebuild"),
        None,
    )
    .await;
    assert_eq!(std::fs::read_dir(outside.join("revs")).unwrap().count(), 0);
    std::fs::remove_file(dir.join("revisions")).unwrap();
    // Accepting a revision writes tokens.css: not through a link.
    let (s, v) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/token-contract/rebuild"),
        None,
    )
    .await;
    assert_eq!((s, &v["decision"]), (200, &json!("revision")), "{v}");
    let rid = v["revision"]["id"].as_str().unwrap().to_owned();
    let css = secret("tokens.css");
    link(&css, dir.join("tokens.css"));
    let _ = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/revisions/{rid}"),
        Some(json!({"status": "accepted"})),
    )
    .await;
    unchanged(&css);

    // DESIGN.md itself, linked after the package loaded: neither read nor
    // overwritten, and after a reload the package is not listed through it.
    let design = secret("DESIGN.md");
    link(&design, dir.join("DESIGN.md"));
    let (_, v) = call(&d, "GET", &format!("/api/design-systems/{id}/kit"), None).await;
    assert!(!v.to_string().contains("OUTSIDE"), "{v}");
    let _ = call(
        &d,
        "PUT",
        &format!("/api/design-systems/{id}/design-md"),
        Some(json!({"body": "# Over"})),
    )
    .await;
    unchanged(&design);
    let _ = call(&d, "POST", "/api/design-systems/reload", None).await;
    let (_, v) = call(&d, "GET", &format!("/api/design-systems/{id}/kit"), None).await;
    assert!(!v.to_string().contains("OUTSIDE"), "{v}");
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn design_system_kit_import_copy_and_revisions() {
    let d = daemon().await;
    design_agent(&d);

    // Bundled kit: tokens, palette, score, showcase; writes are refused.
    let kit = d.get_json("/api/design-systems/airbnb/kit").await;
    assert!(
        kit["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "--accent")
    );
    assert!(
        kit["palette"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["hex"] == "#ff385c")
    );
    assert!(
        kit["score"]["score"].as_u64().unwrap() >= 80,
        "{}",
        kit["score"]
    );
    assert_eq!(kit["hasComponents"], true);
    // A brand-named system names the brand for the trademark note.
    assert_eq!(kit["brand"], "Airbnb");
    let res = d
        .http
        .get(format!("{}/api/design-systems/airbnb/showcase", d.base))
        .send()
        .await
        .unwrap();
    assert!(
        res.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("sandbox")
    );
    let (s, _) = call(
        &d,
        "PUT",
        "/api/design-systems/airbnb/design-md",
        Some(json!({ "body": "# x" })),
    )
    .await;
    assert_eq!(s, 403);
    let (s, _) = call(
        &d,
        "GET",
        "/api/design-systems/airbnb/file/..%2Fapple%2FDESIGN.md",
        None,
    )
    .await;
    assert_eq!(s, 400);

    // Import a pasted shadcn theme.
    let (s, v) = call(
        &d,
        "POST",
        "/api/design-systems/import",
        Some(json!({ "name": "Shad", "css": ":root { --background: 0 0% 100%; --foreground: 240 10% 4%; --primary: 142 70% 40%; }" })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["id"], "user:shad");
    let kit = d.get_json("/api/design-systems/user:shad/kit").await;
    assert_eq!(kit["summary"]["status"], "draft");
    assert!(kit["body"].as_str().unwrap().contains("## 2. Color"));
    assert_eq!(kit["hasComponents"], false);
    assert!(
        kit.get("brand").is_none(),
        "no brand on a user's own system"
    );
    let showcase = d
        .http
        .get(format!("{}/api/design-systems/user:shad/showcase", d.base))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(showcase.contains("--accent:hsl(142 70% 40%)"));
    // Pasted DESIGN.md: tokens from its palette.
    let (s, v) = call(
        &d,
        "POST",
        "/api/design-systems/import",
        Some(json!({ "name": "Paste", "designMd": "# Paste\n\n- **Cream** `#f6efe2`\n- **Leaf** `#2f6b3a`\n- **Ink** `#1d1b16`\n" })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let (s, _) = call(
        &d,
        "POST",
        "/api/design-systems/import",
        Some(json!({ "name": "None" })),
    )
    .await;
    assert_eq!(s, 400);

    // Copy a bundled system, then edit, publish, upload, archive.
    let (s, v) = call(
        &d,
        "POST",
        "/api/design-systems/airbnb/copy",
        Some(json!({ "name": "My Airbnb" })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let id = v["id"].as_str().unwrap().to_owned();
    assert_eq!(id, "user:my-airbnb");
    let list = d.get_json("/api/design-systems").await;
    let mine = list["designSystems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id.as_str())
        .unwrap();
    assert_eq!(mine["title"], "My Airbnb");
    assert_eq!(mine["isEditable"], true);
    let (s, _) = call(
        &d,
        "PUT",
        &format!("/api/design-systems/{id}/design-md"),
        Some(json!({ "body": "# My Airbnb\n\n- **Coral** `#ff5a5f`\n" })),
    )
    .await;
    assert_eq!(s, 200);
    let kit = d.get_json(&format!("/api/design-systems/{id}/kit")).await;
    assert_eq!(kit["palette"][0]["hex"], "#ff5a5f");
    let (s, _) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/status"),
        Some(json!({ "status": "published" })),
    )
    .await;
    assert_eq!(s, 200);
    let list = d.get_json("/api/design-systems").await;
    let mine = list["designSystems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id.as_str())
        .unwrap();
    assert_eq!(mine["status"], "published");
    let (s, v) = upload(
        &d,
        &format!("/api/design-systems/{id}/assets"),
        &[("logo.png", PNG), ("Brand.woff2", b"wOF2")],
    )
    .await;
    assert_eq!(s, 200, "{v}");
    // One file refused and none of the batch is written.
    let (s, _) = upload(
        &d,
        &format!("/api/design-systems/{id}/assets"),
        &[("ok.png", PNG), ("x.sh", b"echo")],
    )
    .await;
    assert_eq!(s, 400);
    assert!(
        !d.data
            .join("design-systems/my-airbnb/assets/ok.png")
            .exists(),
        "the file before the refused one kept"
    );
    // QA-107: Vietnamese names keep their letters, so two of them are two
    // assets, not one `m-u.png` written over the other.
    let assets = d.data.join("design-systems/my-airbnb/assets");
    let (s, v) = upload(
        &d,
        &format!("/api/design-systems/{id}/assets"),
        &[("màu.png", b"red"), ("mẫu.png", b"blue!")],
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(std::fs::read(assets.join("màu.png")).unwrap(), b"red");
    assert_eq!(std::fs::read(assets.join("mẫu.png")).unwrap(), b"blue!");
    // Two files one name in a batch: refused, naming both, none written.
    let (s, v) = upload(
        &d,
        &format!("/api/design-systems/{id}/assets"),
        &[("Hình.png", PNG), ("hình.png", PNG)],
    )
    .await;
    assert_eq!(s, 400, "{v}");
    let said = v["error"]["message"].as_str().unwrap();
    assert!(
        said.contains("Hình.png") && said.contains("hình.png"),
        "{said}"
    );
    assert!(!assets.join("Hình.png").exists() && !assets.join("hình.png").exists());
    // An asset name already on disk (220 bytes, kept whole before names
    // were shortened) is deleted by that very name, never by a shortened
    // one that names another asset.
    let old = format!("{}.png", "o".repeat(216));
    let cut = od_store::paths::upload_name(&old);
    assert_ne!(old, cut);
    std::fs::write(assets.join(&old), b"old").unwrap();
    std::fs::write(assets.join(&cut), b"cut").unwrap();
    let (s, v) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/assets/{old}/delete"),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert!(!assets.join(&old).exists(), "the old asset kept");
    assert_eq!(
        std::fs::read(assets.join(&cut)).unwrap(),
        b"cut",
        "another asset deleted"
    );
    std::fs::remove_file(assets.join(&cut)).unwrap();
    // Two names too long for the disk, alike up to the cut, sent in two
    // batches: two assets, the second never written over the first.
    let mut kept = Vec::new();
    for (n, bytes) in [(1, b"one".as_slice()), (2, b"two!".as_slice())] {
        let raw = format!("{}-v{n}.png", "b".repeat(260));
        let (s, v) = upload(
            &d,
            &format!("/api/design-systems/{id}/assets"),
            &[(raw.as_str(), bytes)],
        )
        .await;
        assert_eq!(s, 200, "{v}");
        let name = v["assets"][0]["name"].as_str().unwrap().to_owned();
        assert!(name.len() <= 255 && name.ends_with(".png"), "{name}");
        kept.push((name, bytes));
    }
    assert_ne!(kept[0].0, kept[1].0, "both long names saved as one");
    for (name, bytes) in &kept {
        assert_eq!(
            &std::fs::read(assets.join(name)).unwrap(),
            bytes,
            "{name} written over"
        );
        let (s, _) = call(
            &d,
            "POST",
            &format!("/api/design-systems/{id}/assets/{name}/delete"),
            None,
        )
        .await;
        assert_eq!(s, 200, "delete {name}");
    }
    // A shortened name whose own is taken skips `stem-1.png` when another
    // file of the batch is sent as that: neither written over.
    // `-` at the cut is trimmed, so the shortened name has room for `-1`.
    let raw = format!(
        "{}{}{}.png",
        "c".repeat(190),
        "-".repeat(20),
        "x".repeat(100)
    );
    let cut = od_store::paths::upload_name(&raw);
    let one = cut.replace(".png", "-1.png");
    assert_eq!(od_store::paths::upload_name(&one), one);
    std::fs::write(assets.join(&cut), b"before").unwrap();
    let (s, v) = upload(
        &d,
        &format!("/api/design-systems/{id}/assets"),
        &[(raw.as_str(), b"long"), (one.as_str(), b"one!")],
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let two = cut.replace(".png", "-2.png");
    assert_eq!(v["assets"][0]["name"], two.as_str(), "{v}");
    assert_eq!(std::fs::read(assets.join(&cut)).unwrap(), b"before");
    assert_eq!(std::fs::read(assets.join(&one)).unwrap(), b"one!");
    assert_eq!(std::fs::read(assets.join(&two)).unwrap(), b"long");
    for n in [&cut, &one, &two] {
        std::fs::remove_file(assets.join(n)).unwrap();
    }
    for n in ["màu.png", "mẫu.png"] {
        let (s, _) = call(
            &d,
            "POST",
            &format!("/api/design-systems/{id}/assets/{n}/delete"),
            None,
        )
        .await;
        assert_eq!(s, 200, "delete {n}");
    }
    let kit = d.get_json(&format!("/api/design-systems/{id}/kit")).await;
    let kinds: Vec<&str> = kit["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["font", "logo"]);
    let logo = d
        .http
        .get(format!(
            "{}/api/design-systems/{id}/file/assets/logo.png",
            d.base
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(logo.headers()["content-type"], "image/png");
    let (s, _) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/assets/Brand.woff2/delete"),
        None,
    )
    .await;
    assert_eq!(s, 200);
    let zip = d
        .http
        .get(format!("{}/api/design-systems/{id}/archive", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(zip.headers()["content-type"], "application/zip");
    let bytes = zip.bytes().await.unwrap();
    let names: Vec<String> = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec()))
        .unwrap()
        .file_names()
        .map(|n| n.unwrap().into_owned())
        .collect();
    assert!(
        names.contains(&"my-airbnb/DESIGN.md".to_owned()),
        "{names:?}"
    );
    assert!(names.contains(&"my-airbnb/assets/logo.png".to_owned()));

    // Token contract: a thin package gets a rebuild revision; accepting
    // it writes tokens.css, a second decision conflicts.
    let (s, v) = call(
        &d,
        "POST",
        "/api/design-systems/user:shad/token-contract/rebuild",
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    assert_eq!(v["decision"], "revision");
    let before = v["score"]["score"].as_u64().unwrap();
    let rid = v["revision"]["id"].as_str().unwrap().to_owned();
    let (s, _) = call(
        &d,
        "POST",
        &format!("/api/design-systems/user:shad/revisions/{rid}"),
        Some(json!({ "status": "accepted" })),
    )
    .await;
    assert_eq!(s, 200);
    let kit = d.get_json("/api/design-systems/user:shad/kit").await;
    assert!(kit["score"]["score"].as_u64().unwrap() > before);
    assert_eq!(kit["revisions"][0]["status"], "accepted");
    let (s, _) = call(
        &d,
        "POST",
        &format!("/api/design-systems/user:shad/revisions/{rid}"),
        Some(json!({ "status": "rejected" })),
    )
    .await;
    assert_eq!(s, 409);
    let (_, v) = call(
        &d,
        "POST",
        "/api/design-systems/user:shad/token-contract/rebuild",
        None,
    )
    .await;
    assert_eq!(v["decision"], "up-to-date");

    // Needs work: the agent revises a copy; reject leaves the package.
    let (s, v) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/revisions"),
        Some(json!({ "feedback": "Warmer accent", "section": "Color" })),
    )
    .await;
    assert_eq!(s, 202, "{v}");
    let revs = wait(&d, &format!("/api/design-systems/{id}/revisions"), |v| {
        v["revisions"][0]["status"] != "running"
    })
    .await;
    let r = &revs["revisions"][0];
    assert_eq!(r["status"], "pending", "{r}");
    let tokens = r["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "tokens.css")
        .unwrap();
    assert!(
        tokens["content"]
            .as_str()
            .unwrap()
            .contains("--accent: #aa5500;")
    );
    let rid = r["id"].as_str().unwrap();
    let (s, _) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/revisions/{rid}"),
        Some(json!({ "status": "rejected" })),
    )
    .await;
    assert_eq!(s, 200);
    let css = std::fs::read_to_string(d.data.join("design-systems/my-airbnb/tokens.css")).unwrap();
    assert!(!css.contains("#aa5500"));

    // An accept that cannot write every file writes none: the revision
    // stays pending, and a Reject then leaves the package as it was.
    let package = d.data.join("design-systems/my-airbnb");
    let rdir = package.join("revisions/half");
    std::fs::create_dir_all(&rdir).unwrap();
    std::fs::write(
        rdir.join("revision.json"),
        json!({"id": "half", "kind": "agent", "status": "pending",
               "createdAt": 1, "updatedAt": 1})
        .to_string(),
    )
    .unwrap();
    std::fs::write(rdir.join("DESIGN.md"), "# Half\n").unwrap();
    std::fs::write(rdir.join("tokens.css"), ":root { --half: 1; }\n").unwrap();
    let design = std::fs::read_to_string(package.join("DESIGN.md")).unwrap();
    let tokens = std::fs::read_to_string(package.join("tokens.css")).unwrap();
    let locked = |file: &std::path::Path, on: bool| {
        let mut perms = std::fs::metadata(file).unwrap().permissions();
        perms.set_readonly(on);
        std::fs::set_permissions(file, perms).unwrap();
    };
    // The second file cannot be written; then the decision itself.
    for stuck in [package.join("tokens.css"), rdir.join("revision.json")] {
        locked(&stuck, true);
        let (s, v) = call(
            &d,
            "POST",
            &format!("/api/design-systems/{id}/revisions/half"),
            Some(json!({ "status": "accepted" })),
        )
        .await;
        locked(&stuck, false);
        // A refused write says so, not "try again" (QA-106).
        assert_eq!(s, 403, "{stuck:?}: {v}");
        assert_eq!(v["error"]["code"], "NO_WRITE_PERMISSION", "{v}");
        for (name, was) in [("DESIGN.md", &design), ("tokens.css", &tokens)] {
            assert_eq!(
                &std::fs::read_to_string(package.join(name)).unwrap(),
                was,
                "{stuck:?}: {name} of the revision written"
            );
        }
        let revs = d
            .get_json(&format!("/api/design-systems/{id}/revisions"))
            .await;
        let half = revs["revisions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "half")
            .unwrap();
        assert_eq!(half["status"], "pending", "{stuck:?}");
    }
    let (s, _) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/revisions/half"),
        Some(json!({ "status": "accepted" })),
    )
    .await;
    assert_eq!(s, 200, "Try again");
    assert_eq!(
        std::fs::read_to_string(package.join("DESIGN.md")).unwrap(),
        "# Half\n"
    );

    // Edit with agent: one project whose folder is the package.
    let (s, v) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/workspace"),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let wp = v["project"]["id"].as_str().unwrap().to_owned();
    assert!(
        v["project"]["metadata"]["baseDir"]
            .as_str()
            .unwrap()
            .ends_with("design-systems/my-airbnb")
    );
    let (_, v) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/workspace"),
        None,
    )
    .await;
    assert_eq!(v["project"]["id"], wp.as_str(), "reused");
    let (s, _) = call(&d, "POST", "/api/design-systems/airbnb/workspace", None).await;
    assert_eq!(s, 403);

    // Delete.
    let (s, _) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/delete"),
        None,
    )
    .await;
    assert_eq!(s, 200);
    assert!(!d.data.join("design-systems/my-airbnb").exists());
    let (s, _) = call(&d, "POST", "/api/design-systems/airbnb/delete", None).await;
    assert_eq!(s, 403);
}

/// QA-109: an asset is replaced only by a file sent under its very name
/// (NFC, any case); a name the cleaning changed (`🖼.png` → `_.png`,
/// `logo chính.png` → `logo-chính.png`) takes a free `stem-N.ext`.
#[tokio::test]
async fn design_system_asset_replaced_only_by_its_own_name() {
    let d = daemon().await;
    let (s, v) = call(
        &d,
        "POST",
        "/api/design-systems/airbnb/copy",
        Some(json!({ "name": "Names" })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let id = v["id"].as_str().unwrap().to_owned();
    let url = format!("/api/design-systems/{id}/assets");
    let assets = d.data.join("design-systems/names/assets");
    let send = |name: &'static str, bytes: &'static [u8]| {
        let (d, url) = (&d, url.as_str());
        async move {
            let (s, v) = upload(d, url, &[(name, bytes)]).await;
            assert_eq!(s, 200, "{name}: {v}");
            v["assets"][0]["name"].as_str().unwrap().to_owned()
        }
    };
    let on_disk = || {
        let mut names: Vec<String> = std::fs::read_dir(&assets)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let read = |n: &str| std::fs::read(assets.join(n)).unwrap();

    // Emoji: both cleaned to `_.png`; the second takes `_-1.png`.
    assert_eq!(send("🎨.png", b"red").await, "_.png");
    assert_eq!(send("🖼.png", b"blue!").await, "_-1.png");
    assert_eq!(read("_.png"), b"red", "🎨.png written over");
    assert_eq!(read("_-1.png"), b"blue!");
    // A space cleaned to `-`: not the asset already named `logo-chính.png`.
    assert_eq!(send("logo-chính.png", b"one").await, "logo-chính.png");
    assert_eq!(send("logo chính.png", b"two!").await, "logo-chính-1.png");
    assert_eq!(
        read("logo-chính.png"),
        b"one",
        "logo-chính.png written over"
    );
    // The other way round (UL-15): the cleaned one first, then a file
    // sent under the name the first is kept as.
    assert_eq!(send("tên mới.png", b"a").await, "tên-mới.png");
    assert_eq!(send("tên-mới.png", b"bb").await, "tên-mới-1.png");
    assert_eq!(read("tên-mới.png"), b"a", "tên mới.png written over");
    // A file sent again under the name it was sent as replaces it, NFD or
    // another case too, under the name on disk.
    assert_eq!(send("Mẫu.png", b"old").await, "Mẫu.png");
    assert_eq!(send("ma\u{302}\u{303}u.PNG", b"new!").await, "Mẫu.png");
    assert_eq!(read("Mẫu.png"), b"new!", "Mẫu.png not replaced");
    assert_eq!(send("🎨.png", b"RED").await, "_.png");
    assert_eq!(read("_.png"), b"RED", "🎨.png not replaced");
    assert_eq!(send("logo-chính.png", b"three").await, "logo-chính.png");
    assert_eq!(send("logo chính.png", b"four!").await, "logo-chính-1.png");
    assert_eq!(read("logo-chính.png"), b"three");
    assert_eq!(read("tên-mới-1.png"), b"bb");
    assert_eq!(read("_-1.png"), b"blue!");
    assert_eq!(
        on_disk(),
        [
            "Mẫu.png",
            "_-1.png",
            "_.png",
            "logo-chính-1.png",
            "logo-chính.png",
            "tên-mới-1.png",
            "tên-mới.png",
        ],
        "an extra or a lost asset"
    );
    // Deleted, its sent name goes too: a file put there after (the
    // agent's) is not what `logo chính.png` was sent as.
    let (s, v) = call(
        &d,
        "POST",
        &format!("/api/design-systems/{id}/assets/logo-chính-1.png/delete"),
        None,
    )
    .await;
    assert_eq!(s, 200, "{v}");
    std::fs::write(assets.join("logo-chính-1.png"), b"agent").unwrap();
    assert_eq!(send("logo chính.png", b"five").await, "logo-chính-2.png");
    assert_eq!(
        read("logo-chính-1.png"),
        b"agent",
        "the agent's file written over"
    );
    // The sent names are kept beside `assets/`, which holds assets only:
    // the kit lists what is on disk there, nothing more.
    assert!(
        d.data
            .join("design-systems/names/.sent-names.json")
            .is_file()
    );
    let kit = d.get_json(&format!("/api/design-systems/{id}/kit")).await;
    let listed: Vec<&str> = kit["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(listed, on_disk(), "the kit lists another asset");
}

/// Copies an editable design system named `name`; returns its id and its
/// `assets/` folder.
async fn ds_copy(d: &Daemon, from: &str, name: &str) -> (String, PathBuf) {
    let (s, v) = call(
        d,
        "POST",
        &format!("/api/design-systems/{from}/copy"),
        Some(json!({ "name": name })),
    )
    .await;
    assert_eq!(s, 200, "{v}");
    let id = v["id"].as_str().unwrap().to_owned();
    let folder = id.trim_start_matches("user:").to_owned();
    (
        id,
        d.data.join("design-systems").join(folder).join("assets"),
    )
}

/// Uploads one asset; returns the name it is kept under.
async fn ds_send(d: &Daemon, id: &str, name: &str, bytes: &[u8]) -> String {
    let (s, v) = upload(
        d,
        &format!("/api/design-systems/{id}/assets"),
        &[(name, bytes)],
    )
    .await;
    assert_eq!(s, 200, "{name}: {v}");
    v["assets"][0]["name"].as_str().unwrap().to_owned()
}

/// QA-109 (review): the sent names go with a copy and with the archive,
/// so a copy keeps the first of `logo chính.png` and `logo-chính.png`;
/// an asset with no sent name recorded (uploaded before them, or put
/// there by the agent) is replaced by a file sent under its own name.
#[tokio::test]
async fn design_system_sent_names_go_with_copies_and_archives() {
    let d = daemon().await;
    let (id, assets) = ds_copy(&d, "airbnb", "Sent").await;
    assert_eq!(
        ds_send(&d, &id, "logo chính.png", b"first").await,
        "logo-chính.png"
    );
    // Copied: `logo-chính.png` sent to the copy is a new asset there.
    let (twin, twin_assets) = ds_copy(&d, &id, "Twin").await;
    assert_eq!(
        ds_send(&d, &twin, "logo-chính.png", b"second").await,
        "logo-chính-1.png"
    );
    assert_eq!(
        std::fs::read(twin_assets.join("logo-chính.png")).unwrap(),
        b"first"
    );
    assert_eq!(
        ds_send(&d, &twin, "logo chính.png", b"third").await,
        "logo-chính.png"
    );
    // Archived: the sent names are in the ZIP, at the package's top.
    let zip = d
        .http
        .get(format!("{}/api/design-systems/{id}/archive", d.base))
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip.to_vec())).unwrap();
    let mut kept = String::new();
    std::io::Read::read_to_string(
        &mut zip.by_name("sent/.sent-names.json").unwrap(),
        &mut kept,
    )
    .unwrap();
    let kept: Value = serde_json::from_str(&kept).unwrap();
    assert_eq!(kept, json!({ "logo-chính.png": "logo chính.png" }));
    // No sent name recorded: the name on disk is the one it was sent as.
    std::fs::write(assets.join("old logo.png"), b"before").unwrap();
    assert_eq!(
        ds_send(&d, &id, "old logo.png", b"after").await,
        "old logo.png"
    );
    assert_eq!(
        std::fs::read(assets.join("old logo.png")).unwrap(),
        b"after"
    );
}

/// QA-109 (review): an asset whose sent name cannot be recorded is not
/// kept, so Try again replaces nothing it should not and adds no `-1`.
#[cfg(unix)]
#[tokio::test]
async fn design_system_asset_not_kept_without_its_sent_name() {
    use std::os::unix::fs::PermissionsExt as _;
    let d = daemon().await;
    let (id, assets) = ds_copy(&d, "airbnb", "Locked").await;
    std::fs::create_dir_all(&assets).unwrap();
    let package = assets.parent().unwrap().to_owned();
    let mode = |m| std::fs::set_permissions(&package, std::fs::Permissions::from_mode(m)).unwrap();
    mode(0o555);
    if std::fs::write(package.join("probe"), b"").is_ok() {
        // Root writes anyway: nothing to test.
        let _ = std::fs::remove_file(package.join("probe"));
        mode(0o755);
        return;
    }
    let (s, v) = upload(
        &d,
        &format!("/api/design-systems/{id}/assets"),
        &[("a b.png", b"x")],
    )
    .await;
    mode(0o755);
    assert_ne!(s, 200, "{v}");
    assert!(!assets.join("a-b.png").exists(), "kept with no sent name");
    assert_eq!(ds_send(&d, &id, "a b.png", b"y").await, "a-b.png");
}

/// QA-109 (review): uploads at the same time each keep their sent name;
/// none is lost to another's write of the record.
#[tokio::test]
async fn design_system_uploads_at_once_keep_every_sent_name() {
    let d = daemon().await;
    let (id, assets) = ds_copy(&d, "airbnb", "Busy").await;
    let names: Vec<String> = (0..24).map(|i| format!("pic {i}.png")).collect();
    let sent =
        futures_util::future::join_all(names.iter().map(|n| ds_send(&d, &id, n, b"one"))).await;
    for (n, kept) in names.iter().zip(&sent) {
        assert_eq!(kept, &n.replace(' ', "-"));
    }
    for n in &names {
        assert_eq!(
            ds_send(&d, &id, n, b"two!").await,
            n.replace(' ', "-"),
            "{n}: its sent name lost"
        );
    }
    let count = std::fs::read_dir(&assets).unwrap().count();
    assert_eq!(count, names.len(), "an asset added again");
}

#[cfg(unix)] // the mock agent is a `sh` script
#[tokio::test]
async fn design_system_from_sources() {
    use base64::Engine as _;
    let d = daemon().await;
    design_agent(&d);
    let (s, v) = call(
        &d,
        "POST",
        "/api/design-systems/generate",
        Some(json!({
            "name": "Tea Site", "brief": "",
            "urls": ["https://tea.example", "javascript:alert(1)"],
            "files": [{ "name": "logo.png", "dataBase64": base64::engine::general_purpose::STANDARD.encode(PNG) }],
        })),
    )
    .await;
    assert_eq!(s, 202, "{v}");
    let job = v["job"]["id"].as_str().unwrap().to_owned();
    let done = wait(&d, &format!("/api/design-systems/jobs/{job}"), |v| {
        v["job"]["status"] != "running"
    })
    .await;
    assert_eq!(done["job"]["status"], "succeeded", "{done}");
    assert!(done["job"].get("phase").is_none());
    let kit = d.get_json("/api/design-systems/user:tea-site/kit").await;
    assert!(
        kit["palette"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["hex"] == "#123456"),
        "the agent saw the website and the attached file"
    );
    let (s, _) = call(
        &d,
        "POST",
        "/api/design-systems/generate",
        Some(json!({ "name": "X", "brief": "" })),
    )
    .await;
    assert_eq!(s, 400, "no brief and no source");
    // QA-107: two attached files kept under one name are refused, never
    // one written over the other in `source/`.
    let png = base64::engine::general_purpose::STANDARD.encode(PNG);
    let (s, v) = call(
        &d,
        "POST",
        "/api/design-systems/generate",
        Some(json!({
            "name": "Twice", "brief": "",
            "files": [
                { "name": "Hướng dẫn màu.png", "dataBase64": png },
                { "name": "a/hướng dẫn màu.png", "dataBase64": png },
            ],
        })),
    )
    .await;
    assert_eq!(s, 400, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Rename one"),
        "{v}"
    );
    let brands = d.get_json("/api/design-systems/brands").await;
    assert!(brands["brands"].as_array().unwrap().len() >= 30);
}
