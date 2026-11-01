//! Database migration and project lifecycle at the store level.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_contracts::CreateProjectRequest;
use od_store::{DataDir, Store, db};
use serde_json::json;

fn open(dir: &std::path::Path) -> Store {
    Store::open(DataDir::new(dir)).unwrap()
}

#[test]
fn migrations_set_user_version_and_create_core_tables() {
    let tmp = tempfile::tempdir().unwrap();
    let store = open(tmp.path());
    store
        .with_conn(|conn| {
            let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            assert_eq!(usize::try_from(version).unwrap(), db::schema_version());
            let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            assert_eq!(mode, "wal");
            for table in db::CORE_TABLES {
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |r| r.get(0),
                )?;
                assert_eq!(n, 1, "table {table}");
            }
            Ok(())
        })
        .unwrap();
    drop(store);
    // Re-opening an up-to-date database is a no-op.
    let again = open(tmp.path());
    assert!(again.list_projects().unwrap().is_empty());
}

#[test]
fn newer_schema_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    drop(open(tmp.path()));
    let conn = rusqlite::Connection::open(tmp.path().join("app.sqlite")).unwrap();
    conn.execute_batch("PRAGMA user_version = 999").unwrap();
    drop(conn);
    assert!(Store::open(DataDir::new(tmp.path())).is_err());
}

#[test]
fn deleting_a_project_cascades_to_conversations() {
    let tmp = tempfile::tempdir().unwrap();
    let store = open(tmp.path());
    let (project, _) = store
        .create_project(CreateProjectRequest {
            name: "P".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    store
        .with_conn(|c| {
            assert_eq!(db::count_conversations(c, &project.id)?, 1);
            Ok(())
        })
        .unwrap();
    store.delete_project(&project.id).unwrap();
    store
        .with_conn(|c| {
            assert_eq!(db::count_conversations(c, &project.id)?, 0);
            Ok(())
        })
        .unwrap();
}

#[test]
fn usage_keeps_the_first_mark_and_counts_turns_since_recording_began() {
    use od_store::NewTurn;
    use od_store::usage::UsageMark;
    let tmp = tempfile::tempdir().unwrap();
    let store = open(tmp.path());
    let (project, conversation) = store
        .create_project(CreateProjectRequest {
            name: "P".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    // Made before recording began: left out.
    let (old, _) = store
        .create_project(CreateProjectRequest {
            name: "Old".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    store
        .with_conn(|c| {
            c.execute(
                "UPDATE projects SET created_at = 0 WHERE id = ?1",
                [&old.id],
            )?;
            Ok(())
        })
        .unwrap();

    assert!(
        store
            .mark_usage(&project.id, UsageMark::FirstPreview)
            .unwrap()
    );
    assert!(
        !store
            .mark_usage(&project.id, UsageMark::FirstPreview)
            .unwrap()
    );
    assert!(!store.mark_usage("missing", UsageMark::Export).unwrap());
    assert!(!store.mark_usage("../x", UsageMark::Export).unwrap());
    for i in 0..2 {
        store
            .begin_turn(&NewTurn {
                conversation_id: &conversation,
                user_message_id: &format!("u{i}"),
                assistant_message_id: &format!("a{i}"),
                content: "hi",
                attachments: &[],
                agent_id: "claude",
                run_id: &format!("r{i}"),
            })
            .unwrap();
    }

    let (since, rows) = store.project_usage().unwrap();
    assert!(since > 0 && since <= project.created_at);
    assert_eq!(rows.len(), 1);
    let first = store
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT at FROM usage_marks WHERE kind = 'first_preview'",
                [],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .unwrap();
    assert_eq!(rows[0].first_preview_at, Some(first));
    assert!(!rows[0].exported);
    assert_eq!(rows[0].turns, 2);

    store.mark_usage(&project.id, UsageMark::Export).unwrap();
    assert!(store.project_usage().unwrap().1[0].exported);

    // The stamp survives a reopen; the marks go with their project.
    drop(store);
    let store = open(tmp.path());
    assert_eq!(store.project_usage().unwrap().0, since);
    store.delete_project(&project.id).unwrap();
    assert!(store.project_usage().unwrap().1.is_empty());
}

#[test]
fn usage_counts_only_projects_people_made_empty() {
    use od_contracts::ProjectMetadata;
    let tmp = tempfile::tempdir().unwrap();
    let store = open(tmp.path());
    let (made, _) = store
        .create_project(CreateProjectRequest {
            name: "Made".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    store
        .create_file_exact(&made.id, "index.html", b"<p>hi</p>")
        .unwrap();
    store
        .create_project(CreateProjectRequest {
            name: "Imported".to_owned(),
            metadata: Some(ProjectMetadata {
                imported_from: Some("claude-design".to_owned()),
                ..ProjectMetadata::default()
            }),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    let copy = store.duplicate_project(&made.id, None).unwrap();
    assert_eq!(copy.copied_files, ["index.html"]);
    // From a saved template (`use_template`), the media library, and an
    // automation's project for one run.
    for metadata in [
        json!({ "templateId": "t1" }),
        json!({ "intent": "media-library" }),
    ] {
        store
            .create_project(CreateProjectRequest {
                name: "Not made here".to_owned(),
                metadata: Some(serde_json::from_value(metadata).unwrap()),
                ..CreateProjectRequest::default()
            })
            .unwrap();
    }
    store
        .create_automation_project(CreateProjectRequest {
            name: "Routine · today".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();

    let (_, rows) = store.project_usage().unwrap();
    assert_eq!(rows.len(), 1, "only the project made here counts");
    assert_eq!(rows[0].created_at, made.created_at);
}

#[test]
fn setup_records_each_step_once_in_a_new_data_folder() {
    use od_contracts::SetupStep;
    let tmp = tempfile::tempdir().unwrap();
    let store = open(tmp.path());
    let steps = |s: &Store| -> Vec<SetupStep> {
        s.setup_marks()
            .unwrap()
            .unwrap()
            .iter()
            .map(|m| m.step)
            .collect()
    };
    assert_eq!(steps(&store), [SetupStep::Opened], "the first open counts");
    let opened = store.setup_marks().unwrap().unwrap()[0].at;

    assert!(store.mark_setup(SetupStep::AgentFound).unwrap());
    let found = store.setup_marks().unwrap().unwrap()[1].at;
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(
        !store.mark_setup(SetupStep::AgentFound).unwrap(),
        "written once"
    );
    assert_eq!(
        store.setup_marks().unwrap().unwrap()[1].at,
        found,
        "never overwritten"
    );

    // The media library is nobody's first project; a project made here is.
    store
        .create_project(CreateProjectRequest {
            name: "Media".to_owned(),
            metadata: Some(serde_json::from_value(json!({ "intent": "media-library" })).unwrap()),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    assert!(!steps(&store).contains(&SetupStep::Project));
    // Nor is an automation's project for one run.
    store
        .create_automation_project(CreateProjectRequest {
            name: "Routine · today".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    assert!(
        !steps(&store).contains(&SetupStep::Project),
        "an automation made it"
    );
    let (project, _) = store
        .create_project(CreateProjectRequest {
            name: "First".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    store.mark_setup(SetupStep::Preview).unwrap();
    // Deleting the first project keeps its mark; reopening stamps nothing new.
    store.delete_project(&project.id).unwrap();
    drop(store);
    let again = open(tmp.path());
    assert_eq!(
        steps(&again),
        [
            SetupStep::Opened,
            SetupStep::AgentFound,
            SetupStep::Project,
            SetupStep::Preview
        ],
        "in step order"
    );
    assert_eq!(again.setup_marks().unwrap().unwrap()[0].at, opened);
}

#[test]
fn setup_is_not_recorded_in_a_data_folder_from_before_it() {
    use od_contracts::SetupStep;
    let tmp = tempfile::tempdir().unwrap();
    // A 0.2.0-beta.7 database: the schema without the setup tables.
    drop(open(tmp.path()));
    let conn = rusqlite::Connection::open(tmp.path().join("app.sqlite")).unwrap();
    conn.execute_batch("DROP TABLE setup_marks; DROP TABLE setup_since;")
        .unwrap();
    drop(conn);
    let store = open(tmp.path());
    assert_eq!(store.setup_marks().unwrap(), None);
    assert!(
        !store.mark_setup(SetupStep::AgentFound).unwrap(),
        "no guess"
    );
    store
        .create_project(CreateProjectRequest {
            name: "P".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    drop(store);
    assert_eq!(open(tmp.path()).setup_marks().unwrap(), None, "for good");
}

/// Recent projects follow the user's last send (spec L6): a rename keeps
/// the order, a send moves the project up, a late send of an earlier
/// (queued) message never moves it back.
#[test]
fn only_a_send_moves_a_project_up_recent_projects() {
    let tmp = tempfile::tempdir().unwrap();
    let store = open(tmp.path());
    let create = |name: &str| {
        store
            .create_project(CreateProjectRequest {
                name: name.to_owned(),
                ..CreateProjectRequest::default()
            })
            .unwrap()
            .0
    };
    let (a, b) = (create("A"), create("B"));
    store
        .with_conn(|c| {
            c.execute(
                "UPDATE projects SET updated_at = 1000 WHERE id = ?1",
                [&a.id],
            )?;
            c.execute(
                "UPDATE projects SET updated_at = 2000 WHERE id = ?1",
                [&b.id],
            )?;
            Ok(())
        })
        .unwrap();
    let order = || {
        store
            .list_projects()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(order(), ["B", "A"]);

    let renamed = store
        .update_project(
            &a.id,
            serde_json::from_value(json!({ "name": "A2", "pendingPrompt": "" })).unwrap(),
        )
        .unwrap();
    assert_eq!(renamed.updated_at, 1000);
    assert_eq!(order(), ["B", "A2"]);

    store.mark_project_sent(&a.id, 3000).unwrap();
    assert_eq!(order(), ["A2", "B"]);
    store.mark_project_sent(&a.id, 2500).unwrap();
    assert_eq!(store.get_project(&a.id).unwrap().updated_at, 3000);
    assert!(store.mark_project_sent("missing", 1).is_err());
}

/// A project read before a send and written after it (a Motion render
/// setting `entryFile`, a critique toggle) keeps the send's time.
#[test]
fn a_stale_project_write_does_not_take_a_send_back() {
    let tmp = tempfile::tempdir().unwrap();
    let store = open(tmp.path());
    let (project, _) = store
        .create_project(CreateProjectRequest {
            name: "A".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    let mut stale = store.get_project(&project.id).unwrap();
    let sent = stale.updated_at + 60_000;
    store.mark_project_sent(&project.id, sent).unwrap();
    stale.name = "Renamed".to_owned();
    let written = store.with_conn(|c| db::update_project(c, &stale)).unwrap();
    // The write answers with the stored time, not the stale one it was given.
    assert_eq!(written, Some(sent));
    let now = store.get_project(&project.id).unwrap();
    assert_eq!(now.name, "Renamed");
    assert_eq!(now.updated_at, sent);
}

#[test]
fn updating_a_missing_project_reports_it_gone() {
    let tmp = tempfile::tempdir().unwrap();
    let store = open(tmp.path());
    let (project, _) = store
        .create_project(CreateProjectRequest {
            name: "A".to_owned(),
            ..CreateProjectRequest::default()
        })
        .unwrap();
    let mut gone = store.get_project(&project.id).unwrap();
    gone.id = "missing".to_owned();
    assert_eq!(
        store.with_conn(|c| db::update_project(c, &gone)).unwrap(),
        None
    );
}
