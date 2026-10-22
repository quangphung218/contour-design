//! Project watcher against a real folder (spec 5.5: write → `file-changed`
//! within 300 ms).

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_preview::{ChangeKind, ProjectEvent, ProjectEvents, Subscription, WatchOptions};
use std::time::{Duration, Instant};

fn native() -> ProjectEvents {
    ProjectEvents::new(WatchOptions {
        force_polling: false,
        ..WatchOptions::default()
    })
}

async fn next_change(sub: &mut Subscription, within: Duration) -> (String, ChangeKind) {
    loop {
        match tokio::time::timeout(within, sub.recv()).await {
            Ok(Some(ProjectEvent::FileChanged { path, kind })) => return (path, kind),
            Ok(Some(_)) => {}
            Ok(None) => panic!("hub closed"),
            Err(_) => panic!("no file-changed within {within:?}"),
        }
    }
}

/// Lets the native watcher settle so setup noise is not mistaken for a hit.
async fn quiet(sub: &mut Subscription) {
    while tokio::time::timeout(Duration::from_millis(400), sub.recv())
        .await
        .is_ok()
    {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn write_reaches_subscriber_within_300ms() {
    let dir = tempfile::tempdir().unwrap();
    let hub = native();
    let mut sub = hub.subscribe("p1", dir.path()).unwrap();
    quiet(&mut sub).await;

    let mut worst = Duration::ZERO;
    for round in 0..5 {
        let started = Instant::now();
        std::fs::write(dir.path().join("index.html"), format!("<p>{round}</p>")).unwrap();
        let (path, kind) = next_change(&mut sub, Duration::from_secs(2)).await;
        let took = started.elapsed();
        worst = worst.max(took);
        assert_eq!(path, "index.html");
        let expected = if round == 0 {
            ChangeKind::Add
        } else {
            ChangeKind::Change
        };
        assert_eq!(kind, expected, "round {round}");
        quiet(&mut sub).await;
    }
    eprintln!("worst write→event latency: {worst:?}");
    assert!(worst <= Duration::from_millis(300), "took {worst:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn burst_of_writes_settles_into_one_event() {
    let dir = tempfile::tempdir().unwrap();
    let hub = native();
    let mut sub = hub.subscribe("p1", dir.path()).unwrap();
    std::fs::write(dir.path().join("a.css"), "x").unwrap();
    quiet(&mut sub).await;

    for i in 0..5 {
        std::fs::write(dir.path().join("a.css"), format!("body{{order:{i}}}")).unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let (path, _) = next_change(&mut sub, Duration::from_secs(2)).await;
    assert_eq!(path, "a.css");
    let extra = tokio::time::timeout(Duration::from_millis(400), sub.recv()).await;
    assert!(
        extra.is_err(),
        "a burst must settle into a single event, got {extra:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unlink_nested_paths_and_ignored_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("deck")).unwrap();
    std::fs::create_dir_all(dir.path().join(".open-design")).unwrap();
    std::fs::write(dir.path().join("deck/slide.html"), "x").unwrap();
    let hub = native();
    let mut sub = hub.subscribe("p1", dir.path()).unwrap();
    quiet(&mut sub).await;

    std::fs::write(dir.path().join(".open-design/project.json"), "{}").unwrap();
    std::fs::write(dir.path().join(".DS_Store"), "junk").unwrap();
    std::fs::remove_file(dir.path().join("deck/slide.html")).unwrap();
    let (path, kind) = next_change(&mut sub, Duration::from_secs(2)).await;
    assert_eq!(
        (path.as_str(), kind),
        ("deck/slide.html", ChangeKind::Unlink)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(400), sub.recv())
            .await
            .is_err(),
        "internal and junk files must not be reported"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watcher_is_shared_and_closed_with_last_subscriber() {
    let dir = tempfile::tempdir().unwrap();
    let hub = native();
    let mut a = hub.subscribe("p1", dir.path()).unwrap();
    let mut b = hub.subscribe("p1", dir.path()).unwrap();
    assert_eq!(hub.watched_projects(), 1);
    quiet(&mut a).await;

    std::fs::write(dir.path().join("x.html"), "x").unwrap();
    assert_eq!(
        next_change(&mut a, Duration::from_secs(2)).await.0,
        "x.html"
    );
    assert_eq!(
        next_change(&mut b, Duration::from_secs(2)).await.0,
        "x.html"
    );

    drop(a);
    assert_eq!(hub.watched_projects(), 1);
    hub.publish(
        "p1",
        ProjectEvent::ConversationCreated {
            project_id: "p1".into(),
            conversation_id: "c1".into(),
            title: None,
            created_at: 1,
        },
    );
    let got = tokio::time::timeout(Duration::from_secs(1), b.recv())
        .await
        .unwrap();
    assert!(matches!(
        got,
        Some(ProjectEvent::ConversationCreated { .. })
    ));
    drop(b);
    assert_eq!(hub.watched_projects(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn polling_fallback_reports_changes() {
    let dir = tempfile::tempdir().unwrap();
    let hub = ProjectEvents::new(WatchOptions {
        force_polling: true,
        poll_interval: Duration::from_millis(100),
        ..WatchOptions::default()
    });
    let mut sub = hub.subscribe("p1", dir.path()).unwrap();
    std::fs::write(dir.path().join("poll.html"), "x").unwrap();
    let (path, kind) = next_change(&mut sub, Duration::from_secs(3)).await;
    assert_eq!((path.as_str(), kind), ("poll.html", ChangeKind::Add));
}
