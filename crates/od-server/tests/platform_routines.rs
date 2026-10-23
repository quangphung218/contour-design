//! Routines over HTTP (spec 1.5): a routine run is a real agent run of the
//! mock `claude`; history, no overlap, and refused inputs.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

mod common;

use axum::http::{StatusCode, header};
use common::{PROBES, write_mock};
use serde_json::{Value, json};
use std::time::Duration;

/// A claude that answers once `gate` exists.
fn gated_claude(gate: &std::path::Path) -> String {
    format!(
        r#"{PROBES}
IFS= read -r line
printf '%s\n' "$line" >> "{gate}.prompts"
while [ ! -f "{gate}" ]; do sleep 0.05; done
printf '{{"type":"system","subtype":"init","session_id":"s-%s","model":"mock"}}\n' "$$"
printf '{{"type":"assistant","message":{{"id":"m-%s","content":[{{"type":"text","text":"routine done"}}]}}}}\n' "$$"
printf '{{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}}\n'
cat >/dev/null
"#,
        gate = gate.display()
    )
}

async fn history(d: &common::Daemon, rid: &str) -> Vec<Value> {
    d.get_json(&format!("/api/routines/{rid}/runs")).await["runs"]
        .as_array()
        .unwrap()
        .clone()
}

async fn wait_status(d: &common::Daemon, rid: &str, status: &str) -> Vec<Value> {
    for _ in 0..300 {
        let h = history(d, rid).await;
        if h.first().is_some_and(|r| r["status"] == status) {
            return h;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "routine never reached {status}: {:?}",
        history(d, rid).await
    );
}

#[tokio::test]
async fn run_now_starts_an_agent_run_and_records_history() {
    let d = common::daemon().await;
    let gate = d.tmp.path().join("gate");
    write_mock(&d.mocks, "claude", &gated_claude(&gate));
    let (project, _) = d.project().await;
    let res = d
        .post(
            "/api/routines",
            &json!({"name": "Daily refresh", "projectId": project, "agentId": "claude",
                    "prompt": "Update the changelog page", "schedule": "0 9 * * 1-5"}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let routine = res.json::<Value>().await.unwrap()["routine"].clone();
    let rid = routine["id"].as_str().unwrap().to_owned();
    assert_eq!(routine["enabled"], true);
    assert!(routine["nextRunAt"].as_i64().unwrap() > od_runs::now_ms());

    let res = d
        .post(&format!("/api/routines/{rid}/run"), &json!({}))
        .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let run = res.json::<Value>().await.unwrap()["run"].clone();
    assert_eq!(
        (run["status"].as_str(), run["trigger"].as_str()),
        (Some("running"), Some("manual"))
    );
    let run_id = run["runId"].as_str().unwrap().to_owned();
    let conversation = run["conversationId"].as_str().unwrap().to_owned();

    // No overlap: a second run while the first goes is refused.
    let res = d
        .post(&format!("/api/routines/{rid}/run"), &json!({}))
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    std::fs::write(&gate, "").unwrap();
    let h = wait_status(&d, &rid, "succeeded").await;
    assert_eq!(h.len(), 1);
    assert_eq!(h[0]["runId"], run_id.as_str());
    let prompts = std::fs::read_to_string(format!("{}.prompts", gate.display())).unwrap();
    assert!(prompts.contains("Update the changelog page"));

    // The run lives in its own conversation named after the routine.
    let convs = d
        .get_json(&format!("/api/projects/{project}/conversations"))
        .await;
    let conv = convs["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == conversation.as_str())
        .unwrap()
        .clone();
    assert!(
        conv["title"]
            .as_str()
            .unwrap()
            .starts_with("Daily refresh · ")
    );
    let end = common::end_of(&d.events(&run_id, None).await).clone();
    assert_eq!(end["status"], "succeeded");

    // Switch off, edit, list, delete.
    let res = d
        .http
        .patch(format!("{}/api/routines/{rid}", d.base))
        .json(&json!({"enabled": false, "schedule": "*/5 * * * *"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let r = res.json::<Value>().await.unwrap()["routine"].clone();
    assert_eq!(
        (r["enabled"].as_bool(), r["nextRunAt"].is_null()),
        (Some(false), true)
    );
    assert_eq!(r["schedule"], "*/5 * * * *");
    assert_eq!(r["lastRun"]["status"], "succeeded");
    let all = d.get_json("/api/routines").await;
    assert_eq!(all["routines"].as_array().unwrap().len(), 1);
    let res = d
        .http
        .delete(format!("{}/api/routines/{rid}", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = d
        .http
        .get(format!("{}/api/routines/{rid}", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_routines_and_foreign_callers_are_refused() {
    let d = common::daemon().await;
    let (project, _) = d.project().await;
    let ok = json!({"name": "R", "projectId": project, "agentId": "claude", "prompt": "p", "schedule": "@daily"});
    let with = |k: &str, v: Value| {
        let mut b = ok.clone();
        b[k] = v;
        b
    };
    for (body, status) in [
        (
            with("agentId", json!("not-an-agent")),
            StatusCode::BAD_REQUEST,
        ),
        (with("projectId", json!("missing")), StatusCode::NOT_FOUND),
        (
            with("schedule", json!("every day")),
            StatusCode::BAD_REQUEST,
        ),
        (
            with("schedule", json!("* * * * * *")),
            StatusCode::BAD_REQUEST,
        ),
        (with("prompt", json!("   ")), StatusCode::BAD_REQUEST),
        (with("name", json!("")), StatusCode::BAD_REQUEST),
        (with("command", json!("rm -rf /")), StatusCode::BAD_REQUEST),
        (with("enabled", json!("yes")), StatusCode::BAD_REQUEST),
    ] {
        let res = d.post("/api/routines", &body).await;
        assert_eq!(res.status(), status, "{body}");
    }
    let res = d.post("/api/routines/nope/run", &json!({})).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = d
        .http
        .patch(format!("{}/api/routines/nope", d.base))
        .json(&json!({"enabled": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Pages from elsewhere and rebinding hosts cannot create routines.
    let port = d.base.rsplit(':').next().unwrap().to_owned();
    for (host, origin) in [
        (
            format!("127.0.0.1:{port}"),
            Some("https://evil.example".to_owned()),
        ),
        (format!("evil.example:{port}"), None),
        (
            format!("192.168.1.5:{port}"),
            Some(format!("http://192.168.1.5:{port}")),
        ),
    ] {
        let mut req = d
            .http
            .post(format!("{}/api/routines", d.base))
            .header(header::HOST, &host)
            .json(&ok);
        if let Some(o) = &origin {
            req = req.header(header::ORIGIN, o);
        }
        assert_eq!(
            req.send().await.unwrap().status(),
            StatusCode::FORBIDDEN,
            "{host} {origin:?}"
        );
    }
    assert!(
        d.get_json("/api/routines").await["routines"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
