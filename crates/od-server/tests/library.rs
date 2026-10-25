//! Track B1-library routes against a real daemon on loopback: skill CRUD
//! and overrides, plugin install/use/uninstall and project-bound plugins,
//! saved templates and Remix, the media library, and design-system kit,
//! import, copy, status, assets, archive, token rebuild and revisions
//! (mock agent). No request leaves loopback.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

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
        if let Ok(t) = std::fs::read_to_string(path) {
            if !t.is_empty() {
                return t;
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{} was not written", path.display());
}

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

    let all = d.get_json("/api/library/assets").await;
    let paths: Vec<&str> = all["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths.len(), 3, "{paths:?}");
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
        &[("logo.png", b"PNG")],
    )
    .await;
    unchanged(&logo);
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
    let (s, _) = upload(
        &d,
        &format!("/api/design-systems/{id}/assets"),
        &[("x.sh", b"echo")],
    )
    .await;
    assert_eq!(s, 400);
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
    let brands = d.get_json("/api/design-systems/brands").await;
    assert!(brands["brands"].as_array().unwrap().len() >= 30);
}
