use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashSet;

use crate::{
    job_events::{
        safe_identifier, validate_command, CommandOutcome, DeploymentCheckpoint, JobCommand,
        JobControl, JobEvent, JobEventKind, JobTarget, StoredJobEvent, WorkerLease,
    },
    jobs::{Job, JobSnapshot, JobStage},
    persistence::{load_job, save_job_projection, PersistenceError},
};

pub(crate) fn append(
    connection: &Connection,
    job_id: &str,
    expected_sequence: u64,
    event: JobEvent,
    occurred_at: i64,
) -> Result<JobSnapshot, PersistenceError> {
    let transaction = connection.unchecked_transaction()?;
    let result = append_in_transaction(
        &transaction,
        job_id,
        expected_sequence,
        event,
        occurred_at,
        None,
    )?;
    transaction.commit()?;
    Ok(result)
}

pub(crate) fn append_leased(
    connection: &Connection,
    job_id: &str,
    expected_sequence: u64,
    event: JobEvent,
    occurred_at: i64,
    lease: &WorkerLease,
    now: i64,
) -> Result<JobSnapshot, PersistenceError> {
    let transaction = connection.unchecked_transaction()?;
    let result = append_in_transaction(
        &transaction,
        job_id,
        expected_sequence,
        event,
        occurred_at,
        Some((lease, now)),
    )?;
    transaction.commit()?;
    Ok(result)
}

pub(crate) fn save_deployment_checkpoint_leased(
    connection: &Connection,
    job_id: &str,
    expected_sequence: u64,
    checkpoint: &DeploymentCheckpoint,
    occurred_at: i64,
    lease: &WorkerLease,
    now: i64,
) -> Result<JobSnapshot, PersistenceError> {
    if !checkpoint.is_safe() {
        return Err(PersistenceError::UnsafeJobEvent);
    }
    let transaction = connection.unchecked_transaction()?;
    let snapshot = append_in_transaction(
        &transaction,
        job_id,
        expected_sequence,
        JobEvent::DeploymentCheckpointReady,
        occurred_at,
        Some((lease, now)),
    )?;
    transaction.execute(
        "INSERT INTO deployment_checkpoints (job_id, sequence, checkpoint_json, created_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(job_id) DO UPDATE SET sequence = excluded.sequence,
             checkpoint_json = excluded.checkpoint_json, created_at = excluded.created_at",
        params![
            job_id,
            as_i64(snapshot.sequence, "deployment_checkpoint.sequence")?,
            serde_json::to_string(checkpoint)?,
            occurred_at
        ],
    )?;
    transaction.commit()?;
    Ok(snapshot)
}

pub(crate) fn deployment_checkpoint(
    connection: &Connection,
    job_id: &str,
) -> Result<Option<DeploymentCheckpoint>, PersistenceError> {
    let row: Option<(i64, String)> = connection
        .query_row(
            "SELECT sequence, checkpoint_json FROM deployment_checkpoints WHERE job_id = ?1",
            [job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((sequence, json)) = row else {
        return Ok(None);
    };
    let sequence = as_u64(sequence, "deployment_checkpoint.sequence")?;
    let snapshot =
        current_snapshot(connection, job_id)?.ok_or(PersistenceError::EventHistoryInvalid)?;
    if sequence > snapshot.sequence {
        return Err(PersistenceError::EventHistoryInvalid);
    }
    let event_kind: String = connection.query_row(
        "SELECT event_kind FROM job_events WHERE job_id = ?1 AND sequence = ?2",
        params![job_id, as_i64(sequence, "deployment_checkpoint.sequence")?],
        |row| row.get(0),
    )?;
    if event_kind != "deployment_checkpoint_ready" {
        return Err(PersistenceError::EventHistoryInvalid);
    }
    let checkpoint: DeploymentCheckpoint = serde_json::from_str(&json)?;
    if !checkpoint.is_safe() {
        return Err(PersistenceError::InvalidStoredValue(
            "deployment_checkpoint",
        ));
    }
    Ok(Some(checkpoint))
}

fn append_in_transaction(
    transaction: &Connection,
    job_id: &str,
    expected_sequence: u64,
    event: JobEvent,
    occurred_at: i64,
    lease: Option<(&WorkerLease, i64)>,
) -> Result<JobSnapshot, PersistenceError> {
    if matches!(event, JobEvent::Imported { .. }) {
        return Err(PersistenceError::EventHistoryInvalid);
    }
    if let Some((lease, now)) = lease {
        validate_lease(transaction, lease, now)?;
    } else if !matches!(event, JobEvent::Created { .. }) && has_lease(transaction)? {
        return Err(PersistenceError::LeaseConflict);
    }
    let previous = current_snapshot(transaction, job_id)?;
    let actual = previous.as_ref().map_or(0, |snapshot| snapshot.sequence);
    if actual != expected_sequence {
        return Err(PersistenceError::SequenceConflict {
            expected: expected_sequence,
            actual,
        });
    }
    let job = event.fold(
        previous.as_ref().map(|snapshot| &snapshot.job),
        occurred_at,
        false,
    )?;
    if job.job_id != job_id {
        return Err(PersistenceError::UnsafeJobEvent);
    }
    let sequence = actual
        .checked_add(1)
        .ok_or(PersistenceError::IntegerOutOfRange("job.event_sequence"))?;
    transaction.execute(
        "INSERT INTO job_events
         (job_id, sequence, event_kind, payload_json, projection_json, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            job_id,
            as_i64(sequence, "job.event_sequence")?,
            kind_text(event.kind()),
            serde_json::to_string(&event)?,
            serde_json::to_string(&job)?,
            occurred_at
        ],
    )?;
    if matches!(
        &event,
        JobEvent::StageChanged {
            stage: JobStage::Resolving
        }
    ) {
        transaction.execute("DELETE FROM job_targets WHERE job_id = ?1", [job_id])?;
    }
    if let JobEvent::SelectionRecorded { targets, .. } = &event {
        insert_targets(transaction, job_id, targets)?;
    }
    if matches!(
        &event,
        JobEvent::StageChanged {
            stage: JobStage::Resolving
        } | JobEvent::Failed { .. }
            | JobEvent::Completed
            | JobEvent::Cancelled
    ) {
        transaction.execute(
            "DELETE FROM deployment_checkpoints WHERE job_id = ?1",
            [job_id],
        )?;
    }
    save_job_projection(transaction, &job)?;
    transaction.execute(
        "UPDATE jobs SET event_sequence = ?2 WHERE job_id = ?1",
        params![job_id, as_i64(sequence, "job.event_sequence")?],
    )?;
    Ok(JobSnapshot { job, sequence })
}

pub(crate) fn snapshot(
    connection: &Connection,
    job_id: &str,
) -> Result<Option<JobSnapshot>, PersistenceError> {
    current_snapshot(connection, job_id)
}

fn raw_snapshot(
    connection: &Connection,
    job_id: &str,
) -> Result<Option<JobSnapshot>, PersistenceError> {
    let Some(job) = load_job(connection, job_id)? else {
        return Ok(None);
    };
    let sequence: i64 = connection.query_row(
        "SELECT event_sequence FROM jobs WHERE job_id = ?1",
        [job_id],
        |row| row.get(0),
    )?;
    Ok(Some(JobSnapshot {
        job,
        sequence: as_u64(sequence, "job.event_sequence")?,
    }))
}

fn current_snapshot(
    connection: &Connection,
    job_id: &str,
) -> Result<Option<JobSnapshot>, PersistenceError> {
    let snapshot = raw_snapshot(connection, job_id)?;
    let (count, maximum): (i64, Option<i64>) = connection.query_row(
        "SELECT COUNT(*), MAX(sequence) FROM job_events WHERE job_id = ?1",
        [job_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    match &snapshot {
        None if count == 0 => Ok(None),
        Some(value)
            if value.sequence > 0
                && count == as_i64(value.sequence, "job.event_sequence")?
                && maximum == Some(count) =>
        {
            let tail: (String, String, String) = connection.query_row(
                "SELECT event_kind, payload_json, projection_json FROM job_events
                     WHERE job_id = ?1 AND sequence = ?2",
                params![job_id, count],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            parse_event(&tail.0, &tail.1)?;
            let projected: Job =
                serde_json::from_str(&tail.2).map_err(|_| PersistenceError::EventHistoryInvalid)?;
            if projected == value.job {
                Ok(snapshot)
            } else {
                Err(PersistenceError::EventHistoryInvalid)
            }
        }
        _ => Err(PersistenceError::EventHistoryInvalid),
    }
}

pub(crate) fn list_events(
    connection: &Connection,
    after_cursor: u64,
    limit: usize,
) -> Result<Vec<StoredJobEvent>, PersistenceError> {
    if limit > 1000 {
        return Err(PersistenceError::IntegerOutOfRange("job_events.limit"));
    }
    let mut statement = connection.prepare(
        "SELECT cursor, job_id, sequence, event_kind, payload_json,
                projection_json, occurred_at
         FROM job_events WHERE cursor > ?1 ORDER BY cursor LIMIT ?2",
    )?;
    let rows = statement
        .query_map(
            params![as_i64(after_cursor, "job_events.cursor")?, limit as i64],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut checked = HashSet::new();
    for (_, job_id, _, _, _, _, _) in &rows {
        if checked.insert(job_id.clone()) {
            rebuild(connection, job_id)?;
        }
    }
    rows.into_iter()
        .map(
            |(cursor, job_id, sequence, kind, payload, projection, occurred_at)| {
                let event = parse_event(&kind, &payload)?;
                let sequence = as_u64(sequence, "job_events.sequence")?;
                let snapshot = JobSnapshot {
                    job: serde_json::from_str(&projection)
                        .map_err(|_| PersistenceError::EventHistoryInvalid)?,
                    sequence,
                };
                if snapshot.job.job_id != job_id {
                    return Err(PersistenceError::EventHistoryInvalid);
                }
                Ok(StoredJobEvent {
                    cursor: as_u64(cursor, "job_events.cursor")?,
                    job_id,
                    sequence,
                    event,
                    snapshot,
                    occurred_at,
                })
            },
        )
        .collect()
}

pub(crate) fn rebuild(
    connection: &Connection,
    job_id: &str,
) -> Result<JobSnapshot, PersistenceError> {
    let transaction = connection.unchecked_transaction()?;
    let stored = raw_snapshot(&transaction, job_id)?;
    let mut statement = transaction.prepare(
        "SELECT sequence, event_kind, payload_json, projection_json, occurred_at FROM job_events
         WHERE job_id = ?1 ORDER BY sequence",
    )?;
    let rows = statement
        .query_map([job_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut folded: Option<Job> = None;
    let mut at_projection: Option<Job> = None;
    let mut target_state: Vec<JobTarget> = Vec::new();
    for (index, (sequence, kind, payload, projection, occurred_at)) in rows.iter().enumerate() {
        let wanted = i64::try_from(index + 1)
            .map_err(|_| PersistenceError::IntegerOutOfRange("job_events.sequence"))?;
        if *sequence != wanted {
            return Err(PersistenceError::EventHistoryInvalid);
        }
        let event = parse_event(kind, payload)?;
        folded = Some(event.fold(folded.as_ref(), *occurred_at, true)?);
        match &event {
            JobEvent::StageChanged {
                stage: JobStage::Resolving,
            } => target_state.clear(),
            JobEvent::SelectionRecorded { targets, .. } => target_state.clone_from(targets),
            _ => {}
        }
        if folded.as_ref().is_none_or(|job| job.job_id != job_id) {
            return Err(PersistenceError::EventHistoryInvalid);
        }
        let recorded: Job =
            serde_json::from_str(projection).map_err(|_| PersistenceError::EventHistoryInvalid)?;
        if folded.as_ref() != Some(&recorded) {
            return Err(PersistenceError::EventHistoryInvalid);
        }
        if stored
            .as_ref()
            .is_some_and(|snapshot| snapshot.sequence == index as u64 + 1)
        {
            at_projection = folded.clone();
        }
    }
    if let Some(stored) = &stored {
        if stored.sequence == 0 || at_projection.as_ref() != Some(&stored.job) {
            return Err(PersistenceError::EventHistoryInvalid);
        }
    }
    let job = folded.ok_or(PersistenceError::EventHistoryInvalid)?;
    let sequence = rows.len() as u64;
    target_state.sort_by(|left, right| {
        (role_order(left.role), &left.update_id).cmp(&(role_order(right.role), &right.update_id))
    });
    if stored
        .as_ref()
        .is_none_or(|snapshot| snapshot.sequence < sequence)
    {
        save_job_projection(&transaction, &job)?;
        transaction.execute(
            "UPDATE jobs SET event_sequence = ?2 WHERE job_id = ?1",
            params![job_id, as_i64(sequence, "job.event_sequence")?],
        )?;
        insert_targets(&transaction, job_id, &target_state)?;
    } else if raw_targets(&transaction, job_id)? != target_state {
        return Err(PersistenceError::EventHistoryInvalid);
    }
    transaction.commit()?;
    Ok(JobSnapshot { job, sequence })
}

pub(crate) fn rebuild_all(connection: &Connection) -> Result<(), PersistenceError> {
    list_snapshots(connection).map(|_| ())
}

pub(crate) fn list_snapshots(
    connection: &Connection,
) -> Result<Vec<JobSnapshot>, PersistenceError> {
    let mut statement = connection
        .prepare("SELECT job_id FROM jobs UNION SELECT job_id FROM job_events ORDER BY job_id")?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut snapshots = Vec::with_capacity(ids.len());
    for job_id in ids {
        snapshots.push(rebuild(connection, &job_id)?);
    }
    snapshots.sort_by(|left, right| {
        (left.job.created_at, &left.job.job_id).cmp(&(right.job.created_at, &right.job.job_id))
    });
    Ok(snapshots)
}

pub(crate) fn enqueue_command(
    connection: &Connection,
    command: &JobCommand,
) -> Result<JobCommand, PersistenceError> {
    validate_command(command)?;
    let transaction = connection.unchecked_transaction()?;
    if let Some(existing) = command_by_id(&transaction, &command.command_id)? {
        if existing.job_id == command.job_id
            && existing.control == command.control
            && existing.expected_sequence == command.expected_sequence
        {
            return Ok(existing);
        }
        return Err(PersistenceError::CommandConflict);
    }
    let snapshot = current_snapshot(&transaction, &command.job_id)?
        .ok_or(PersistenceError::EventHistoryInvalid)?;
    if matches!(
        snapshot.job.stage,
        JobStage::Deploying
            | JobStage::NeedsReconciliation
            | JobStage::Completed
            | JobStage::Cancelled
    ) || (snapshot.job.stage == JobStage::AwaitingProcessExit
        && !matches!(
            command.control,
            JobControl::RetryDeployment | JobControl::Cancel
        ))
        || (command.control == JobControl::RetryDeployment
            && snapshot.job.stage != JobStage::AwaitingProcessExit)
    {
        return Err(PersistenceError::CommandConflict);
    }
    transaction.execute(
        "INSERT INTO job_commands (command_id, job_id, control, expected_sequence, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            command.command_id,
            command.job_id,
            control_text(command.control),
            as_i64(command.expected_sequence, "job_command.expected_sequence")?,
            command.created_at
        ],
    )?;
    transaction.commit()?;
    Ok(command.clone())
}

pub(crate) fn pending_commands(
    connection: &Connection,
    job_id: &str,
) -> Result<Vec<JobCommand>, PersistenceError> {
    let mut statement = connection.prepare(
        "SELECT command_id FROM job_commands WHERE job_id = ?1 AND processed_at IS NULL
         ORDER BY created_at, command_id",
    )?;
    let ids = statement
        .query_map([job_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.iter()
        .map(|id| command_by_id(connection, id)?.ok_or(PersistenceError::EventHistoryInvalid))
        .collect()
}

pub(crate) fn finish_command(
    connection: &Connection,
    command_id: &str,
    outcome: CommandOutcome,
    processed_at: i64,
) -> Result<JobCommand, PersistenceError> {
    finish_command_inner(connection, command_id, outcome, processed_at, None)
}

pub(crate) fn finish_command_leased(
    connection: &Connection,
    command_id: &str,
    outcome: CommandOutcome,
    processed_at: i64,
    lease: &WorkerLease,
    now: i64,
) -> Result<JobCommand, PersistenceError> {
    finish_command_inner(
        connection,
        command_id,
        outcome,
        processed_at,
        Some((lease, now)),
    )
}

fn finish_command_inner(
    connection: &Connection,
    command_id: &str,
    outcome: CommandOutcome,
    processed_at: i64,
    lease: Option<(&WorkerLease, i64)>,
) -> Result<JobCommand, PersistenceError> {
    if outcome == CommandOutcome::Applied {
        return Err(PersistenceError::CommandConflict);
    }
    let transaction = connection.unchecked_transaction()?;
    if let Some((lease, now)) = lease {
        validate_lease(&transaction, lease, now)?;
    }
    let existing =
        command_by_id(&transaction, command_id)?.ok_or(PersistenceError::CommandConflict)?;
    if existing.outcome.is_some() {
        if existing.outcome == Some(outcome) {
            return Ok(existing);
        }
        return Err(PersistenceError::CommandConflict);
    }
    transaction.execute(
        "UPDATE job_commands SET outcome_json = ?2, processed_at = ?3 WHERE command_id = ?1",
        params![command_id, serde_json::to_string(&outcome)?, processed_at],
    )?;
    transaction.commit()?;
    command_by_id(connection, command_id)?.ok_or(PersistenceError::EventHistoryInvalid)
}

pub(crate) fn apply_command(
    connection: &Connection,
    command_id: &str,
    event: JobEvent,
    outcome: CommandOutcome,
    occurred_at: i64,
    lease: &WorkerLease,
    now: i64,
) -> Result<JobSnapshot, PersistenceError> {
    if outcome != CommandOutcome::Applied {
        return Err(PersistenceError::CommandConflict);
    }
    let transaction = connection.unchecked_transaction()?;
    let command =
        command_by_id(&transaction, command_id)?.ok_or(PersistenceError::CommandConflict)?;
    validate_lease(&transaction, lease, now)?;
    if let Some(recorded) = command.outcome {
        if recorded == outcome {
            return current_snapshot(&transaction, &command.job_id)?
                .ok_or(PersistenceError::EventHistoryInvalid);
        }
        return Err(PersistenceError::CommandConflict);
    }
    let matching = matches!(
        (command.control, &event),
        (JobControl::Cancel, JobEvent::Cancelled)
            | (
                JobControl::Pause,
                JobEvent::StageChanged {
                    stage: JobStage::Paused
                }
            )
            | (
                JobControl::Resume,
                JobEvent::StageChanged {
                    stage: JobStage::Resolving
                }
            )
            | (
                JobControl::RetryDeployment,
                JobEvent::StageChanged {
                    stage: JobStage::Preparing
                }
            )
    );
    if !matching {
        return Err(PersistenceError::CommandConflict);
    }
    let snapshot = append_in_transaction(
        &transaction,
        &command.job_id,
        command.expected_sequence,
        event,
        occurred_at,
        Some((lease, now)),
    )?;
    transaction.execute(
        "UPDATE job_commands SET outcome_json = ?2, processed_at = ?3 WHERE command_id = ?1",
        params![command_id, serde_json::to_string(&outcome)?, occurred_at],
    )?;
    transaction.commit()?;
    Ok(snapshot)
}

pub(crate) fn acquire_lease(
    connection: &Connection,
    owner_id: &str,
    now: i64,
    ttl: i64,
) -> Result<Option<WorkerLease>, PersistenceError> {
    if !safe_identifier(owner_id) || ttl <= 0 {
        return Err(PersistenceError::LeaseConflict);
    }
    let expires_at = now
        .checked_add(ttl)
        .ok_or(PersistenceError::LeaseConflict)?;
    let transaction = connection.unchecked_transaction()?;
    let prior: Option<(i64, i64)> = transaction
        .query_row(
            "SELECT generation, expires_at FROM worker_leases WHERE lease_key = 'worker'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let generation = match prior {
        None => 1,
        Some((_, expires)) if expires > now => return Ok(None),
        Some((generation, _)) => generation
            .checked_add(1)
            .ok_or(PersistenceError::LeaseConflict)?,
    };
    transaction.execute(
        "INSERT INTO worker_leases (lease_key, owner_id, generation, expires_at)
         VALUES ('worker', ?1, ?2, ?3)
         ON CONFLICT(lease_key) DO UPDATE SET owner_id = excluded.owner_id,
             generation = excluded.generation, expires_at = excluded.expires_at",
        params![owner_id, generation, expires_at],
    )?;
    transaction.commit()?;
    Ok(Some(WorkerLease {
        owner_id: owner_id.to_owned(),
        generation: as_u64(generation, "worker_lease.generation")?,
        expires_at,
    }))
}

pub(crate) fn renew_lease(
    connection: &Connection,
    lease: &WorkerLease,
    now: i64,
    ttl: i64,
) -> Result<Option<WorkerLease>, PersistenceError> {
    if ttl <= 0 {
        return Err(PersistenceError::LeaseConflict);
    }
    let expires_at = now
        .checked_add(ttl)
        .ok_or(PersistenceError::LeaseConflict)?;
    let changed = connection.execute(
        "UPDATE worker_leases SET expires_at = ?3 WHERE lease_key = 'worker' AND owner_id = ?1
         AND generation = ?2 AND expires_at > ?4",
        params![
            lease.owner_id,
            as_i64(lease.generation, "worker_lease.generation")?,
            expires_at,
            now
        ],
    )?;
    Ok((changed == 1).then(|| WorkerLease {
        expires_at,
        ..lease.clone()
    }))
}

pub(crate) fn release_lease(
    connection: &Connection,
    lease: &WorkerLease,
) -> Result<bool, PersistenceError> {
    Ok(connection.execute(
        "UPDATE worker_leases SET expires_at = 0
         WHERE lease_key = 'worker' AND owner_id = ?1 AND generation = ?2",
        params![
            lease.owner_id,
            as_i64(lease.generation, "worker_lease.generation")?
        ],
    )? == 1)
}

pub(crate) fn has_lease(connection: &Connection) -> Result<bool, PersistenceError> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM worker_leases WHERE lease_key = 'worker')",
        [],
        |row| row.get::<_, i64>(0),
    )? == 1)
}

pub(crate) fn validate_lease(
    connection: &Connection,
    lease: &WorkerLease,
    now: i64,
) -> Result<(), PersistenceError> {
    if lease.expires_at <= now {
        return Err(PersistenceError::LeaseConflict);
    }
    let valid: i64 = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM worker_leases WHERE lease_key = 'worker' AND owner_id = ?1
         AND generation = ?2 AND expires_at > ?3)",
        params![
            lease.owner_id,
            as_i64(lease.generation, "worker_lease.generation")?,
            now
        ],
        |row| row.get(0),
    )?;
    if valid == 1 {
        Ok(())
    } else {
        Err(PersistenceError::LeaseConflict)
    }
}

fn insert_targets(
    connection: &Connection,
    job_id: &str,
    targets: &[JobTarget],
) -> Result<(), PersistenceError> {
    connection.execute("DELETE FROM job_targets WHERE job_id = ?1", [job_id])?;
    for target in targets {
        connection.execute(
            "INSERT INTO job_targets (job_id, role, update_id, identity_name, publisher,
                version, architecture, resource_id, package_kind, expected_size, sha256)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                job_id,
                enum_text(target.role)?,
                target.update_id,
                target.identity_name,
                target.publisher,
                target.version,
                enum_text(target.architecture)?,
                target.resource_id,
                enum_text(target.package_kind)?,
                as_i64(target.expected_size, "job_target.expected_size")?,
                target.sha256
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn targets(
    connection: &Connection,
    job_id: &str,
) -> Result<Vec<JobTarget>, PersistenceError> {
    rebuild(connection, job_id)?;
    raw_targets(connection, job_id)
}

fn role_order(role: crate::job_events::JobTargetRole) -> u8 {
    match role {
        crate::job_events::JobTargetRole::Dependency => 0,
        crate::job_events::JobTargetRole::Main => 1,
        crate::job_events::JobTargetRole::Resource => 2,
    }
}

fn raw_targets(connection: &Connection, job_id: &str) -> Result<Vec<JobTarget>, PersistenceError> {
    let mut statement = connection.prepare(
        "SELECT role, update_id, identity_name, publisher, version, architecture,
            resource_id, package_kind, expected_size, sha256 FROM job_targets
         WHERE job_id = ?1 ORDER BY role, update_id",
    )?;
    let rows = statement
        .query_map([job_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, String>(9)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(
                role,
                update_id,
                identity_name,
                publisher,
                version,
                architecture,
                resource_id,
                package_kind,
                expected_size,
                sha256,
            )| {
                Ok(JobTarget {
                    role: parse_enum(&role)?,
                    update_id,
                    identity_name,
                    publisher,
                    version,
                    architecture: parse_enum(&architecture)?,
                    resource_id,
                    package_kind: parse_enum(&package_kind)?,
                    expected_size: as_u64(expected_size, "job_target.expected_size")?,
                    sha256,
                })
            },
        )
        .collect()
}

fn enum_text<T: Serialize>(value: T) -> Result<String, PersistenceError> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(value) => Ok(value),
        _ => Err(PersistenceError::EventHistoryInvalid),
    }
}

fn parse_enum<T: DeserializeOwned>(value: &str) -> Result<T, PersistenceError> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|_| PersistenceError::EventHistoryInvalid)
}

fn command_by_id(
    connection: &Connection,
    command_id: &str,
) -> Result<Option<JobCommand>, PersistenceError> {
    type CommandRow = (
        String,
        String,
        String,
        i64,
        i64,
        Option<i64>,
        Option<String>,
    );
    let raw: Option<CommandRow> = connection
        .query_row(
            "SELECT command_id, job_id, control, expected_sequence, created_at,
                    processed_at, outcome_json FROM job_commands WHERE command_id = ?1",
            [command_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()?;
    raw.map(
        |(command_id, job_id, control, expected_sequence, created_at, processed_at, outcome)| {
            Ok(JobCommand {
                command_id,
                job_id,
                control: parse_control(&control)?,
                expected_sequence: as_u64(expected_sequence, "job_command.expected_sequence")?,
                created_at,
                processed_at,
                outcome: outcome
                    .as_deref()
                    .map(serde_json::from_str)
                    .transpose()
                    .map_err(|_| PersistenceError::EventHistoryInvalid)?,
            })
        },
    )
    .transpose()
}

fn control_text(control: JobControl) -> &'static str {
    match control {
        JobControl::Pause => "pause",
        JobControl::Resume => "resume",
        JobControl::RetryDeployment => "retry_deployment",
        JobControl::Cancel => "cancel",
    }
}

fn parse_control(value: &str) -> Result<JobControl, PersistenceError> {
    match value {
        "pause" => Ok(JobControl::Pause),
        "resume" => Ok(JobControl::Resume),
        "retry_deployment" => Ok(JobControl::RetryDeployment),
        "cancel" => Ok(JobControl::Cancel),
        _ => Err(PersistenceError::InvalidStoredValue("job_command.control")),
    }
}

fn parse_event(kind: &str, payload: &str) -> Result<JobEvent, PersistenceError> {
    let event: JobEvent =
        serde_json::from_str(payload).map_err(|_| PersistenceError::EventHistoryInvalid)?;
    if kind_text(event.kind()) != kind {
        return Err(PersistenceError::EventHistoryInvalid);
    }
    Ok(event)
}

fn kind_text(kind: JobEventKind) -> &'static str {
    match kind {
        JobEventKind::Created => "created",
        JobEventKind::Imported => "imported",
        JobEventKind::StageChanged => "stage_changed",
        JobEventKind::ProgressRecorded => "progress_recorded",
        JobEventKind::DeploymentProgressRecorded => "deployment_progress_recorded",
        JobEventKind::DeploymentCheckpointReady => "deployment_checkpoint_ready",
        JobEventKind::DeploymentBlocked => "deployment_blocked",
        JobEventKind::SelectionRecorded => "selection_recorded",
        JobEventKind::Failed => "failed",
        JobEventKind::Completed => "completed",
        JobEventKind::Cancelled => "cancelled",
        JobEventKind::Recovered => "recovered",
    }
}

fn as_i64(value: u64, field: &'static str) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::IntegerOutOfRange(field))
}

fn as_u64(value: i64, field: &'static str) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| PersistenceError::InvalidStoredValue(field))
}
