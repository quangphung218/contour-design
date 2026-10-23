//! Crystallize (track C4-depth): a succeeded routine run becomes a skill
//! proposal and a memory proposal; applying writes them, once.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

mod common;

use axum::http::{StatusCode, header};
use common::{PROBES, write_mock};
use serde_json::{Value, json};
use std::time::Duration;

fn claude(text: &str, ok: bool) -> String {
    let result = if ok {
        r#"{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}"#
    } else {
        r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#
    };
    format!(
        r#"{PROBES}
IFS= read -r line
printf '{{"type":"system","subtype":"init","session_id":"s-%s","model":"mock"}}\n' "$$"
printf '{{"type":"assistant","message":{{"id":"m-%s","content":[{{"type":"text","text":"{text}"}}]}}}}\n' "$$"
printf '%s\n' '{result}'
cat >/dev/null
"#
    )
}

/// Runs routine `rid` now and waits for its history entry to end.
async fn run_once(d: &common::Daemon, rid: &str) -> Value {
    let res = d
        .post(&format!("/api/routines/{rid}/run"), &json!({}))
        .await;
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let id = res.json::<Value>().await.unwrap()["run"]["id"].clone();
    for _ in 0..300 {
        let runs = d.get_json(&format!("/api/routines/{rid}/runs")).await["runs"].clone();
        if let Some(r) = runs.as_array().unwrap().iter().find(|r| r["id"] == id) {
            if r["status"] != "running" {
                return r.clone();
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("routine run never ended");
}

async fn pending(d: &common::Daemon) -> Vec<Value> {
    d.get_json("/api/automation-proposals?status=pending-review")
        .await["proposals"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn a_run_crystallizes_into_a_user_skill_and_a_memory() {
    let d = common::daemon().await;
    write_mock(
        &d.mocks,
        "claude",
        &claude("Built three hero variants with a shared grid.", true),
    );
    let (project, _) = d.project().await;
    let res = d
        .post(
            "/api/routines",
            &json!({"name": "Hero refresh", "projectId": project, "agentId": "claude",
                    "prompt": "Refresh the hero section", "schedule": "@daily"}),
        )
        .await;
    let rid = res.json::<Value>().await.unwrap()["routine"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let run = run_once(&d, &rid).await;
    assert_eq!(run["status"], "succeeded");
    let run_id = run["id"].as_str().unwrap();

    let res = d
        .post(
            &format!("/api/routines/{rid}/runs/{run_id}/crystallize"),
            &json!({}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.json::<Value>().await.unwrap();
    assert_eq!(
        (body["routineId"].as_str(), body["runId"].as_str()),
        (Some(rid.as_str()), Some(run_id))
    );
    let packet = &body["packet"];
    let md = packet["bodyMarkdown"].as_str().unwrap();
    assert!(md.contains("Refresh the hero section"), "{md}");
    assert!(
        md.contains("Built three hero variants with a shared grid."),
        "the run's reply: {md}"
    );
    let proposals = body["proposals"].as_array().unwrap();
    let kinds: Vec<_> = proposals
        .iter()
        .map(|p| p["targetKind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["skill", "memory-node"]);
    assert_eq!(pending(&d).await.len(), 2);
    let packets = d.get_json("/api/automation-source-packets").await;
    assert_eq!(packets["packets"][0]["id"], packet["id"]);

    // Apply the skill: a user skill appears in the library.
    let skill_id = proposals[0]["id"].as_str().unwrap();
    let res = d
        .post(
            &format!("/api/automation-proposals/{skill_id}/apply"),
            &json!({}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let applied = res.json::<Value>().await.unwrap();
    assert_eq!(applied["proposal"]["status"], "applied");
    let slug = applied["result"]["skillId"].as_str().unwrap().to_owned();
    assert_eq!(slug, "hero-refresh-run-skill");
    let skill = d.get_json(&format!("/api/skills/{slug}")).await;
    assert_eq!(skill["source"], "user", "{skill}");
    assert!(skill.to_string().contains("## Workflow"), "{skill}");
    // Reviewed once.
    let res = d
        .post(
            &format!("/api/automation-proposals/{skill_id}/apply"),
            &json!({}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let res = d
        .post(
            &format!("/api/automation-proposals/{skill_id}/reject"),
            &json!({}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    // Apply the memory: a global memory entry.
    let memory_id = proposals[1]["id"].as_str().unwrap();
    let res = d
        .post(
            &format!("/api/automation-proposals/{memory_id}/apply"),
            &json!({}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let memories = d.get_json("/api/memory").await;
    let entry = memories["memories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| {
            m["text"]
                .as_str()
                .unwrap()
                .contains("Built three hero variants")
        })
        .expect("memory written")
        .clone();
    assert_eq!(entry["kind"], "project");
    assert!(pending(&d).await.is_empty());

    // A second crystallize of the same run gets a free skill name.
    let res = d
        .post(
            &format!("/api/routines/{rid}/runs/{run_id}/crystallize"),
            &json!({}),
        )
        .await;
    let again = res.json::<Value>().await.unwrap();
    let skill2 = again["proposals"][0]["id"].as_str().unwrap();
    let res = d
        .post(
            &format!("/api/automation-proposals/{skill2}/apply"),
            &json!({}),
        )
        .await;
    let applied = res.json::<Value>().await.unwrap();
    assert_eq!(applied["result"]["skillId"], "hero-refresh-run-skill-2");

    // Reject the memory one with a reason.
    let memory2 = again["proposals"][1]["id"].as_str().unwrap();
    let res = d
        .post(
            &format!("/api/automation-proposals/{memory2}/reject"),
            &json!({"reason": "Dismissed in Automations"}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let p = d
        .get_json(&format!("/api/automation-proposals/{memory2}"))
        .await;
    assert_eq!(p["proposal"]["status"], "rejected");
    assert_eq!(
        p["proposal"]["metadata"]["rejectedReason"],
        "Dismissed in Automations"
    );
    let all = d.get_json("/api/automation-proposals").await;
    assert_eq!(all["proposals"].as_array().unwrap().len(), 4);
    let rejected = d
        .get_json("/api/automation-proposals?status=rejected")
        .await;
    assert_eq!(rejected["proposals"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn only_succeeded_runs_crystallize_and_strangers_are_refused() {
    let d = common::daemon().await;
    write_mock(&d.mocks, "claude", &claude("broke", false));
    let (project, _) = d.project().await;
    let res = d
        .post(
            "/api/routines",
            &json!({"name": "Flaky", "projectId": project, "agentId": "claude",
                    "prompt": "p", "schedule": "@daily"}),
        )
        .await;
    let rid = res.json::<Value>().await.unwrap()["routine"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let run = run_once(&d, &rid).await;
    assert_ne!(run["status"], "succeeded");
    let run_id = run["id"].as_str().unwrap();
    let res = d
        .post(
            &format!("/api/routines/{rid}/runs/{run_id}/crystallize"),
            &json!({}),
        )
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    for path in [
        format!("/api/routines/nope/runs/{run_id}/crystallize"),
        format!("/api/routines/{rid}/runs/nope/crystallize"),
        "/api/automation-proposals/nope/apply".to_owned(),
        "/api/automation-proposals/nope/reject".to_owned(),
    ] {
        assert_eq!(
            d.post(&path, &json!({})).await.status(),
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    let res = d
        .http
        .get(format!("{}/api/automation-proposals?status=weird", d.base))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(pending(&d).await.is_empty());

    // A page from elsewhere cannot write skills through a proposal.
    let res = d
        .http
        .post(format!("{}/api/automation-proposals/nope/apply", d.base))
        .header(header::ORIGIN, "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
