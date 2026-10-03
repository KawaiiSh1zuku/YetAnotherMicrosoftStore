use std::{fs, path::PathBuf};

use rusqlite::Connection;
use yet_another_microsoft_store_lib::persistence::Persistence;

struct TestDatabase(PathBuf);

impl TestDatabase {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "yamstore-final-schema-{}.sqlite3",
            uuid::Uuid::new_v4()
        )))
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let _ = fs::remove_file(self.0.with_extension("sqlite3-shm"));
        let _ = fs::remove_file(self.0.with_extension("sqlite3-wal"));
    }
}

#[test]
fn initial_schema_contains_final_applicability_identity_and_event_tables() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("create final schema");
    assert_eq!(store.schema_version().expect("schema version"), 1);
    drop(store);

    let connection = Connection::open(&database.0).expect("inspect final schema");
    for table in [
        "products",
        "package_versions",
        "package_associations",
        "jobs",
        "job_events",
        "job_commands",
        "worker_leases",
        "job_targets",
    ] {
        let present: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("inspect table");
        assert_eq!(present, 1, "missing final schema table {table}");
    }

    for column in [
        "publisher",
        "resource_id",
        "package_kind",
        "minimum_os_version",
        "is_neutral",
        "content_id",
    ] {
        let present: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('package_versions') WHERE name = ?1",
                [column],
                |row| row.get(0),
            )
            .expect("inspect applicability column");
        assert_eq!(present, 1, "missing final package column {column}");
    }
}
