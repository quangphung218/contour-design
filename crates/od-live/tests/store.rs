//! Spec 5.7 M6: a template with `<script>` is refused; a refresh updates
//! `data.json` (and the rendered preview) and appends one line to
//! `refreshes.jsonl`.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_live::model::RefreshStatus;
use od_live::{CreateInput, LiveError, LiveStore, UpdateInput};
use od_store::files::ProjectRoot;
use serde_json::{Value, json};
use std::path::Path;

fn input(v: Value) -> CreateInput {
    serde_json::from_value(v).unwrap()
}

fn setup() -> (tempfile::TempDir, ProjectRoot, LiveStore) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("p1");
    std::fs::create_dir_all(&dir).unwrap();
    let root = ProjectRoot {
        dir: dir.clone(),
        imported: false,
    };
    (tmp, root, LiveStore::new(dir, "p1"))
}

fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

const TEMPLATE: &str = r#"<!doctype html><html><body><h1>{{data.title}}</h1>
<ul><li data-od-repeat="row in data.rows">{{row.name}}: {{row.value}}</li></ul></body></html>"#;

fn kpi_input() -> CreateInput {
    input(json!({
        "title": "Weekly KPIs",
        "document": {
            "dataJson": { "title": "KPIs", "rows": [{ "name": "old", "value": 0 }] },
            "sourceJson": {
                "type": "local_file",
                "input": { "path": "data/kpis.json" },
                "outputMapping": { "dataPaths": [{ "from": "json.rows", "to": "rows" }] },
                "refreshPermission": "manual_refresh_granted_for_read_only"
            }
        }
    }))
}

#[test]
fn script_templates_are_refused_and_nothing_is_written() {
    let (_tmp, _root, store) = setup();
    for template in [
        "<div>{{data.title}}</div><script>fetch('/x')</script>",
        "<iframe src=\"https://example.com\"></iframe>",
        "<img src=x onerror=\"alert(1)\">",
    ] {
        let err = store
            .create(kpi_input(), Some(template), None, None)
            .unwrap_err();
        assert!(matches!(err, LiveError::Template(_)), "{template}: {err}");
    }
    assert!(store.list().unwrap().is_empty());
    // Nothing left behind, not even a temp folder.
    let left: Vec<_> = std::fs::read_dir(store.root())
        .map(|d| d.map(|e| e.unwrap().file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");

    // Updating an existing artifact to a script template is refused too.
    let a = store
        .create(kpi_input(), Some(TEMPLATE), None, None)
        .unwrap();
    let err = store
        .update(
            &a.id,
            UpdateInput::default(),
            Some("<script></script>"),
            None,
        )
        .unwrap_err();
    assert!(matches!(err, LiveError::Template(_)));
    assert_eq!(store.template(&a.id).unwrap(), TEMPLATE);
}

#[test]
fn refresh_updates_data_and_logs_one_line() {
    let (_tmp, root, store) = setup();
    let a = store
        .create(kpi_input(), Some(TEMPLATE), None, Some("run-1".to_owned()))
        .unwrap();
    let dir = store.paths(&a.id).unwrap().dir;
    for f in [
        "artifact.json",
        "template.html",
        "data.json",
        "index.html",
        "provenance.json",
    ] {
        assert!(dir.join(f).is_file(), "{f}");
    }
    assert!(!dir.join("refreshes.jsonl").exists());
    let preview = store.preview_html(&a.id).unwrap();
    assert!(preview.contains("<li>old: 0</li>"), "{preview}");
    assert_eq!(a.created_by_run_id.as_deref(), Some("run-1"));

    std::fs::create_dir_all(root.dir.join("data")).unwrap();
    std::fs::write(
        root.dir.join("data/kpis.json"),
        r#"{"rows":[{"name":"signups","value":42},{"name":"<b>churn</b>","value":1.5}]}"#,
    )
    .unwrap();
    let out = store.refresh(&root, &a.id).unwrap();
    assert_eq!(out.refresh.status, RefreshStatus::Succeeded);
    assert_eq!(out.artifact.refresh_status, RefreshStatus::Succeeded);
    assert!(out.artifact.last_refreshed_at.is_some());

    let data: Value =
        serde_json::from_slice(&std::fs::read(dir.join("data.json")).unwrap()).unwrap();
    assert_eq!(
        data,
        json!({ "title": "KPIs", "rows": [{ "name": "signups", "value": 42 }, { "name": "<b>churn</b>", "value": 1.5 }] })
    );
    let html = std::fs::read_to_string(dir.join("index.html")).unwrap();
    assert!(html.contains("<li>signups: 42</li>"), "{html}");
    assert!(html.contains("&lt;b&gt;churn&lt;/b&gt;: 1.5"), "{html}");
    let log = lines(&dir.join("refreshes.jsonl"));
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(log[0]["status"], "succeeded");
    assert_eq!(log[0]["artifactId"], a.id.as_str());
    assert_eq!(log[0]["refreshId"], out.refresh.id.as_str());
    assert!(!dir.join("refresh.lock").exists(), "lock released");
    let stored = store.get(&a.id).unwrap();
    assert_eq!(stored.refresh_status, RefreshStatus::Succeeded);
    let provenance: Value =
        serde_json::from_slice(&std::fs::read(dir.join("provenance.json")).unwrap()).unwrap();
    assert_eq!(provenance["generatedBy"], "refresh_runner");

    // A broken source: refresh fails, data stays, one more (failed) line.
    std::fs::write(root.dir.join("data/kpis.json"), "{not json").unwrap();
    let err = store.refresh(&root, &a.id).unwrap_err();
    assert!(matches!(err, LiveError::Source(_)), "{err}");
    let log = lines(&dir.join("refreshes.jsonl"));
    assert_eq!(log.len(), 2);
    assert_eq!(log[1]["status"], "failed");
    assert_eq!(log[1]["error"]["code"], "REFRESH_SOURCE_FAILED");
    assert_eq!(
        store.get(&a.id).unwrap().refresh_status,
        RefreshStatus::Failed
    );
    let data2: Value =
        serde_json::from_slice(&std::fs::read(dir.join("data.json")).unwrap()).unwrap();
    assert_eq!(data2, data);
    assert_eq!(store.refreshes(&a.id).unwrap().len(), 2);
}

#[test]
fn concurrent_refresh_is_refused() {
    let (_tmp, root, store) = setup();
    let a = store
        .create(kpi_input(), Some(TEMPLATE), None, None)
        .unwrap();
    let lock = store.paths(&a.id).unwrap().file("refresh.lock");
    std::fs::write(&lock, "{}").unwrap();
    let err = store.refresh(&root, &a.id).unwrap_err();
    assert!(matches!(err, LiveError::RefreshInProgress), "{err}");
    assert!(lock.exists(), "someone else's lock is left alone");
}

#[test]
fn artifacts_without_a_refreshable_source_do_not_refresh() {
    let (_tmp, root, store) = setup();
    let a = store
        .create(input(json!({ "title": "Static" })), None, None, None)
        .unwrap();
    let err = store.refresh(&root, &a.id).unwrap_err();
    assert!(matches!(err, LiveError::NotRefreshable(_)));
    assert!(store.preview_html(&a.id).unwrap().contains("<p>Static</p>"));
    // Daemon-owned fields are refused on input.
    assert!(serde_json::from_value::<CreateInput>(json!({ "title": "x", "id": "y" })).is_err());
    // Paths escaping the store are not found.
    assert!(matches!(store.get("../p1"), Err(LiveError::NotFound)));
    store.delete(&a.id).unwrap();
    assert!(store.list().unwrap().is_empty());
}
