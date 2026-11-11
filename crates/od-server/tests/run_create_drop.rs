//! `POST /api/runs` whose client goes away (or cancels) between registering
//! the run and launching the agent.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

mod common;

use axum::body::Body;
use axum::http::{Request, header};
use common::*;
use serde_json::json;
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;
use tower::ServiceExt;

/// Hangs after init until killed.
const SLOW: &str = r#"
IFS= read -r line
printf '{"type":"system","subtype":"init","session_id":"s","model":"mock"}\n'
sleep 300
"#;

const HELLO: &str = r#"
IFS= read -r line
printf '{"type":"system","subtype":"init","session_id":"s","model":"mock"}\n'
printf '{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"hello"}]}}\n'
printf '{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","usage":{"input_tokens":1}}\n'
"#;

type Pending = Pin<
    Box<dyn Future<Output = Result<axum::response::Response, std::convert::Infallible>> + Send>,
>;

/// Sends `POST /api/runs` in process and polls it only until the run is
/// registered; returns the half-done request future and the run id.
async fn create_until_registered(d: &Daemon, project: &str) -> (Pending, String) {
    let body = json!({"agentId": "claude", "projectId": project, "message": "go"});
    let mut req = Request::post("/api/runs")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    req.extensions_mut().insert(axum::extract::ConnectInfo(
        "127.0.0.1:1".parse::<std::net::SocketAddr>().unwrap(),
    ));
    let mut fut: Pending = Box::pin(od_server::router(d.state.clone()).oneshot(req));
    for _ in 0..2000 {
        let polled = std::future::poll_fn(|cx| Poll::Ready(fut.as_mut().poll(cx))).await;
        assert!(polled.is_pending(), "the request finished before the test");
        if let Some(run) = d.state.runs.registry.list().first() {
            return (fut, run.id().to_owned());
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    panic!("the run was never registered");
}

async fn end_status(d: &Daemon, run_id: &str) -> String {
    let events = tokio::time::timeout(Duration::from_secs(10), d.events(run_id, None))
        .await
        .expect("the run never ended");
    let end = events.last().unwrap();
    assert_eq!(end.event, "end");
    end.data["status"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn client_gone_after_the_run_is_registered_still_ends_the_run() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &format!("{PROBES}{HELLO}"));
    let (project, conversation) = d.project().await;
    let (fut, run_id) = create_until_registered(&d, &project).await;
    drop(fut); // the connection closed: axum drops the handler future
    assert_eq!(end_status(&d, &run_id).await, "succeeded");
    let messages = d.settled_messages(&project, &conversation).await;
    assert!(
        messages.iter().all(|m| m["runStatus"] != json!("running")),
        "{messages:?}"
    );
}

#[tokio::test]
async fn cancel_before_launch_is_not_lost() {
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &format!("{PROBES}{SLOW}"));
    let (project, conversation) = d.project().await;
    let (fut, run_id) = create_until_registered(&d, &project).await;
    let res = d
        .post(&format!("/api/runs/{run_id}/cancel"), &json!({}))
        .await;
    assert_eq!(res.status().as_u16(), 200);
    let _ = fut.await; // the create request completes after the cancel
    assert_eq!(end_status(&d, &run_id).await, "canceled");
    let messages = d.settled_messages(&project, &conversation).await;
    let assistant = messages.iter().find(|m| m["role"] == "assistant").unwrap();
    assert_eq!(assistant["runStatus"], "canceled", "{assistant}");
}

#[tokio::test]
async fn shutdown_before_launch_is_not_lost() {
    // QA-026: quitting while a run is still being prepared stops it too.
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &format!("{PROBES}{SLOW}"));
    let (project, conversation) = d.project().await;
    let (fut, run_id) = create_until_registered(&d, &project).await;
    tokio::time::timeout(Duration::from_secs(10), d.state.runs.stop_all())
        .await
        .expect("stop_all did not return");
    // Told as Contour closing, not as a Stop the user never pressed.
    let reg = d.state.runs.registry.get(&run_id).unwrap();
    assert_eq!(reg.status(), od_contracts::RunStatus::Failed);
    assert_eq!(
        reg.end_payload().and_then(|e| e.failure_category),
        Some(od_contracts::FailureCategory::DaemonRestart)
    );
    // The turn is recorded as ended before stop_all returns.
    let body = d
        .get_json(&format!(
            "/api/projects/{project}/conversations/{conversation}/messages"
        ))
        .await;
    let assistant = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["role"] == "assistant")
        .cloned()
        .unwrap();
    assert_eq!(assistant["runStatus"], "failed", "{assistant}");
    let _ = fut.await;
}

#[tokio::test]
async fn a_stop_before_launch_stays_a_stop_when_contour_quits() {
    // The user pressed Stop while the run was being prepared, then quit:
    // the turn is theirs, "Stopped", not cut off by Contour closing.
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &format!("{PROBES}{SLOW}"));
    let (project, _) = d.project().await;
    let (fut, run_id) = create_until_registered(&d, &project).await;
    let res = d
        .post(&format!("/api/runs/{run_id}/cancel"), &json!({}))
        .await;
    assert_eq!(res.status().as_u16(), 200);
    tokio::time::timeout(Duration::from_secs(10), d.state.runs.stop_all())
        .await
        .expect("stop_all did not return");
    let _ = fut.await;
    assert_eq!(end_status(&d, &run_id).await, "canceled");
    let reg = d.state.runs.registry.get(&run_id).unwrap();
    assert_eq!(reg.end_payload().and_then(|e| e.failure_category), None);
}

#[tokio::test]
async fn a_stop_stays_a_stop_when_contour_quits_while_the_launch_step_ends_it() {
    // As above, with the quit landing while the launch step, having seen
    // the Stop, still writes the run's end: a test DB lock holds it there.
    let d = daemon().await;
    write_mock(&d.mocks, "claude", &format!("{PROBES}{SLOW}"));
    let (project, _) = d.project().await;
    let (fut, run_id) = create_until_registered(&d, &project).await;
    // The first store write after the step's check (dropping the run's
    // GenUI surfaces) waits on this lock, up to SQLite's busy timeout.
    let genui =
        rusqlite::Connection::open(d.state.store.data_dir().root().join("genui.sqlite")).unwrap();
    genui.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let res = d
        .post(&format!("/api/runs/{run_id}/cancel"), &json!({}))
        .await;
    assert_eq!(res.status().as_u16(), 200);
    let create = tokio::spawn(fut);
    // Gives the step time to reach the lock. Nothing tells from outside
    // when it has, unless it lets go of the Stop there (the bug), so the
    // wait ends early only then.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while d.state.runs.pending_cancels().contains(&run_id) && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let stop = tokio::spawn({
        let state = d.state.clone();
        async move { state.runs.stop_all().await }
    });
    // `stop_all` has sorted the runs once it closes the task set; it then
    // waits for the launch step, which waits for the lock.
    for _ in 0..2000 {
        if d.state.runs.tasks.is_closed() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(d.state.runs.tasks.is_closed(), "stop_all never ran");
    genui.execute_batch("COMMIT").unwrap();
    tokio::time::timeout(Duration::from_secs(10), stop)
        .await
        .expect("stop_all did not return")
        .unwrap();
    let _ = create.await;
    let reg = d.state.runs.registry.get(&run_id).unwrap();
    assert_eq!(reg.status(), od_contracts::RunStatus::Canceled);
    assert_eq!(reg.end_payload().and_then(|e| e.failure_category), None);
}
