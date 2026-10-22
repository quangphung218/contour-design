//! Database migration and project lifecycle at the store level.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_contracts::CreateProjectRequest;
use od_store::{DataDir, Store, db};

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
