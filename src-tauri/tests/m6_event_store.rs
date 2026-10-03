use std::{fs, path::PathBuf};

use rusqlite::Connection;
use yet_another_microsoft_store_lib::{
    deployment::DeploymentScope,
    domain::{Architecture, PackageKind},
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    job_events::{CommandOutcome, JobCommand, JobControl, JobEvent, JobTarget, JobTargetRole},
    jobs::{Job, JobKind, JobStage},
    persistence::{Persistence, PersistenceError},
};

struct TestDatabase(PathBuf);

impl TestDatabase {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "yamstore-m6-events-{}.sqlite3",
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

fn job(job_id: &str) -> Job {
    Job {
        job_id: job_id.to_owned(),
        kind: JobKind::Install,
        product_id: "9WZDNCRFJ3Q8".to_owned(),
        requested_market: "CN".to_owned(),
        requested_architectures: vec![Architecture::X64],
        requested_languages: vec!["zh-CN".to_owned()],
        deployment_scope: DeploymentScope::CurrentUser,
        selected_update_id: None,
        package_family_name: None,
        stage: JobStage::Queued,
        bytes_done: 0,
        bytes_total: None,
        version: None,
        architecture: None,
        language: None,
        error: None,
        created_at: 100,
        updated_at: 100,
    }
}

fn created(store: &Persistence, job_id: &str) {
    store
        .append_job_event(job_id, 0, JobEvent::Created { job: job(job_id) }, 100)
        .expect("create job event");
}

#[test]
fn append_updates_event_and_projection_together_or_rolls_both_back() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-atomic");

    let snapshot = store
        .append_job_event(
            "job-atomic",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            200,
        )
        .expect("append transition");
    assert_eq!(snapshot.sequence, 2);
    assert_eq!(snapshot.job.stage, JobStage::Resolving);

    let connection = Connection::open(&database.0).expect("open inspection connection");
    connection
        .execute_batch(
            "CREATE TRIGGER reject_job_update BEFORE UPDATE ON jobs
             BEGIN SELECT RAISE(ABORT, 'project failed'); END;",
        )
        .expect("install failure trigger");
    assert!(store
        .append_job_event(
            "job-atomic",
            2,
            JobEvent::StageChanged {
                stage: JobStage::Selecting
            },
            300
        )
        .is_err());
    assert_eq!(store.list_job_events(0, 10).expect("events").len(), 2);
    assert_eq!(
        store
            .job_snapshot("job-atomic")
            .expect("snapshot")
            .unwrap()
            .sequence,
        2
    );
}

#[test]
fn stale_expected_sequence_cannot_append_or_change_projection() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-conflict");
    assert!(matches!(
        store.append_job_event(
            "job-conflict",
            0,
            JobEvent::StageChanged {
                stage: JobStage::Resolving
            },
            200
        ),
        Err(PersistenceError::SequenceConflict { .. })
    ));
    assert_eq!(store.list_job_events(0, 10).expect("events").len(), 1);
    assert_eq!(
        store
            .job_snapshot("job-conflict")
            .expect("snapshot")
            .unwrap()
            .job
            .stage,
        JobStage::Queued
    );
}

#[test]
fn command_id_is_idempotent_and_cannot_be_reused_for_a_different_request() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-command");
    let command = JobCommand {
        command_id: "command-1".to_owned(),
        job_id: "job-command".to_owned(),
        control: JobControl::Cancel,
        expected_sequence: 1,
        created_at: 200,
        processed_at: None,
        outcome: None,
    };
    assert_eq!(
        store.enqueue_job_command(&command).expect("enqueue"),
        command
    );
    assert_eq!(
        store.enqueue_job_command(&command).expect("deduplicate"),
        command
    );
    assert_eq!(
        store
            .pending_job_commands("job-command")
            .expect("pending")
            .len(),
        1
    );
    let mut conflicting = command;
    conflicting.control = JobControl::Pause;
    assert!(matches!(
        store.enqueue_job_command(&conflicting),
        Err(PersistenceError::CommandConflict)
    ));
}

#[test]
fn global_cursor_pages_events_in_insert_order_without_repeating_rows() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-a");
    created(&store, "job-b");
    store
        .append_job_event(
            "job-a",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            200,
        )
        .expect("append third event");

    let page = store.list_job_events(0, 2).expect("first page");
    assert_eq!(page.len(), 2);
    assert_eq!(
        page.iter()
            .map(|event| event.job_id.as_str())
            .collect::<Vec<_>>(),
        ["job-a", "job-b"]
    );
    assert!(page[0].cursor < page[1].cursor);
    let next = store.list_job_events(page[1].cursor, 2).expect("next page");
    assert_eq!(next.len(), 1);
    assert_eq!((next[0].job_id.as_str(), next[0].sequence), ("job-a", 2));
    assert!(store
        .list_job_events(next[0].cursor, 2)
        .expect("end")
        .is_empty());
}

#[test]
fn cursor_replay_returns_the_projection_at_each_event_sequence() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-history");
    store
        .append_job_event(
            "job-history",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            101,
        )
        .unwrap();
    store
        .append_job_event(
            "job-history",
            2,
            JobEvent::StageChanged {
                stage: JobStage::Selecting,
            },
            102,
        )
        .unwrap();

    let first_page = store.list_job_events(0, 2).expect("first cursor page");
    let second_page = store
        .list_job_events(first_page[1].cursor, 2)
        .expect("second cursor page");
    let history = first_page
        .into_iter()
        .chain(second_page)
        .collect::<Vec<_>>();
    assert_eq!(
        history
            .iter()
            .map(|entry| entry.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        history
            .iter()
            .map(|entry| entry.snapshot.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        history
            .iter()
            .map(|entry| entry.snapshot.job.stage)
            .collect::<Vec<_>>(),
        [JobStage::Queued, JobStage::Resolving, JobStage::Selecting]
    );
    assert!(history
        .iter()
        .all(|entry| entry.snapshot.job.job_id == entry.job_id));
    assert_eq!(
        store
            .job_snapshot("job-history")
            .unwrap()
            .unwrap()
            .job
            .stage,
        JobStage::Selecting
    );
    assert_eq!(history[0].snapshot.job.stage, JobStage::Queued);
}

#[test]
fn rebuild_replays_a_lagging_projection_but_rejects_gaps_and_ahead_projection() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-rebuild");
    store
        .append_job_event(
            "job-rebuild",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            200,
        )
        .expect("append second event");
    let connection = Connection::open(&database.0).expect("inspect database");
    connection
        .execute("UPDATE jobs SET event_sequence = 1, stage = 'queued', updated_at = 100 WHERE job_id = 'job-rebuild'", [])
        .expect("simulate stale projection");
    let rebuilt = store.rebuild_job_projection("job-rebuild").expect("replay");
    assert_eq!(
        (rebuilt.sequence, rebuilt.job.stage),
        (2, JobStage::Resolving)
    );

    connection
        .execute(
            "UPDATE jobs SET event_sequence = 3 WHERE job_id = 'job-rebuild'",
            [],
        )
        .expect("simulate projection ahead");
    assert!(matches!(
        store.rebuild_job_projection("job-rebuild"),
        Err(PersistenceError::EventHistoryInvalid)
    ));
    connection
        .execute(
            "UPDATE jobs SET event_sequence = 2 WHERE job_id = 'job-rebuild'",
            [],
        )
        .expect("restore projection sequence");
    connection
        .execute(
            "DELETE FROM job_events WHERE job_id = 'job-rebuild' AND sequence = 1",
            [],
        )
        .expect("create history gap");
    assert!(matches!(
        store.rebuild_job_projection("job-rebuild"),
        Err(PersistenceError::EventHistoryInvalid)
    ));
}

#[test]
fn event_payload_rejects_urls_paths_and_noncanonical_error_text() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    for unsafe_product in [
        "https://example.test/package.msix",
        r"C:\Users\Admin\payload.msix",
        "proxy-password=secret",
    ] {
        let mut unsafe_job = job("unsafe");
        unsafe_job.product_id = unsafe_product.to_owned();
        assert!(matches!(
            store.append_job_event("unsafe", 0, JobEvent::Created { job: unsafe_job }, 100),
            Err(PersistenceError::UnsafeJobEvent)
        ));
    }
    assert!(store.list_job_events(0, 10).expect("events").is_empty());

    created(&store, "safe-error");
    store
        .append_job_event(
            "safe-error",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            101,
        )
        .unwrap();
    let mut error = AppErrorDto::new(ErrorCode::CatalogUnavailable, RetryAdvice::Retry);
    error.message_key = "https://example.test/private-response".to_owned();
    assert!(matches!(
        store.append_job_event("safe-error", 2, JobEvent::Failed { error }, 102),
        Err(PersistenceError::EventHistoryInvalid)
    ));
    assert_eq!(store.list_job_events(0, 10).unwrap().len(), 2);
}

#[test]
fn semantic_events_reject_illegal_terminal_transition_and_progress_regression() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-semantics");
    assert!(matches!(
        store.append_job_event("job-semantics", 1, JobEvent::Completed, 200),
        Err(PersistenceError::EventHistoryInvalid)
    ));
    for (sequence, stage) in [
        (1, JobStage::Resolving),
        (2, JobStage::Selecting),
        (3, JobStage::Downloading),
    ] {
        store
            .append_job_event(
                "job-semantics",
                sequence,
                JobEvent::StageChanged { stage },
                200 + sequence as i64,
            )
            .expect("advance stage");
    }
    store
        .append_job_event(
            "job-semantics",
            4,
            JobEvent::ProgressRecorded {
                bytes_done: 100,
                bytes_total: Some(200),
            },
            300,
        )
        .expect("record progress");
    assert!(matches!(
        store.append_job_event(
            "job-semantics",
            5,
            JobEvent::ProgressRecorded {
                bytes_done: 50,
                bytes_total: Some(200)
            },
            301
        ),
        Err(PersistenceError::EventHistoryInvalid)
    ));
    assert_eq!(
        store
            .job_snapshot("job-semantics")
            .unwrap()
            .unwrap()
            .job
            .bytes_done,
        100
    );
    assert!(matches!(
        store.append_job_event(
            "job-semantics",
            5,
            JobEvent::ProgressRecorded {
                bytes_done: 150,
                bytes_total: None
            },
            302
        ),
        Err(PersistenceError::EventHistoryInvalid)
    ));
}

#[test]
fn caller_cannot_replace_selection_error_or_import_a_snapshot() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-closed");
    assert!(matches!(
        store.append_job_event(
            "job-closed",
            1,
            JobEvent::Imported {
                job: job("job-closed")
            },
            200
        ),
        Err(PersistenceError::EventHistoryInvalid)
    ));
    assert!(matches!(
        store.append_job_event(
            "job-closed",
            1,
            JobEvent::SelectionRecorded {
                selected_update_id: "update-new".to_owned(),
                package_family_name: "Example.App_123".to_owned(),
                version: "1.2.3.4".to_owned(),
                architecture: Architecture::X64,
                language: Some("zh-CN".to_owned()),
                targets: Vec::new(),
            },
            200
        ),
        Err(PersistenceError::EventHistoryInvalid)
    ));
    let forged = r#"{"kind":"stage_changed","stage":"resolving","selectedUpdateId":"attacker","error":{"code":"download_failed"}}"#;
    assert!(serde_json::from_str::<JobEvent>(forged).is_err());
    assert_eq!(store.list_job_events(0, 10).unwrap().len(), 1);

    let mut preselected = job("job-preselected");
    preselected.selected_update_id = Some("update-main".to_owned());
    assert!(matches!(
        store.append_job_event(
            "job-preselected",
            0,
            JobEvent::Created { job: preselected },
            100
        ),
        Err(PersistenceError::EventHistoryInvalid)
    ));
}

#[test]
fn expired_lease_owner_cannot_write_after_generation_is_taken_over() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-lease");
    let first = store
        .acquire_worker_lease("owner-a", 100, 10)
        .expect("acquire first lease")
        .expect("lease available");
    assert_eq!(first.generation, 1);
    assert!(store
        .acquire_worker_lease("owner-b", 105, 10)
        .expect("competing lease")
        .is_none());
    let second = store
        .acquire_worker_lease("owner-b", 110, 10)
        .expect("take over expired lease")
        .expect("lease expired");
    assert_eq!(second.generation, 2);
    assert!(matches!(
        store.append_job_event_leased(
            "job-lease",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving
            },
            111,
            &first,
            111
        ),
        Err(PersistenceError::LeaseConflict)
    ));
    store
        .append_job_event_leased(
            "job-lease",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            111,
            &second,
            111,
        )
        .expect("current lease owner writes");
    assert_eq!(
        store.job_snapshot("job-lease").unwrap().unwrap().sequence,
        2
    );
    assert!(store.release_worker_lease(&second).expect("release lease"));
    let third = store
        .acquire_worker_lease("owner-b", 112, 10)
        .expect("reacquire lease")
        .expect("released lease available");
    assert_eq!(third.generation, 3);
    assert!(matches!(
        store.append_job_event_leased(
            "job-lease",
            2,
            JobEvent::StageChanged {
                stage: JobStage::Selecting
            },
            113,
            &second,
            113
        ),
        Err(PersistenceError::LeaseConflict)
    ));
}

#[test]
fn command_outcome_and_transition_commit_or_rollback_together() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-control");
    let lease = store
        .acquire_worker_lease("owner", 100, 100)
        .expect("acquire lease")
        .expect("lease available");
    let command = JobCommand {
        command_id: "cancel-1".to_owned(),
        job_id: "job-control".to_owned(),
        control: JobControl::Cancel,
        expected_sequence: 1,
        created_at: 101,
        processed_at: None,
        outcome: None,
    };
    store
        .enqueue_job_command(&command)
        .expect("enqueue command");
    let connection = Connection::open(&database.0).expect("open trigger connection");
    connection
        .execute_batch(
            "CREATE TRIGGER reject_control_projection BEFORE UPDATE ON jobs
         BEGIN SELECT RAISE(ABORT, 'projection failed'); END;",
        )
        .expect("install failure trigger");
    assert!(store
        .apply_job_command(
            "cancel-1",
            JobEvent::Cancelled,
            CommandOutcome::Applied,
            110,
            &lease,
            110
        )
        .is_err());
    assert_eq!(store.pending_job_commands("job-control").unwrap().len(), 1);
    assert_eq!(store.list_job_events(0, 10).unwrap().len(), 1);
    connection
        .execute_batch("DROP TRIGGER reject_control_projection")
        .expect("remove failure trigger");
    let applied = store
        .apply_job_command(
            "cancel-1",
            JobEvent::Cancelled,
            CommandOutcome::Applied,
            110,
            &lease,
            110,
        )
        .expect("apply command");
    assert_eq!(
        (applied.sequence, applied.job.stage),
        (2, JobStage::Cancelled)
    );
    assert!(store
        .pending_job_commands("job-control")
        .unwrap()
        .is_empty());
    assert_eq!(
        store.enqueue_job_command(&command).unwrap().outcome,
        Some(CommandOutcome::Applied)
    );
    let repeated = store
        .apply_job_command(
            "cancel-1",
            JobEvent::Cancelled,
            CommandOutcome::Applied,
            111,
            &lease,
            111,
        )
        .expect("duplicate command is idempotent");
    assert_eq!(
        (repeated.sequence, repeated.job.stage),
        (2, JobStage::Cancelled)
    );
    assert_eq!(store.list_job_events(0, 10).unwrap().len(), 2);
}

#[test]
fn finishing_command_requires_current_worker_lease() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-finish-lease");
    let old_lease = store
        .acquire_worker_lease("old-owner", 100, 10)
        .unwrap()
        .unwrap();
    let command = JobCommand {
        command_id: "pause-lease".to_owned(),
        job_id: "job-finish-lease".to_owned(),
        control: JobControl::Pause,
        expected_sequence: 1,
        created_at: 101,
        processed_at: None,
        outcome: None,
    };
    store.enqueue_job_command(&command).unwrap();

    assert!(matches!(
        store.finish_job_command_leased(
            "pause-lease",
            CommandOutcome::AlreadySatisfied,
            111,
            &old_lease,
            111,
        ),
        Err(PersistenceError::LeaseConflict)
    ));
    assert_eq!(
        store.pending_job_commands("job-finish-lease").unwrap(),
        std::slice::from_ref(&command)
    );

    let new_lease = store
        .acquire_worker_lease("new-owner", 111, 10)
        .unwrap()
        .unwrap();
    assert!(matches!(
        store.finish_job_command_leased(
            "pause-lease",
            CommandOutcome::AlreadySatisfied,
            112,
            &old_lease,
            112,
        ),
        Err(PersistenceError::LeaseConflict)
    ));
    assert_eq!(
        store.pending_job_commands("job-finish-lease").unwrap(),
        [command]
    );
    let finished = store
        .finish_job_command_leased(
            "pause-lease",
            CommandOutcome::AlreadySatisfied,
            112,
            &new_lease,
            112,
        )
        .expect("current owner finishes command");
    assert_eq!(finished.outcome, Some(CommandOutcome::AlreadySatisfied));
    assert!(store
        .pending_job_commands("job-finish-lease")
        .unwrap()
        .is_empty());
    assert_eq!(store.list_job_events(0, 10).unwrap().len(), 1);
}

#[test]
fn opening_database_replays_lagging_projection_and_rejects_history_gap() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).unwrap();
    created(&store, "job-open");
    store
        .append_job_event(
            "job-open",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            101,
        )
        .unwrap();
    drop(store);
    let connection = Connection::open(&database.0).unwrap();
    connection
        .execute(
            "UPDATE jobs SET event_sequence = 1, stage = 'queued',
        updated_at = 100 WHERE job_id = 'job-open'",
            [],
        )
        .unwrap();
    drop(connection);
    let reopened = Persistence::open(&database.0).expect("replay on open");
    assert_eq!(
        reopened.job_snapshot("job-open").unwrap().unwrap().sequence,
        2
    );
    drop(reopened);
    let connection = Connection::open(&database.0).unwrap();
    connection
        .execute(
            "DELETE FROM job_events WHERE job_id = 'job-open' AND sequence = 1",
            [],
        )
        .unwrap();
    drop(connection);
    assert!(matches!(
        Persistence::open(&database.0),
        Err(PersistenceError::EventHistoryInvalid)
    ));
}

#[test]
fn cursor_read_fails_closed_if_stored_event_payload_is_tampered() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).unwrap();
    created(&store, "job-tampered");
    let connection = Connection::open(&database.0).unwrap();
    connection
        .execute(
            "UPDATE job_events SET payload_json = REPLACE(payload_json,
            '9WZDNCRFJ3Q8', 'https://example.test/secret.msix')
         WHERE job_id = 'job-tampered'",
            [],
        )
        .unwrap();
    assert!(store.list_job_events(0, 10).is_err());
}

#[test]
fn selection_freezes_safe_targets_in_the_event_transaction() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    created(&store, "job-target");
    store
        .append_job_event(
            "job-target",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            101,
        )
        .unwrap();
    store
        .append_job_event(
            "job-target",
            2,
            JobEvent::StageChanged {
                stage: JobStage::Selecting,
            },
            102,
        )
        .unwrap();
    let target = JobTarget {
        role: JobTargetRole::Main,
        update_id: "update-main".to_owned(),
        identity_name: "Example.App".to_owned(),
        publisher: "CN=Example".to_owned(),
        version: "1.2.3.4".to_owned(),
        architecture: Architecture::X64,
        resource_id: None,
        package_kind: PackageKind::Main,
        expected_size: 4096,
        sha256: "a".repeat(64),
    };
    let selected = JobEvent::SelectionRecorded {
        selected_update_id: "update-main".to_owned(),
        package_family_name: "Example.App_123".to_owned(),
        version: "1.2.3.4".to_owned(),
        architecture: Architecture::X64,
        language: Some("zh-CN".to_owned()),
        targets: vec![target.clone()],
    };
    let mut mismatched = selected.clone();
    if let JobEvent::SelectionRecorded { targets, .. } = &mut mismatched {
        targets[0].version = "9.9.9.9".to_owned();
    }
    assert!(matches!(
        store.append_job_event("job-target", 3, mismatched, 103),
        Err(PersistenceError::UnsafeJobEvent)
    ));
    let connection = Connection::open(&database.0).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_target BEFORE INSERT ON job_targets
         BEGIN SELECT RAISE(ABORT, 'target failed'); END;",
        )
        .unwrap();
    assert!(store
        .append_job_event("job-target", 3, selected.clone(), 103)
        .is_err());
    assert_eq!(
        store.job_snapshot("job-target").unwrap().unwrap().sequence,
        3
    );
    assert!(store.job_targets("job-target").unwrap().is_empty());
    connection
        .execute_batch("DROP TRIGGER reject_target")
        .unwrap();
    store
        .append_job_event("job-target", 3, selected, 103)
        .unwrap();
    assert_eq!(
        store.job_targets("job-target").unwrap(),
        vec![target.clone()]
    );

    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
    connection
        .execute("DELETE FROM jobs WHERE job_id = 'job-target'", [])
        .unwrap();
    let rebuilt = store
        .rebuild_job_projection("job-target")
        .expect("rebuild missing projection");
    assert_eq!(rebuilt.sequence, 4);
    assert_eq!(
        rebuilt.job.selected_update_id.as_deref(),
        Some("update-main")
    );
    assert_eq!(store.job_targets("job-target").unwrap(), vec![target]);
}

#[test]
fn recovery_noop_is_rejected_and_verifying_can_finish_without_deployment() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).unwrap();
    created(&store, "job-recovery");
    for (sequence, stage) in [
        (1, JobStage::Resolving),
        (2, JobStage::Selecting),
        (3, JobStage::Downloading),
    ] {
        store
            .append_job_event(
                "job-recovery",
                sequence,
                JobEvent::StageChanged { stage },
                100 + sequence as i64,
            )
            .unwrap();
    }
    store
        .append_job_event(
            "job-recovery",
            4,
            JobEvent::Recovered {
                stage: JobStage::Interrupted,
            },
            105,
        )
        .unwrap();
    assert!(matches!(
        store.append_job_event(
            "job-recovery",
            5,
            JobEvent::Recovered {
                stage: JobStage::Interrupted
            },
            106
        ),
        Err(PersistenceError::EventHistoryInvalid)
    ));

    let mut domain_job = job("job-already-current");
    domain_job.stage = JobStage::Verifying;
    domain_job
        .transition_to(JobStage::Completed, 200)
        .expect("already-current verification can complete");
    assert_eq!(domain_job.stage, JobStage::Completed);
}

#[test]
fn leased_restart_recovery_survives_released_lease_row_and_is_idempotent() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).expect("open database");
    for (job_id, stages) in [
        (
            "job-download-restart",
            vec![
                JobStage::Resolving,
                JobStage::Selecting,
                JobStage::Downloading,
            ],
        ),
        (
            "job-deploy-restart",
            vec![
                JobStage::Resolving,
                JobStage::Selecting,
                JobStage::Downloading,
                JobStage::Verifying,
                JobStage::Deploying,
            ],
        ),
    ] {
        created(&store, job_id);
        for (index, stage) in stages.into_iter().enumerate() {
            store
                .append_job_event(
                    job_id,
                    index as u64 + 1,
                    JobEvent::StageChanged { stage },
                    101 + index as i64,
                )
                .expect("advance job before restart");
        }
    }
    let old_lease = store
        .acquire_worker_lease("old-owner", 200, 30)
        .expect("acquire old lease")
        .expect("lease available");
    assert!(store
        .release_worker_lease(&old_lease)
        .expect("release old lease"));
    drop(store);

    let reopened = Persistence::open(&database.0).expect("reopen database");
    let new_lease = reopened
        .acquire_worker_lease("new-owner", 300, 200)
        .expect("acquire new lease")
        .expect("released lease available");
    assert_eq!(new_lease.generation, old_lease.generation + 1);
    let actions = reopened
        .recover_jobs_after_restart_leased(301, &new_lease, 301)
        .expect("recover with new lease");
    assert_eq!(
        actions,
        vec![
            (
                "job-deploy-restart".to_owned(),
                yet_another_microsoft_store_lib::jobs::RecoveryAction::ReconcileInventory
            ),
            (
                "job-download-restart".to_owned(),
                yet_another_microsoft_store_lib::jobs::RecoveryAction::ReResolve
            ),
        ]
    );
    let deployed = reopened
        .job_snapshot("job-deploy-restart")
        .unwrap()
        .unwrap();
    let downloading = reopened
        .job_snapshot("job-download-restart")
        .unwrap()
        .unwrap();
    assert_eq!(
        (deployed.sequence, deployed.job.stage),
        (7, JobStage::NeedsReconciliation)
    );
    assert_eq!(
        (downloading.sequence, downloading.job.stage),
        (5, JobStage::Interrupted)
    );

    assert_eq!(
        reopened
            .recover_jobs_after_restart_leased(302, &new_lease, 302)
            .unwrap(),
        actions
    );
    assert_eq!(
        reopened
            .job_snapshot("job-deploy-restart")
            .unwrap()
            .unwrap()
            .sequence,
        7
    );
    assert_eq!(
        reopened
            .job_snapshot("job-download-restart")
            .unwrap()
            .unwrap()
            .sequence,
        5
    );
}

#[test]
fn job_snapshots_include_active_and_terminal_jobs_in_creation_order() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).unwrap();
    created(&store, "job-b");
    store
        .append_job_event("job-b", 1, JobEvent::Cancelled, 101)
        .unwrap();
    created(&store, "job-a");
    store
        .append_job_event(
            "job-a",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            101,
        )
        .unwrap();
    let mut later = job("job-c");
    later.created_at = 200;
    later.updated_at = 200;
    store
        .append_job_event("job-c", 0, JobEvent::Created { job: later }, 200)
        .unwrap();

    let snapshots = store
        .list_job_snapshots()
        .expect("list validated snapshots");
    assert_eq!(
        snapshots
            .iter()
            .map(|snapshot| snapshot.job.job_id.as_str())
            .collect::<Vec<_>>(),
        ["job-a", "job-b", "job-c"]
    );
    assert_eq!(
        snapshots
            .iter()
            .map(|snapshot| snapshot.job.stage)
            .collect::<Vec<_>>(),
        [JobStage::Resolving, JobStage::Cancelled, JobStage::Queued]
    );
    assert_eq!(
        snapshots
            .iter()
            .map(|snapshot| snapshot.sequence)
            .collect::<Vec<_>>(),
        [2, 2, 1]
    );
}

#[test]
fn job_snapshot_list_fails_if_any_projection_or_target_diverges() {
    let database = TestDatabase::new();
    let store = Persistence::open(&database.0).unwrap();
    created(&store, "healthy");
    created(&store, "selected");
    store
        .append_job_event(
            "selected",
            1,
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            },
            101,
        )
        .unwrap();
    store
        .append_job_event(
            "selected",
            2,
            JobEvent::StageChanged {
                stage: JobStage::Selecting,
            },
            102,
        )
        .unwrap();
    store
        .append_job_event(
            "selected",
            3,
            JobEvent::SelectionRecorded {
                selected_update_id: "update-main".to_owned(),
                package_family_name: "Example.App_123".to_owned(),
                version: "1.2.3.4".to_owned(),
                architecture: Architecture::X64,
                language: None,
                targets: vec![JobTarget {
                    role: JobTargetRole::Main,
                    update_id: "update-main".to_owned(),
                    identity_name: "Example.App".to_owned(),
                    publisher: "CN=Example".to_owned(),
                    version: "1.2.3.4".to_owned(),
                    architecture: Architecture::X64,
                    resource_id: None,
                    package_kind: PackageKind::Main,
                    expected_size: 4096,
                    sha256: "a".repeat(64),
                }],
            },
            103,
        )
        .unwrap();
    assert_eq!(store.list_job_snapshots().unwrap().len(), 2);

    let connection = Connection::open(&database.0).unwrap();
    connection
        .execute(
            "UPDATE job_targets SET sha256 = ?1 WHERE job_id = 'selected'",
            ["b".repeat(64)],
        )
        .unwrap();
    assert!(matches!(
        store.list_job_snapshots(),
        Err(PersistenceError::EventHistoryInvalid)
    ));
    connection
        .execute(
            "UPDATE job_targets SET sha256 = ?1 WHERE job_id = 'selected'",
            ["a".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE jobs SET stage = 'failed' WHERE job_id = 'healthy'",
            [],
        )
        .unwrap();
    assert!(matches!(
        store.list_job_snapshots(),
        Err(PersistenceError::EventHistoryInvalid)
    ));
}
