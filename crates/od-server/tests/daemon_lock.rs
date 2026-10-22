//! One daemon per data dir: a second one must not touch the first one's runs.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

use od_runs::{RunMeta, Runs};
use od_runtime::process::{self, SpawnSpec, group_alive};
use od_server::{ServeOptions, serve};
use od_store::DataDir;
use std::path::Path;
use std::time::Duration;

fn options(data: &Path, tmp: &Path) -> ServeOptions {
    ServeOptions {
        host: "127.0.0.1".to_owned(),
        port: 0,
        data_dir: DataDir::new(data),
        resource_root: tmp.join("no-content"),
        api_token: None,
        allowed_origins: Vec::new(),
    }
}

async fn wait_for_lock_url(data: &Path) -> Option<String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(data.join("daemon.lock")) {
            if text.contains("http://") {
                return Some(text);
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

#[tokio::test]
async fn second_daemon_on_the_same_data_dir_leaves_the_first_ones_agent_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");

    let first = tokio::spawn(serve(options(&data, tmp.path())));
    let first_url = wait_for_lock_url(&data).await;

    // The first daemon's live run: an agent in its own process group.
    let mut agent = process::spawn(&SpawnSpec {
        program: "/bin/sleep".into(),
        args: vec!["30".into()],
        cwd: tmp.path().to_path_buf(),
        env: Vec::new(),
    })
    .unwrap();
    let pgid = i32::try_from(agent.id().unwrap()).unwrap();
    let runs = Runs::new(data.join("runs"));
    let (run, _) = runs
        .create_or_reuse(RunMeta {
            run_id: "live".into(),
            agent_id: "claude".into(),
            ..RunMeta::default()
        })
        .unwrap();
    run.set_process(Some(pgid), Some("/bin/sleep".into()));

    let second =
        tokio::time::timeout(Duration::from_secs(3), serve(options(&data, tmp.path()))).await;
    let err = match second {
        Ok(Err(err)) => format!("{err:#}"),
        Ok(Ok(())) => panic!("the second daemon served"),
        Err(_) => panic!("the second daemon started next to the first"),
    };
    assert!(group_alive(pgid), "the second daemon killed the agent");
    assert!(err.contains("already running"), "{err}");
    let first_url = first_url.expect("the first daemon recorded its url");
    assert!(err.contains(first_url.trim()), "{err}");

    first.abort();
    let _ = agent.kill().await;
}

#[tokio::test]
async fn the_lock_is_released_when_the_daemon_stops() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let first = tokio::spawn(serve(options(&data, tmp.path())));
    assert!(wait_for_lock_url(&data).await.is_some());
    first.abort();
    let _ = first.await;
    let second = tokio::spawn(serve(options(&data, tmp.path())));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!second.is_finished(), "{:?}", second.await);
    second.abort();
}
