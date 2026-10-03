use std::{
    fs,
    path::{Path, PathBuf},
};

use yet_another_microsoft_store_lib::{
    deployment::DeploymentScope,
    domain::{
        AppSettings, Architecture, CacheEntry, CacheState, DependencyKind, DiagnosticEvent,
        DiagnosticOperation, InstallObservation, InstallSource, PackageDependency, PackageFormat,
        PackageKind, PackageRecord, PackageVersion, ProductRecord, ProxyCredentialPolicy,
        ProxyMode,
    },
    error::ErrorCode,
    jobs::{Job, JobKind, JobStage, RecoveryAction},
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

fn product() -> ProductRecord {
    ProductRecord {
        product_id: "9WZDNCRFJ3Q8".to_owned(),
        package_family_name: Some("Example.App_123".to_owned()),
        title: Some("Example".to_owned()),
        publisher: Some("CN=Example".to_owned()),
        market: "CN".to_owned(),
        languages: vec!["zh-CN".to_owned(), "en-US".to_owned()],
        updated_at: 100,
    }
}

fn package() -> PackageRecord {
    PackageRecord {
        update_id: "update-main".to_owned(),
        product_id: "9WZDNCRFJ3Q8".to_owned(),
        package_family_name: Some("Example.App_123".to_owned()),
        package_moniker: "example-main".to_owned(),
        identity_name: Some("Example.App".to_owned()),
        publisher: Some("CN=Example".to_owned()),
        resource_id: None,
        package_kind: PackageKind::Main,
        version: PackageVersion::new(1, 2, 3, 4),
        architecture: Architecture::X64,
        language: Some("zh-CN".to_owned()),
        market: "CN".to_owned(),
        format: PackageFormat::MsixBundle,
        minimum_os_version: Some(PackageVersion::new(10, 0, 19045, 0)),
        is_neutral: Some(true),
        content_id: Some("content-main".to_owned()),
        file_size: Some(4096),
        sha256: Some("abcdef".to_owned()),
        install_source: InstallSource::MicrosoftStore,
    }
}

fn job(job_id: &str, stage: JobStage) -> Job {
    Job {
        job_id: job_id.to_owned(),
        kind: JobKind::Install,
        product_id: "9WZDNCRFJ3Q8".to_owned(),
        requested_market: "CN".to_owned(),
        requested_architectures: vec![Architecture::X64],
        requested_languages: vec!["zh-CN".to_owned()],
        deployment_scope: DeploymentScope::AllUsers,
        selected_update_id: Some("update-main".to_owned()),
        package_family_name: Some("Example.App_123".to_owned()),
        stage,
        bytes_done: 512,
        bytes_total: Some(1024),
        version: Some("1.2.3.4".to_owned()),
        architecture: Some(Architecture::X64),
        language: Some("zh-CN".to_owned()),
        requires_elevation: false,
        error: None,
        created_at: 100,
        updated_at: 200,
    }
}

#[test]
fn schema_migration_is_replayable() {
    let database = TestDatabase::new("migration-replay");

    let first = Persistence::open(database.path()).expect("first migration should succeed");
    assert_eq!(first.schema_version().expect("schema version"), 3);
    drop(first);

    let reopened = Persistence::open(database.path()).expect("migration replay should succeed");
    assert_eq!(reopened.schema_version().expect("schema version"), 3);
}

#[test]
fn failed_migration_rolls_back_schema_and_version() {
    let database = TestDatabase::new("migration-rollback");
    let connection = rusqlite::Connection::open(database.path()).expect("seed database");
    connection
        .execute_batch(
            "CREATE TABLE jobs (sentinel TEXT NOT NULL);
             INSERT INTO jobs (sentinel) VALUES ('preserve-me');",
        )
        .expect("seed conflicting table");
    drop(connection);

    assert!(
        Persistence::open(database.path()).is_err(),
        "migration should fail atomically"
    );

    let connection = rusqlite::Connection::open(database.path()).expect("reopen database");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read schema version");
    let product_table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'products'",
            [],
            |row| row.get(0),
        )
        .expect("inspect schema");
    let sentinel: String = connection
        .query_row("SELECT sentinel FROM jobs", [], |row| row.get(0))
        .expect("read preexisting data");

    assert_eq!(version, 0);
    assert_eq!(product_table_count, 0);
    assert_eq!(sentinel, "preserve-me");
}

#[test]
fn repositories_round_trip_domain_records() {
    let database = TestDatabase::new("round-trip");
    let store = Persistence::open(database.path()).expect("open database");
    let product = product();
    let package = package();
    let dependencies = vec![PackageDependency {
        source_update_id: "update-main".to_owned(),
        target_update_id: "framework-vclibs".to_owned(),
        kind: DependencyKind::Prerequisite,
    }];
    let cache = CacheEntry {
        cache_key: "update-main:x64:zh-CN:abcdef".to_owned(),
        job_id: Some("job-cache".to_owned()),
        update_id: "update-main".to_owned(),
        path: r"E:\Cache\example.msixbundle".to_owned(),
        size: 4096,
        sha256: "abcdef".to_owned(),
        state: CacheState::Verified,
        last_accessed_at: 120,
    };
    let settings = AppSettings {
        region: "CN".to_owned(),
        market: "CN".to_owned(),
        preferred_architectures: vec![Architecture::X64, Architecture::Arm64],
        preferred_languages: vec!["zh-CN".to_owned(), "en-US".to_owned()],
        proxy_mode: ProxyMode::Https,
        proxy_host: Some("proxy.example.test".to_owned()),
        proxy_port: Some(8443),
        proxy_credentials: ProxyCredentialPolicy::WindowsCredentialManager,
        cache_enabled: true,
        cache_directory: r"E:\Cache".to_owned(),
        max_cache_bytes: 10 * 1024 * 1024,
        retention_days: 30,
        keep_installed_payloads: false,
        max_concurrent_downloads: 3,
    };
    let observation = InstallObservation {
        package_family_name: "Example.App_123".to_owned(),
        product_id: Some("9WZDNCRFJ3Q8".to_owned()),
        source: InstallSource::MicrosoftStore,
        observed_at: 130,
    };
    let diagnostic = DiagnosticEvent {
        job_id: Some("job-cache".to_owned()),
        code: ErrorCode::DownloadFailed,
        stage: Some(JobStage::Downloading),
        operation: DiagnosticOperation::Download,
        os_error_code: Some(12029),
        retryable: true,
        occurred_at: 140,
    };

    store.upsert_product(&product).expect("save product");
    store.upsert_package(&package).expect("save package");
    store
        .replace_dependencies("update-main", &dependencies)
        .expect("save dependencies");
    store.upsert_cache_entry(&cache).expect("save cache");
    store.save_settings(&settings).expect("save settings");
    store
        .record_install_observation(&observation)
        .expect("save install source");
    store
        .record_diagnostic(&diagnostic)
        .expect("save diagnostic");

    assert_eq!(
        store.product("9WZDNCRFJ3Q8").expect("load product"),
        Some(product)
    );
    assert_eq!(
        store.package("update-main").expect("load package"),
        Some(package)
    );
    assert_eq!(
        store
            .dependencies("update-main")
            .expect("load dependencies"),
        dependencies
    );
    assert_eq!(
        store
            .cache_entry("update-main:x64:zh-CN:abcdef")
            .expect("load cache"),
        Some(cache)
    );
    assert_eq!(store.settings().expect("load settings"), Some(settings));
    assert_eq!(
        store
            .install_observation("Example.App_123")
            .expect("load install source"),
        Some(observation)
    );
    assert_eq!(
        store
            .diagnostic_count(ErrorCode::DownloadFailed)
            .expect("count diagnostics"),
        1
    );
}

#[test]
fn restart_recovery_is_persisted_before_jobs_are_returned() {
    let database = TestDatabase::new("restart-recovery");
    let store = Persistence::open(database.path()).expect("open database");
    store
        .save_job(&job("job-download", JobStage::Downloading))
        .expect("save download job");
    store
        .save_job(&job("job-deploy", JobStage::Deploying))
        .expect("save deployment job");
    drop(store);

    let reopened = Persistence::open(database.path()).expect("reopen database");
    let actions = reopened
        .recover_jobs_after_restart(300)
        .expect("recover jobs");

    assert_eq!(
        actions,
        vec![
            ("job-deploy".to_owned(), RecoveryAction::ReconcileInventory),
            ("job-download".to_owned(), RecoveryAction::ReResolve),
        ]
    );
    assert_eq!(
        reopened
            .job("job-download")
            .expect("load download job")
            .expect("download job")
            .stage,
        JobStage::Interrupted
    );
    assert_eq!(
        reopened
            .job("job-deploy")
            .expect("load deployment job")
            .expect("deployment job")
            .stage,
        JobStage::NeedsReconciliation
    );
    drop(reopened);

    let reopened_again = Persistence::open(database.path()).expect("reopen database again");
    assert_eq!(
        reopened_again
            .recover_jobs_after_restart(400)
            .expect("recover pending jobs again"),
        actions
    );
}

#[test]
fn job_request_context_survives_settings_changes_and_restart() {
    let database = TestDatabase::new("job-request-context");
    let store = Persistence::open(database.path()).expect("open database");
    let original = job("job-context", JobStage::Paused);
    store.save_job(&original).expect("save job");
    store
        .save_settings(&AppSettings {
            region: "US".to_owned(),
            market: "US".to_owned(),
            preferred_architectures: vec![Architecture::Arm64],
            preferred_languages: vec!["en-US".to_owned()],
            proxy_mode: ProxyMode::System,
            proxy_host: None,
            proxy_port: None,
            proxy_credentials: ProxyCredentialPolicy::PromptEveryTime,
            cache_enabled: false,
            cache_directory: String::new(),
            max_cache_bytes: 0,
            retention_days: 0,
            keep_installed_payloads: false,
            max_concurrent_downloads: 1,
        })
        .expect("change global settings");
    drop(store);

    let reopened = Persistence::open(database.path()).expect("reopen database");
    let restored = reopened
        .job("job-context")
        .expect("load job")
        .expect("job exists");

    assert_eq!(restored.requested_market, "CN");
    assert_eq!(restored.requested_architectures, vec![Architecture::X64]);
    assert_eq!(restored.requested_languages, vec!["zh-CN"]);
    assert_eq!(restored.deployment_scope, DeploymentScope::AllUsers);
    assert_eq!(restored.selected_update_id.as_deref(), Some("update-main"));
}
