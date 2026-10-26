//! Project watcher against a real folder (spec 5.5: write → `file-changed`
//! within 300 ms).

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_preview::{ChangeKind, ProjectEvent, ProjectEvents, Subscription, WatchOptions};
use std::time::{Duration, Instant};

/// One test at a time: these time a watcher, and six of them at once on a
/// small CI runner took the median write→event latency from ~220 ms to
/// ~300+ ms, with FSEvents still delivering each write within 20 ms.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let hub = native();
    let mut sub = hub.subscribe("p1", dir.path()).unwrap();
    quiet(&mut sub).await;

    // Why the median, not the worst: 200 ms of the 300 ms budget is the
    // debounce window (spec 5.5), so a single scheduler stall of ~100 ms on a
    // shared CI runner fails a worst-case check (macos-15 hit 315 ms once;
    // locally every round is ~230 ms). A slower pipeline (a longer window, the
    // 500 ms polling fallback) delays every round, so the median still
    // catches it. Each round must also wait out the window: no raw events.
    let debounce = WatchOptions::default().debounce;
    let mut took = Vec::new();
    for round in 0..7 {
        let started = Instant::now();
        std::fs::write(dir.path().join("index.html"), format!("<p>{round}</p>")).unwrap();
        let (path, kind) = next_change(&mut sub, Duration::from_secs(2)).await;
        took.push(started.elapsed());
        assert_eq!(path, "index.html");
        let expected = if round == 0 {
            ChangeKind::Add
        } else {
            ChangeKind::Change
        };
        assert_eq!(kind, expected, "round {round}");
        quiet(&mut sub).await;
    }
    eprintln!("write→event latency per round: {took:?}");
    assert!(
        took.iter().all(|t| *t >= debounce),
        "faster than the debounce window: {took:?}"
    );
    took.sort_unstable();
    let median = took[took.len() / 2];
    assert!(
        median <= Duration::from_millis(300),
        "median {median:?} of {took:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn burst_of_writes_settles_into_one_event() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let hub = native();
    let mut sub = hub.subscribe("p1", dir.path()).unwrap();
    std::fs::write(dir.path().join("a.css"), "x").unwrap();
    quiet(&mut sub).await;

    // Why three tries: under heavy I/O, FSEvents can hand over a write after
    // the debouncer already flushed, which splits one burst in two. That is
    // the OS, not the debouncer (the pre-merge check saw it once at 740f0b5; it
    // never reproduced in 93 runs, idle or at load 24). Without debouncing
    // every try yields several events, so the test still fails: a 25 ms
    // DEBOUNCE splits all three. The trade-off: a debouncer that splits
    // only some bursts (fewer than 2 in 3) would slip through.
    let mut extras = Vec::new();
    for attempt in 0..3 {
        for i in 0..5 {
            let css = format!("body{{order:{attempt}{i}}}");
            std::fs::write(dir.path().join("a.css"), css).unwrap();
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        let (path, _) = next_change(&mut sub, Duration::from_secs(2)).await;
        assert_eq!(path, "a.css");
        match tokio::time::timeout(Duration::from_millis(400), sub.recv()).await {
            Err(_) => return,
            Ok(extra) => extras.push(extra),
        }
        quiet(&mut sub).await;
    }
    panic!("a burst must settle into a single event, got extra {extras:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unlink_nested_paths_and_ignored_files() {
    let _serial = SERIAL.lock().await;
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

/// The daemon reads a file to serve the preview; on Linux inotify reports
/// that open, and a reload per read would reload the preview forever.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reading_a_file_is_not_a_change() {
    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "x").unwrap();
    let hub = native();
    let mut sub = hub.subscribe("p1", dir.path()).unwrap();
    quiet(&mut sub).await;

    for _ in 0..3 {
        assert_eq!(
            std::fs::read_to_string(dir.path().join("index.html")).unwrap(),
            "x"
        );
    }
    let got = tokio::time::timeout(Duration::from_millis(600), sub.recv()).await;
    assert!(got.is_err(), "a read was reported: {got:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watcher_is_shared_and_closed_with_last_subscriber() {
    let _serial = SERIAL.lock().await;
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
    let _serial = SERIAL.lock().await;
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
