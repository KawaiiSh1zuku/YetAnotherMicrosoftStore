use std::{
    fs,
    path::{Path, PathBuf},
};

use rusqlite::Connection;
use yet_another_microsoft_store_lib::{
    domain::{PackageKind, PackageVersion},
    error::{SafeErrorDetail, SafeField},
    persistence::Persistence,
};

struct TestDatabase {
    path: PathBuf,
}

impl TestDatabase {
    fn new(name: &str) -> Self {
        Self {
            path: std::env::temp_dir()
                .join(format!("yamstore-{name}-{}.sqlite3", uuid::Uuid::new_v4())),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.path.with_extension("sqlite3-shm"));
        let _ = fs::remove_file(self.path.with_extension("sqlite3-wal"));
    }
}

fn seed_v1_database(path: &Path) -> Connection {
    let connection = Connection::open(path).expect("create v1 database");
    connection
        .execute_batch(include_str!("../migrations/0001_m2.sql"))
        .expect("apply v1 schema");
    connection
        .pragma_update(None, "user_version", 1)
        .expect("mark v1 schema");
    connection
        .execute(
            "INSERT INTO products (
                product_id, package_family_name, title, publisher, market,
                languages_json, updated_at
             ) VALUES ('product', 'Example.App_123', 'Example', 'CN=Example',
                       'CN', '[\"zh-CN\"]', 100)",
            [],
        )
        .expect("insert v1 product");
    connection
        .execute(
            "INSERT INTO package_versions (
                update_id, product_id, package_family_name, package_moniker,
                identity_name, version, architecture, language, market, format,
                file_size, sha256, install_source
             ) VALUES (
                'update-main', 'product', 'Example.App_123', 'example-main',
                'Example.App', '1.2.3.4', 'x64', 'zh-CN', 'CN', 'msix',
                4096, 'abcdef', 'microsoft_store'
             )",
            [],
        )
        .expect("insert v1 package");
    connection
}

#[test]
fn v1_database_upgrades_to_v2_without_inventing_applicability_metadata() {
    let database = TestDatabase::new("m3-v1-upgrade");
    drop(seed_v1_database(database.path()));

    let store = Persistence::open(database.path()).expect("upgrade v1 database");
    let package = store
        .package("update-main")
        .expect("load migrated package")
        .expect("migrated package exists");

    assert_eq!(store.schema_version().expect("schema version"), 4);
    assert_eq!(package.version, PackageVersion::new(1, 2, 3, 4));
    assert_eq!(package.package_kind, PackageKind::Unknown);
    assert_eq!(package.publisher, None);
    assert_eq!(package.resource_id, None);
    assert_eq!(package.minimum_os_version, None);
    assert_eq!(package.is_neutral, None);
    assert_eq!(package.content_id, None);
}

#[test]
fn failed_v2_migration_rolls_back_columns_and_schema_version() {
    let database = TestDatabase::new("m3-v2-rollback");
    let connection = seed_v1_database(database.path());
    connection
        .execute(
            "ALTER TABLE package_versions ADD COLUMN package_kind TEXT",
            [],
        )
        .expect("create a v2 migration conflict");
    drop(connection);

    assert!(
        Persistence::open(database.path()).is_err(),
        "v2 migration should fail atomically"
    );

    let connection = Connection::open(database.path()).expect("reopen failed migration database");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read schema version");
    let publisher_columns: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('package_versions') WHERE name = 'publisher'",
            [],
            |row| row.get(0),
        )
        .expect("inspect rolled back columns");

    assert_eq!(version, 1);
    assert_eq!(publisher_columns, 0);
}

#[test]
fn v1_job_errors_are_read_with_allowlisted_legacy_details() {
    let database = TestDatabase::new("m3-v1-job-error");
    let connection = seed_v1_database(database.path());
    connection
        .execute(
            "INSERT INTO jobs (
                job_id, kind, product_id, requested_market,
                requested_architectures_json, requested_languages_json, deployment_scope,
                stage, bytes_done, requires_elevation, error_json, created_at, updated_at
             ) VALUES (
                'legacy-error', 'install', 'product', 'CN', '[\"x64\"]',
                '[\"zh-CN\"]', 'CurrentUser', 'failed', 0, 0,
                '{\"code\":\"catalog_unavailable\",\"messageKey\":\"errors.catalogUnavailable\",\"retry\":\"re_resolve\",\"details\":[{\"key\":\"field\",\"value\":\"packageUri\"},{\"key\":\"server\",\"value\":\"secret\"}]}',
                100, 200
             )",
            [],
        )
        .expect("insert v1 error-bearing job");
    drop(connection);

    let store = Persistence::open(database.path()).expect("upgrade v1 database");
    let job = store
        .job("legacy-error")
        .expect("read migrated job")
        .expect("legacy job exists");
    let details = job.error.expect("legacy error is retained").details;

    assert_eq!(
        details,
        vec![
            SafeErrorDetail::Field {
                field: SafeField::PackageUri,
            },
            SafeErrorDetail::Redacted,
        ]
    );
}
