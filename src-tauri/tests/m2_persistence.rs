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
        ProxyMode, ThemeMode,
    },
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    job_events::{
        DeploymentCheckpoint, DeploymentCheckpointPackage, JobEvent, JobTarget, JobTargetRole,
    },
    jobs::{Job, JobKind, JobStage, RecoveryAction},
    package_process::ProcessDescriptor,
    persistence::Persistence,
};

fn deployment_checkpoint() -> DeploymentCheckpoint {
    DeploymentCheckpoint {
        product_id: "product".to_owned(),
        main_update_id: "update-main".to_owned(),
        main_version: PackageVersion::new(1, 2, 3, 4),
        content_id: Some("content-main".to_owned()),
        packages: vec![DeploymentCheckpointPackage {
            role: JobTargetRole::Main,
            order: 0,
            cache_key: "cache-main".to_owned(),
            update_id: "update-main".to_owned(),
            identity_name: "Example.App".to_owned(),
            publisher: "CN=Example".to_owned(),
            version: "1.2.3.4".to_owned(),
            architecture: Architecture::X64,
            resource_id: None,
            package_kind: PackageKind::Main,
            format: PackageFormat::MsixBundle,
            expected_size: 4096,
            sha256: "abcdef".to_owned(),
        }],
    }
}

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
        app_name: Some("Example".to_owned()),
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

fn job(job_id: &str) -> Job {
    Job {
        job_id: job_id.to_owned(),
        kind: JobKind::Install,
        product_id: "9WZDNCRFJ3Q8".to_owned(),
        requested_market: "CN".to_owned(),
        requested_architectures: vec![Architecture::X64],
        requested_languages: vec!["zh-CN".to_owned()],
        deployment_scope: DeploymentScope::AllUsers,
        selected_update_id: None,
        package_family_name: None,
        stage: JobStage::Queued,
        bytes_done: 0,
        bytes_total: None,
        deployment_progress: None,
        version: None,
        architecture: None,
        language: None,
        error: None,
        blocked_processes: Vec::new(),
        created_at: 200,
        updated_at: 200,
    }
}

fn advance(store: &Persistence, job_id: &str, stages: &[JobStage]) {
    store.save_job(&job(job_id)).expect("create queued job");
    for (index, stage) in stages.iter().enumerate() {
        store
            .append_job_event(
                job_id,
                index as u64 + 1,
                JobEvent::StageChanged { stage: *stage },
                201 + index as i64,
            )
            .expect("advance job stage");
    }
}

#[test]
fn schema_migration_is_replayable() {
    let database = TestDatabase::new("migration-replay");

    let first = Persistence::open(database.path()).expect("first migration should succeed");
    assert_eq!(first.schema_version().expect("schema version"), 2);
    drop(first);

    let reopened = Persistence::open(database.path()).expect("migration replay should succeed");
    assert_eq!(reopened.schema_version().expect("schema version"), 2);

    let connection = rusqlite::Connection::open(database.path()).expect("inspect schema");
    let elevation_columns: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('jobs') WHERE name = 'requires_elevation'",
            [],
            |row| row.get(0),
        )
        .expect("inspect jobs columns");
    assert_eq!(elevation_columns, 0);
    let deployment_progress_columns: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('jobs') WHERE name = 'deployment_progress'",
            [],
            |row| row.get(0),
        )
        .expect("inspect deployment progress column");
    assert_eq!(deployment_progress_columns, 1);
    let runtime_progress_tables: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'job_progress'",
            [],
            |row| row.get(0),
        )
        .expect("inspect runtime progress table");
    assert_eq!(runtime_progress_tables, 1);
}

#[test]
fn schema_v1_upgrades_to_v2_without_rewriting_history() {
    let database = TestDatabase::new("migration-v1-v2");
    let connection = rusqlite::Connection::open(database.path()).expect("create v1 database");
    connection
        .execute_batch(include_str!("../migrations/0001_initial.sql"))
        .expect("apply v1 schema");
    connection
        .pragma_update(None, "user_version", 1)
        .expect("mark v1 schema");
    let legacy_job = job("job-migrated");
    let created = JobEvent::Created {
        job: legacy_job.clone(),
    };
    connection
        .execute(
            "INSERT INTO products
             (product_id, package_family_name, app_name, publisher, market, languages_json, updated_at)
             VALUES ('9WZDNCRFJ3Q8', 'Example.App_123', 'Example', 'CN=Example', 'CN',
                     '[\"zh-CN\",\"en-US\"]', 100)",
            [],
        )
        .expect("seed v1 product");
    connection
        .execute(
            "INSERT INTO jobs (
                job_id, kind, product_id, requested_market, requested_architectures_json,
                requested_languages_json, deployment_scope, selected_update_id,
                package_family_name, stage, bytes_done, bytes_total, deployment_progress,
                version, architecture, language, error_json, blocked_processes_json,
                event_sequence, created_at, updated_at
             ) VALUES (?1, 'install', ?2, 'CN', '[\"x64\"]', '[\"zh-CN\"]',
                       'AllUsers', NULL, NULL, 'queued', 0, NULL, NULL, NULL, NULL, NULL,
                       NULL, '[]', 1, 200, 200)",
            rusqlite::params![legacy_job.job_id, legacy_job.product_id],
        )
        .expect("seed v1 job projection");
    connection
        .execute(
            "INSERT INTO job_events
             (job_id, sequence, event_kind, payload_json, projection_json, occurred_at)
             VALUES ('job-migrated', 1, 'created', ?1, ?2, 200)",
            rusqlite::params![
                serde_json::to_string(&created).expect("serialize created event"),
                serde_json::to_string(&legacy_job).expect("serialize job projection"),
            ],
        )
        .expect("seed v1 event");
    connection
        .execute(
            "INSERT INTO cache_entries
             (cache_key, job_id, update_id, path, size, sha256, state, last_accessed_at)
             VALUES ('legacy-cache', 'job-migrated', 'update-main', 'E:\\Cache\\legacy.msix',
                     1, ?1, 'verified', 200)",
            ["ab".repeat(32)],
        )
        .expect("seed v1 cache entry");
    let legacy_settings = AppSettings {
        region: "CN".to_owned(),
        market: "CN".to_owned(),
        preferred_architectures: vec![Architecture::X64],
        preferred_languages: vec!["zh-CN".to_owned()],
        proxy_mode: ProxyMode::Disabled,
        proxy_host: None,
        proxy_port: None,
        proxy_credentials: ProxyCredentialPolicy::PromptEveryTime,
        cache_enabled: true,
        cache_directory: r"E:\Cache".to_owned(),
        max_cache_bytes: 1024,
        retention_days: 30,
        keep_installed_payloads: false,
        max_concurrent_downloads: 2,
        max_concurrent_update_scans: 16,
        theme: ThemeMode::System,
        diagnostics_enabled: false,
    };
    connection
        .execute(
            "INSERT INTO settings (key, value_json, updated_at) VALUES ('application', ?1, 200)",
            [serde_json::to_string(&legacy_settings).expect("serialize settings")],
        )
        .expect("seed v1 settings");

    let events_before: Vec<(i64, String, String)> = connection
        .prepare(
            "SELECT sequence, event_kind, projection_json FROM job_events
             WHERE job_id = 'job-migrated' ORDER BY sequence",
        )
        .expect("prepare history query")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query history")
        .collect::<Result<_, _>>()
        .expect("read history");
    drop(connection);

    let reopened = Persistence::open(database.path()).expect("reopen v2 database");
    assert_eq!(reopened.schema_version().expect("schema version"), 2);
    assert_eq!(
        reopened
            .job_snapshot("job-migrated")
            .expect("load migrated job")
            .expect("migrated job exists")
            .sequence,
        1
    );
    assert!(reopened.product("9WZDNCRFJ3Q8").expect("product").is_some());
    assert_eq!(
        reopened.settings().expect("settings"),
        Some(legacy_settings)
    );
    assert!(reopened
        .cache_entry("legacy-cache")
        .expect("cache entry")
        .is_some());
    drop(reopened);

    let after = rusqlite::Connection::open(database.path()).expect("inspect reopened database");
    let events_after: Vec<(i64, String, String)> = after
        .prepare(
            "SELECT sequence, event_kind, projection_json FROM job_events
             WHERE job_id = 'job-migrated' ORDER BY sequence",
        )
        .expect("prepare history query")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query history")
        .collect::<Result<_, _>>()
        .expect("read history");
    assert_eq!(events_after, events_before);
    let progress_rows: i64 = after
        .query_row("SELECT COUNT(*) FROM job_progress", [], |row| row.get(0))
        .expect("count runtime progress");
    assert_eq!(progress_rows, 0);
}

#[test]
fn database_cleanup_removes_completed_and_cancelled_history_but_preserves_failed_data() {
    let database = TestDatabase::new("database-cleanup");
    let store = Persistence::open(database.path()).expect("open database");
    store
        .upsert_product(&product())
        .expect("save catalog product");

    let completed_sequence = deploying(&store, "job-completed");
    store
        .append_job_event(
            "job-completed",
            completed_sequence,
            JobEvent::Completed,
            300,
        )
        .expect("complete job");
    store
        .save_job(&job("job-cancelled"))
        .expect("create cancelled job");
    store
        .append_job_event("job-cancelled", 1, JobEvent::Cancelled, 301)
        .expect("cancel job");
    store
        .save_job(&job("job-failed"))
        .expect("create failed job");
    store
        .append_job_event(
            "job-failed",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            302,
        )
        .expect("start failed job");
    store
        .append_job_event(
            "job-failed",
            2,
            JobEvent::Failed {
                error: AppErrorDto::new(ErrorCode::CatalogUnavailable, RetryAdvice::Retry),
            },
            303,
        )
        .expect("fail job");
    store
        .upsert_cache_entry(&CacheEntry {
            cache_key: "completed-cache".to_owned(),
            job_id: Some("job-completed".to_owned()),
            update_id: "update-main".to_owned(),
            path: r"E:\Cache\completed.msix".to_owned(),
            size: 1,
            sha256: "ab".repeat(32),
            state: CacheState::Verified,
            last_accessed_at: 300,
        })
        .expect("save completed cache reference");
    store
        .record_diagnostic(&DiagnosticEvent {
            job_id: Some("job-cancelled".to_owned()),
            code: ErrorCode::DownloadFailed,
            stage: Some(JobStage::Cancelled),
            operation: DiagnosticOperation::Download,
            os_error_code: None,
            retryable: false,
            occurred_at: 301,
        })
        .expect("save terminal diagnostic");

    let report = store
        .cleanup_database(1_000)
        .expect("clean terminal history");
    assert_eq!(report.removed_jobs, 2);
    assert_eq!(report.removed_events, 10);
    assert_eq!(report.removed_commands, 0);
    assert_eq!(report.removed_diagnostics, 1);
    assert_eq!(report.removed_progress, 0);
    assert!(store
        .job_snapshot("job-completed")
        .expect("completed query")
        .is_none());
    assert!(store
        .job_snapshot("job-cancelled")
        .expect("cancelled query")
        .is_none());
    assert_eq!(
        store
            .job_snapshot("job-failed")
            .expect("failed query")
            .expect("failed job remains")
            .job
            .stage,
        JobStage::Failed
    );
    assert!(store
        .product("9WZDNCRFJ3Q8")
        .expect("product query")
        .is_some());
    assert_eq!(
        store
            .cache_entry("completed-cache")
            .expect("cache query")
            .expect("cache entry remains")
            .job_id,
        None
    );
}

#[test]
fn database_cleanup_refuses_leases_pending_commands_and_active_jobs() {
    let database = TestDatabase::new("database-cleanup-busy");
    let store = Persistence::open(database.path()).expect("open database");
    store
        .save_job(&job("job-active"))
        .expect("create active job");
    assert!(matches!(
        store.cleanup_database(300),
        Err(yet_another_microsoft_store_lib::persistence::PersistenceError::MaintenanceBusy)
    ));

    store
        .append_job_event("job-active", 1, JobEvent::Cancelled, 301)
        .expect("cancel active job");
    let lease = store
        .acquire_worker_lease("worker", 301, 100)
        .expect("lease query")
        .expect("lease available");
    assert!(matches!(
        store.cleanup_database(302),
        Err(yet_another_microsoft_store_lib::persistence::PersistenceError::MaintenanceBusy)
    ));
    assert!(store.release_worker_lease(&lease).expect("release lease"));
}

fn deploying(store: &Persistence, job_id: &str) -> u64 {
    advance(
        store,
        job_id,
        &[
            JobStage::Resolving,
            JobStage::Selecting,
            JobStage::Downloading,
            JobStage::Verifying,
            JobStage::Preparing,
            JobStage::Deploying,
        ],
    );
    store.job_snapshot(job_id).unwrap().unwrap().sequence
}

#[test]
fn checkpoint_and_blocked_processes_survive_reopen_and_terminal_cleanup() {
    let database = TestDatabase::new("deployment-checkpoint");
    let checkpoint = deployment_checkpoint();
    let processes = vec![ProcessDescriptor {
        pid: 420,
        name: "Example.exe".to_owned(),
    }];
    {
        let store = Persistence::open(database.path()).expect("open database");
        let sequence = deploying(&store, "job-checkpoint");
        let lease = store
            .acquire_worker_lease("worker-a", 300, 100)
            .unwrap()
            .unwrap();
        let ready = store
            .save_deployment_checkpoint_leased(
                "job-checkpoint",
                sequence,
                &checkpoint,
                301,
                &lease,
                301,
            )
            .expect("checkpoint and ready event commit together");
        let blocked = store
            .append_job_event_leased(
                "job-checkpoint",
                ready.sequence,
                JobEvent::DeploymentBlocked {
                    processes: processes.clone(),
                },
                302,
                &lease,
                302,
            )
            .expect("record deployment block");
        assert_eq!(blocked.job.stage, JobStage::AwaitingProcessExit);
        assert_eq!(blocked.job.blocked_processes, processes);
    }
    {
        let store = Persistence::open(database.path()).expect("reopen database");
        assert_eq!(
            store
                .deployment_checkpoint("job-checkpoint")
                .expect("load checkpoint"),
            Some(checkpoint)
        );
        let waiting = store.job_snapshot("job-checkpoint").unwrap().unwrap();
        assert_eq!(waiting.job.stage, JobStage::AwaitingProcessExit);
        let lease = store
            .acquire_worker_lease("worker-b", 500, 100)
            .unwrap()
            .unwrap();
        store
            .append_job_event_leased(
                "job-checkpoint",
                waiting.sequence,
                JobEvent::Cancelled,
                501,
                &lease,
                501,
            )
            .expect("cancel waiting deployment");
        assert_eq!(store.deployment_checkpoint("job-checkpoint").unwrap(), None);
    }
}

#[test]
fn invalid_or_stale_checkpoint_write_rolls_back_without_advancing_the_job() {
    let database = TestDatabase::new("deployment-checkpoint-rollback");
    let store = Persistence::open(database.path()).expect("open database");
    let sequence = deploying(&store, "job-checkpoint-rollback");
    let old_lease = store
        .acquire_worker_lease("worker-a", 300, 10)
        .unwrap()
        .unwrap();
    let mut invalid = deployment_checkpoint();
    invalid.packages[0].cache_key.clear();
    assert!(store
        .save_deployment_checkpoint_leased(
            "job-checkpoint-rollback",
            sequence,
            &invalid,
            301,
            &old_lease,
            301,
        )
        .is_err());
    assert_eq!(
        store
            .job_snapshot("job-checkpoint-rollback")
            .unwrap()
            .unwrap()
            .sequence,
        sequence
    );
    assert_eq!(
        store
            .deployment_checkpoint("job-checkpoint-rollback")
            .unwrap(),
        None
    );

    let new_lease = store
        .acquire_worker_lease("worker-b", 311, 100)
        .unwrap()
        .unwrap();
    assert!(store
        .save_deployment_checkpoint_leased(
            "job-checkpoint-rollback",
            sequence,
            &deployment_checkpoint(),
            312,
            &old_lease,
            312,
        )
        .is_err());
    store
        .save_deployment_checkpoint_leased(
            "job-checkpoint-rollback",
            sequence,
            &deployment_checkpoint(),
            312,
            &new_lease,
            312,
        )
        .expect("current lease saves checkpoint");
}

#[test]
fn malformed_checkpoint_json_fails_closed() {
    let database = TestDatabase::new("deployment-checkpoint-malformed");
    {
        let store = Persistence::open(database.path()).expect("open database");
        let sequence = deploying(&store, "job-checkpoint-malformed");
        let lease = store
            .acquire_worker_lease("worker", 300, 100)
            .unwrap()
            .unwrap();
        store
            .save_deployment_checkpoint_leased(
                "job-checkpoint-malformed",
                sequence,
                &deployment_checkpoint(),
                301,
                &lease,
                301,
            )
            .expect("save checkpoint");
    }
    let connection = rusqlite::Connection::open(database.path()).expect("open raw database");
    connection
        .execute(
            "UPDATE deployment_checkpoints SET checkpoint_json = '{\"packages\":[]}' WHERE job_id = ?1",
            ["job-checkpoint-malformed"],
        )
        .expect("corrupt checkpoint fixture");
    drop(connection);
    let store = Persistence::open(database.path()).expect("reopen database");
    assert!(store
        .deployment_checkpoint("job-checkpoint-malformed")
        .is_err());
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
        max_concurrent_update_scans: 16,
        theme: ThemeMode::Dark,
        diagnostics_enabled: true,
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
    advance(
        &store,
        "job-download",
        &[
            JobStage::Resolving,
            JobStage::Selecting,
            JobStage::Downloading,
        ],
    );
    advance(
        &store,
        "job-deploy",
        &[
            JobStage::Resolving,
            JobStage::Selecting,
            JobStage::Downloading,
            JobStage::Verifying,
            JobStage::Preparing,
            JobStage::Deploying,
        ],
    );
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
    let original = job("job-context");
    store.save_job(&original).expect("save job");
    store
        .append_job_event(
            "job-context",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            201,
        )
        .unwrap();
    store
        .append_job_event(
            "job-context",
            2,
            JobEvent::StageChanged {
                stage: JobStage::Selecting,
            },
            202,
        )
        .unwrap();
    store
        .append_job_event(
            "job-context",
            3,
            JobEvent::SelectionRecorded {
                selected_update_id: "update-main".to_owned(),
                package_family_name: "Example.App_123".to_owned(),
                version: "1.2.3.4".to_owned(),
                architecture: Architecture::X64,
                language: Some("zh-CN".to_owned()),
                targets: vec![JobTarget {
                    role: JobTargetRole::Main,
                    update_id: "update-main".to_owned(),
                    identity_name: "Example.App".to_owned(),
                    publisher: "CN=Example".to_owned(),
                    version: "1.2.3.4".to_owned(),
                    architecture: Architecture::X64,
                    resource_id: None,
                    package_kind: PackageKind::Main,
                    expected_size: 1024,
                    sha256: "a".repeat(64),
                }],
            },
            203,
        )
        .unwrap();
    store
        .append_job_event(
            "job-context",
            4,
            JobEvent::StageChanged {
                stage: JobStage::Downloading,
            },
            204,
        )
        .unwrap();
    store
        .append_job_event(
            "job-context",
            5,
            JobEvent::StageChanged {
                stage: JobStage::Paused,
            },
            205,
        )
        .unwrap();
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
            max_concurrent_update_scans: 16,
            theme: ThemeMode::System,
            diagnostics_enabled: false,
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
