use std::{
    fmt,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    deployment::DeploymentScope,
    domain::{
        AppSettings, Architecture, CacheEntry, CacheState, DiagnosticEvent, InstallObservation,
        InstallSource, PackageDependency, PackageFormat, PackageKind, PackageRecord,
        PackageVersion, ProductRecord,
    },
    error::{AppErrorDto, ErrorCode},
    identity::{AssociationConfidence, PackageAssociation},
    job_events::{CommandOutcome, JobCommand, JobEvent, JobTarget, StoredJobEvent, WorkerLease},
    job_store,
    jobs::{Job, JobKind, JobSnapshot, JobStage, RecoveryAction},
};

const CURRENT_SCHEMA_VERSION: i64 = 1;
const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("../migrations/0001_initial.sql"))];

#[derive(Debug)]
pub enum PersistenceError {
    Sqlite(rusqlite::Error),
    Serialization(serde_json::Error),
    IntegerOutOfRange(&'static str),
    InvalidStoredValue(&'static str),
    UnsupportedSchema(i64),
    SequenceConflict { expected: u64, actual: u64 },
    EventHistoryInvalid,
    UnsafeJobEvent,
    CommandConflict,
    LeaseConflict,
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(_) => formatter.write_str("SQLite operation failed"),
            Self::Serialization(_) => formatter.write_str("persistent data serialization failed"),
            Self::IntegerOutOfRange(field) => {
                write!(formatter, "value is too large for SQLite: {field}")
            }
            Self::InvalidStoredValue(field) => {
                write!(formatter, "stored value is invalid: {field}")
            }
            Self::UnsupportedSchema(version) => {
                write!(
                    formatter,
                    "database schema version {version} is newer than supported"
                )
            }
            Self::SequenceConflict { .. } => formatter.write_str("job sequence conflict"),
            Self::EventHistoryInvalid => formatter.write_str("job event history is invalid"),
            Self::UnsafeJobEvent => formatter.write_str("job event contains unsafe data"),
            Self::CommandConflict => formatter.write_str("job command conflict"),
            Self::LeaseConflict => formatter.write_str("worker lease conflict"),
        }
    }
}

impl std::error::Error for PersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Serialization(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for PersistenceError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for PersistenceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

pub struct Persistence {
    connection: Connection,
}

impl Persistence {
    pub fn open(path: &Path) -> Result<Self, PersistenceError> {
        let mut connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
        migrate(&mut connection)?;
        job_store::rebuild_all(&connection)?;
        Ok(Self { connection })
    }

    pub fn schema_version(&self) -> Result<i64, PersistenceError> {
        Ok(self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    pub fn upsert_product(&self, product: &ProductRecord) -> Result<(), PersistenceError> {
        let languages = serde_json::to_string(&product.languages)?;
        self.connection.execute(
            "INSERT INTO products (
                product_id, package_family_name, app_name, publisher, market, languages_json, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(product_id) DO UPDATE SET
                package_family_name = excluded.package_family_name,
                app_name = excluded.app_name,
                publisher = excluded.publisher,
                market = excluded.market,
                languages_json = excluded.languages_json,
                updated_at = excluded.updated_at",
            params![
                product.product_id,
                product.package_family_name,
                product.app_name,
                product.publisher,
                product.market,
                languages,
                product.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn product(&self, product_id: &str) -> Result<Option<ProductRecord>, PersistenceError> {
        type Row = (
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            String,
            i64,
        );
        let row: Option<Row> = self
            .connection
            .query_row(
                "SELECT product_id, package_family_name, app_name, publisher, market,
                        languages_json, updated_at
                 FROM products WHERE product_id = ?1",
                [product_id],
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
        row.map(
            |(
                product_id,
                package_family_name,
                app_name,
                publisher,
                market,
                languages,
                updated_at,
            )| {
                Ok(ProductRecord {
                    product_id,
                    package_family_name,
                    app_name,
                    publisher,
                    market,
                    languages: serde_json::from_str(&languages)?,
                    updated_at,
                })
            },
        )
        .transpose()
    }

    pub fn upsert_package(&self, package: &PackageRecord) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO package_versions (
                update_id, product_id, package_family_name, package_moniker, identity_name,
                publisher, resource_id, package_kind, version, architecture, language, market,
                format, minimum_os_version, is_neutral, content_id, file_size, sha256,
                install_source
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18, ?19
             )
             ON CONFLICT(update_id) DO UPDATE SET
                product_id = excluded.product_id,
                package_family_name = excluded.package_family_name,
                package_moniker = excluded.package_moniker,
                identity_name = excluded.identity_name,
                publisher = excluded.publisher,
                resource_id = excluded.resource_id,
                package_kind = excluded.package_kind,
                version = excluded.version,
                architecture = excluded.architecture,
                language = excluded.language,
                market = excluded.market,
                format = excluded.format,
                minimum_os_version = excluded.minimum_os_version,
                is_neutral = excluded.is_neutral,
                content_id = excluded.content_id,
                file_size = excluded.file_size,
                sha256 = excluded.sha256,
                install_source = excluded.install_source",
            params![
                package.update_id,
                package.product_id,
                package.package_family_name,
                package.package_moniker,
                package.identity_name,
                package.publisher,
                package.resource_id,
                enum_text(package.package_kind)?,
                package.version.to_string(),
                enum_text(package.architecture)?,
                package.language,
                package.market,
                enum_text(package.format)?,
                package
                    .minimum_os_version
                    .map(|version| version.to_string()),
                package.is_neutral.map(i64::from),
                package.content_id,
                package
                    .file_size
                    .map(|value| to_i64(value, "file_size"))
                    .transpose()?,
                package.sha256,
                enum_text(package.install_source)?
            ],
        )?;
        Ok(())
    }

    pub fn package(&self, update_id: &str) -> Result<Option<PackageRecord>, PersistenceError> {
        let row: Option<PackageRow> = self
            .connection
            .query_row(
                "SELECT update_id, product_id, package_family_name, package_moniker,
                        identity_name, publisher, resource_id, package_kind, version,
                        architecture, language, market, format, minimum_os_version,
                        is_neutral, content_id, file_size, sha256, install_source
                 FROM package_versions WHERE update_id = ?1",
                [update_id],
                read_package_row,
            )
            .optional()?;
        row.map(PackageRecord::try_from).transpose()
    }

    pub fn replace_dependencies(
        &self,
        source_update_id: &str,
        dependencies: &[PackageDependency],
    ) -> Result<(), PersistenceError> {
        if dependencies
            .iter()
            .any(|dependency| dependency.source_update_id != source_update_id)
        {
            return Err(PersistenceError::InvalidStoredValue(
                "dependency.source_update_id",
            ));
        }
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM package_dependencies WHERE source_update_id = ?1",
            [source_update_id],
        )?;
        for dependency in dependencies {
            transaction.execute(
                "INSERT INTO package_dependencies (
                    source_update_id, target_update_id, kind
                 ) VALUES (?1, ?2, ?3)",
                params![
                    dependency.source_update_id,
                    dependency.target_update_id,
                    enum_text(dependency.kind)?
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn dependencies(
        &self,
        source_update_id: &str,
    ) -> Result<Vec<PackageDependency>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT source_update_id, target_update_id, kind
             FROM package_dependencies
             WHERE source_update_id = ?1
             ORDER BY target_update_id, kind",
        )?;
        let rows = statement
            .query_map([source_update_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(source_update_id, target_update_id, kind)| {
                Ok(PackageDependency {
                    source_update_id,
                    target_update_id,
                    kind: parse_enum(&kind, "dependency.kind")?,
                })
            })
            .collect()
    }

    pub fn save_job(&self, job: &Job) -> Result<(), PersistenceError> {
        let sequence = job_store::snapshot(&self.connection, &job.job_id)?
            .map_or(0, |snapshot| snapshot.sequence);
        let event = if sequence == 0 {
            JobEvent::Created { job: job.clone() }
        } else {
            return Err(PersistenceError::EventHistoryInvalid);
        };
        self.append_job_event(&job.job_id, sequence, event, job.updated_at)?;
        Ok(())
    }

    pub fn append_job_event(
        &self,
        job_id: &str,
        expected_sequence: u64,
        event: JobEvent,
        occurred_at: i64,
    ) -> Result<JobSnapshot, PersistenceError> {
        job_store::append(
            &self.connection,
            job_id,
            expected_sequence,
            event,
            occurred_at,
        )
    }

    pub fn job_snapshot(&self, job_id: &str) -> Result<Option<JobSnapshot>, PersistenceError> {
        job_store::snapshot(&self.connection, job_id)
    }

    pub fn list_job_snapshots(&self) -> Result<Vec<JobSnapshot>, PersistenceError> {
        job_store::list_snapshots(&self.connection)
    }

    pub fn list_job_events(
        &self,
        after_cursor: u64,
        limit: usize,
    ) -> Result<Vec<StoredJobEvent>, PersistenceError> {
        job_store::list_events(&self.connection, after_cursor, limit)
    }

    pub fn rebuild_job_projection(&self, job_id: &str) -> Result<JobSnapshot, PersistenceError> {
        job_store::rebuild(&self.connection, job_id)
    }

    pub fn enqueue_job_command(
        &self,
        command: &JobCommand,
    ) -> Result<JobCommand, PersistenceError> {
        job_store::enqueue_command(&self.connection, command)
    }

    pub fn pending_job_commands(&self, job_id: &str) -> Result<Vec<JobCommand>, PersistenceError> {
        job_store::pending_commands(&self.connection, job_id)
    }

    pub fn finish_job_command(
        &self,
        command_id: &str,
        outcome: CommandOutcome,
        processed_at: i64,
    ) -> Result<JobCommand, PersistenceError> {
        job_store::finish_command(&self.connection, command_id, outcome, processed_at)
    }

    pub fn finish_job_command_leased(
        &self,
        command_id: &str,
        outcome: CommandOutcome,
        processed_at: i64,
        lease: &WorkerLease,
        now: i64,
    ) -> Result<JobCommand, PersistenceError> {
        job_store::finish_command_leased(
            &self.connection,
            command_id,
            outcome,
            processed_at,
            lease,
            now,
        )
    }

    pub fn apply_job_command(
        &self,
        command_id: &str,
        event: JobEvent,
        outcome: CommandOutcome,
        occurred_at: i64,
        lease: &WorkerLease,
        now: i64,
    ) -> Result<JobSnapshot, PersistenceError> {
        job_store::apply_command(
            &self.connection,
            command_id,
            event,
            outcome,
            occurred_at,
            lease,
            now,
        )
    }

    pub fn acquire_worker_lease(
        &self,
        owner_id: &str,
        now: i64,
        ttl: i64,
    ) -> Result<Option<WorkerLease>, PersistenceError> {
        job_store::acquire_lease(&self.connection, owner_id, now, ttl)
    }

    pub fn renew_worker_lease(
        &self,
        lease: &WorkerLease,
        now: i64,
        ttl: i64,
    ) -> Result<Option<WorkerLease>, PersistenceError> {
        job_store::renew_lease(&self.connection, lease, now, ttl)
    }

    pub fn release_worker_lease(&self, lease: &WorkerLease) -> Result<bool, PersistenceError> {
        job_store::release_lease(&self.connection, lease)
    }

    pub fn append_job_event_leased(
        &self,
        job_id: &str,
        expected_sequence: u64,
        event: JobEvent,
        occurred_at: i64,
        lease: &WorkerLease,
        now: i64,
    ) -> Result<JobSnapshot, PersistenceError> {
        job_store::append_leased(
            &self.connection,
            job_id,
            expected_sequence,
            event,
            occurred_at,
            lease,
            now,
        )
    }

    pub fn job_targets(&self, job_id: &str) -> Result<Vec<JobTarget>, PersistenceError> {
        job_store::targets(&self.connection, job_id)
    }

    pub fn job(&self, job_id: &str) -> Result<Option<Job>, PersistenceError> {
        Ok(self.job_snapshot(job_id)?.map(|snapshot| snapshot.job))
    }

    pub fn recover_jobs_after_restart(
        &self,
        updated_at: i64,
    ) -> Result<Vec<(String, RecoveryAction)>, PersistenceError> {
        self.recover_jobs_after_restart_inner(updated_at, None)
    }

    pub fn recover_jobs_after_restart_leased(
        &self,
        updated_at: i64,
        lease: &WorkerLease,
        now: i64,
    ) -> Result<Vec<(String, RecoveryAction)>, PersistenceError> {
        self.recover_jobs_after_restart_inner(updated_at, Some((lease, now)))
    }

    fn recover_jobs_after_restart_inner(
        &self,
        updated_at: i64,
        lease: Option<(&WorkerLease, i64)>,
    ) -> Result<Vec<(String, RecoveryAction)>, PersistenceError> {
        match lease {
            Some((lease, now)) => job_store::validate_lease(&self.connection, lease, now)?,
            None if job_store::has_lease(&self.connection)? => {
                return Err(PersistenceError::LeaseConflict);
            }
            None => {}
        }
        let mut statement = self.connection.prepare(
            "SELECT job_id, kind, product_id, requested_market,
                    requested_architectures_json, requested_languages_json, deployment_scope,
                    selected_update_id, package_family_name, stage, bytes_done, bytes_total,
                    version, architecture, language, error_json,
                    created_at, updated_at
             FROM jobs ORDER BY job_id",
        )?;
        let rows = statement
            .query_map([], read_job_row)?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        let mut recovered = Vec::new();
        let mut changed = Vec::new();
        for row in rows {
            let mut job = Job::try_from(row)?;
            let old_stage = job.stage;
            let action = job.recover_after_restart(updated_at);
            if action != RecoveryAction::None {
                recovered.push((job.job_id.clone(), action));
                if job.stage != old_stage {
                    changed.push(job);
                }
            }
        }

        for job in &changed {
            let snapshot = job_store::snapshot(&self.connection, &job.job_id)?
                .ok_or(PersistenceError::EventHistoryInvalid)?;
            let event = JobEvent::Recovered { stage: job.stage };
            match lease {
                Some((lease, now)) => {
                    self.append_job_event_leased(
                        &job.job_id,
                        snapshot.sequence,
                        event,
                        updated_at,
                        lease,
                        now,
                    )?;
                }
                None => {
                    self.append_job_event(&job.job_id, snapshot.sequence, event, updated_at)?;
                }
            }
        }
        Ok(recovered)
    }

    pub fn upsert_cache_entry(&self, entry: &CacheEntry) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO cache_entries (
                cache_key, job_id, update_id, path, size, sha256, state, last_accessed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(cache_key) DO UPDATE SET
                job_id = excluded.job_id,
                update_id = excluded.update_id,
                path = excluded.path,
                size = excluded.size,
                sha256 = excluded.sha256,
                state = excluded.state,
                last_accessed_at = excluded.last_accessed_at",
            params![
                entry.cache_key,
                entry.job_id,
                entry.update_id,
                entry.path,
                to_i64(entry.size, "cache.size")?,
                entry.sha256,
                enum_text(entry.state)?,
                entry.last_accessed_at
            ],
        )?;
        Ok(())
    }

    pub fn cache_entry(&self, cache_key: &str) -> Result<Option<CacheEntry>, PersistenceError> {
        type Row = (
            String,
            Option<String>,
            String,
            String,
            i64,
            String,
            String,
            i64,
        );
        let row: Option<Row> = self
            .connection
            .query_row(
                "SELECT cache_key, job_id, update_id, path, size, sha256, state,
                        last_accessed_at
                 FROM cache_entries WHERE cache_key = ?1",
                [cache_key],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(cache_key, job_id, update_id, path, size, sha256, state, last_accessed_at)| {
                Ok(CacheEntry {
                    cache_key,
                    job_id,
                    update_id,
                    path,
                    size: from_i64(size, "cache.size")?,
                    sha256,
                    state: parse_enum::<CacheState>(&state, "cache.state")?,
                    last_accessed_at,
                })
            },
        )
        .transpose()
    }

    pub fn cache_entries(&self) -> Result<Vec<CacheEntry>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT cache_key, job_id, update_id, path, size, sha256, state,
                    last_accessed_at
             FROM cache_entries ORDER BY cache_key",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?;
        rows.map(|row| {
            let (cache_key, job_id, update_id, path, size, sha256, state, last_accessed_at) = row?;
            Ok(CacheEntry {
                cache_key,
                job_id,
                update_id,
                path,
                size: from_i64(size, "cache.size")?,
                sha256,
                state: parse_enum::<CacheState>(&state, "cache.state")?,
                last_accessed_at,
            })
        })
        .collect()
    }

    pub fn delete_cache_entry(&self, cache_key: &str) -> Result<(), PersistenceError> {
        self.connection.execute(
            "DELETE FROM cache_entries WHERE cache_key = ?1",
            [cache_key],
        )?;
        Ok(())
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO settings (key, value_json, updated_at)
             VALUES ('application', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at = excluded.updated_at",
            params![serde_json::to_string(settings)?, now_unix_seconds()],
        )?;
        Ok(())
    }

    pub fn settings(&self) -> Result<Option<AppSettings>, PersistenceError> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT value_json FROM settings WHERE key = 'application'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|value| serde_json::from_str(&value).map_err(PersistenceError::from))
            .transpose()
    }

    pub fn record_install_observation(
        &self,
        observation: &InstallObservation,
    ) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO install_observations (
                package_family_name, product_id, source, observed_at
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(package_family_name) DO UPDATE SET
                product_id = excluded.product_id,
                source = excluded.source,
                observed_at = excluded.observed_at",
            params![
                observation.package_family_name,
                observation.product_id,
                enum_text(observation.source)?,
                observation.observed_at
            ],
        )?;
        Ok(())
    }

    pub fn install_observation(
        &self,
        package_family_name: &str,
    ) -> Result<Option<InstallObservation>, PersistenceError> {
        let row: Option<(String, Option<String>, String, i64)> = self
            .connection
            .query_row(
                "SELECT package_family_name, product_id, source, observed_at
                 FROM install_observations WHERE package_family_name = ?1",
                [package_family_name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        row.map(|(package_family_name, product_id, source, observed_at)| {
            Ok(InstallObservation {
                package_family_name,
                product_id,
                source: parse_enum::<InstallSource>(&source, "install.source")?,
                observed_at,
            })
        })
        .transpose()
    }

    pub fn upsert_package_association(
        &self,
        association: &PackageAssociation,
    ) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO package_associations (
                package_family_name, product_id, content_id, identity_name, publisher,
                confidence, observed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(package_family_name) DO UPDATE SET
                product_id = excluded.product_id,
                content_id = excluded.content_id,
                identity_name = excluded.identity_name,
                publisher = excluded.publisher,
                confidence = excluded.confidence,
                observed_at = excluded.observed_at",
            params![
                association.package_family_name,
                association.product_id,
                association.content_id,
                association.identity_name,
                association.publisher,
                enum_text(association.confidence)?,
                association.observed_at,
            ],
        )?;
        Ok(())
    }

    pub fn package_association(
        &self,
        package_family_name: &str,
    ) -> Result<Option<PackageAssociation>, PersistenceError> {
        let row: Option<PackageAssociationRow> = self
            .connection
            .query_row(
                "SELECT package_family_name, product_id, content_id, identity_name,
                            publisher, confidence, observed_at
                     FROM package_associations WHERE package_family_name = ?1",
                [package_family_name],
                read_package_association_row,
            )
            .optional()?;
        row.map(PackageAssociation::try_from).transpose()
    }

    pub fn record_deployment_success(
        &self,
        association: &PackageAssociation,
        observation: &InstallObservation,
    ) -> Result<(), PersistenceError> {
        if association.package_family_name != observation.package_family_name
            || association.product_id != observation.product_id
            || observation.source != InstallSource::ThisClient
        {
            return Err(PersistenceError::InvalidStoredValue(
                "deployment_success.identity",
            ));
        }
        let transaction = self.connection.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO package_associations (
                package_family_name, product_id, content_id, identity_name, publisher,
                confidence, observed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(package_family_name) DO UPDATE SET
                product_id = excluded.product_id,
                content_id = excluded.content_id,
                identity_name = excluded.identity_name,
                publisher = excluded.publisher,
                confidence = excluded.confidence,
                observed_at = excluded.observed_at",
            params![
                association.package_family_name,
                association.product_id,
                association.content_id,
                association.identity_name,
                association.publisher,
                enum_text(association.confidence)?,
                association.observed_at,
            ],
        )?;
        transaction.execute(
            "INSERT INTO install_observations (
                package_family_name, product_id, source, observed_at
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(package_family_name) DO UPDATE SET
                product_id = excluded.product_id,
                source = excluded.source,
                observed_at = excluded.observed_at",
            params![
                observation.package_family_name,
                observation.product_id,
                enum_text(observation.source)?,
                observation.observed_at,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn record_diagnostic(&self, diagnostic: &DiagnosticEvent) -> Result<(), PersistenceError> {
        self.connection.execute(
            "INSERT INTO diagnostics (
                job_id, code, stage, operation, os_error_code, retryable, occurred_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                diagnostic.job_id,
                enum_text(diagnostic.code)?,
                diagnostic.stage.map(enum_text).transpose()?,
                enum_text(diagnostic.operation)?,
                diagnostic.os_error_code,
                i64::from(diagnostic.retryable),
                diagnostic.occurred_at
            ],
        )?;
        Ok(())
    }

    pub fn diagnostic_count(&self, code: ErrorCode) -> Result<u64, PersistenceError> {
        let count: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM diagnostics WHERE code = ?1",
            [enum_text(code)?],
            |row| row.get(0),
        )?;
        from_i64(count, "diagnostics.count")
    }
}

fn migrate(connection: &mut Connection) -> Result<(), PersistenceError> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(PersistenceError::UnsupportedSchema(version));
    }
    for (migration_version, sql) in MIGRATIONS
        .iter()
        .filter(|(migration_version, _)| *migration_version > version)
    {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(sql)?;
        transaction.pragma_update(None, "user_version", migration_version)?;
        transaction.commit()?;
    }
    Ok(())
}

pub(crate) fn save_job_projection(
    connection: &Connection,
    job: &Job,
) -> Result<(), PersistenceError> {
    let error_json = job.error.as_ref().map(serde_json::to_string).transpose()?;
    let requested_architectures = serde_json::to_string(&job.requested_architectures)?;
    let requested_languages = serde_json::to_string(&job.requested_languages)?;
    connection.execute(
        "INSERT INTO jobs (
            job_id, kind, product_id, requested_market, requested_architectures_json,
            requested_languages_json, deployment_scope, selected_update_id,
            package_family_name, stage, bytes_done, bytes_total, version, architecture,
            language, error_json, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
            ?17, ?18
         )
         ON CONFLICT(job_id) DO UPDATE SET
            kind = excluded.kind,
            product_id = excluded.product_id,
            requested_market = excluded.requested_market,
            requested_architectures_json = excluded.requested_architectures_json,
            requested_languages_json = excluded.requested_languages_json,
            deployment_scope = excluded.deployment_scope,
            selected_update_id = excluded.selected_update_id,
            package_family_name = excluded.package_family_name,
            stage = excluded.stage,
            bytes_done = excluded.bytes_done,
            bytes_total = excluded.bytes_total,
            version = excluded.version,
            architecture = excluded.architecture,
            language = excluded.language,
            error_json = excluded.error_json,
            updated_at = excluded.updated_at",
        params![
            job.job_id,
            enum_text(job.kind)?,
            job.product_id,
            job.requested_market,
            requested_architectures,
            requested_languages,
            enum_text(job.deployment_scope)?,
            job.selected_update_id,
            job.package_family_name,
            enum_text(job.stage)?,
            to_i64(job.bytes_done, "job.bytes_done")?,
            job.bytes_total
                .map(|value| to_i64(value, "job.bytes_total"))
                .transpose()?,
            job.version,
            job.architecture.map(enum_text).transpose()?,
            job.language,
            error_json,
            job.created_at,
            job.updated_at
        ],
    )?;
    Ok(())
}

pub(crate) fn load_job(
    connection: &Connection,
    job_id: &str,
) -> Result<Option<Job>, PersistenceError> {
    let row: Option<JobRow> = connection
        .query_row(
            "SELECT job_id, kind, product_id, requested_market,
                requested_architectures_json, requested_languages_json, deployment_scope,
                selected_update_id, package_family_name, stage, bytes_done, bytes_total,
                version, architecture, language, error_json,
                created_at, updated_at FROM jobs WHERE job_id = ?1",
            [job_id],
            read_job_row,
        )
        .optional()?;
    row.map(Job::try_from).transpose()
}

struct PackageRow {
    update_id: String,
    product_id: String,
    package_family_name: Option<String>,
    package_moniker: String,
    identity_name: Option<String>,
    publisher: Option<String>,
    resource_id: Option<String>,
    package_kind: String,
    version: String,
    architecture: String,
    language: Option<String>,
    market: String,
    format: String,
    minimum_os_version: Option<String>,
    is_neutral: Option<i64>,
    content_id: Option<String>,
    file_size: Option<i64>,
    sha256: Option<String>,
    install_source: String,
}

struct PackageAssociationRow {
    package_family_name: String,
    product_id: Option<String>,
    content_id: Option<String>,
    identity_name: String,
    publisher: String,
    confidence: String,
    observed_at: i64,
}

fn read_package_association_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<PackageAssociationRow> {
    Ok(PackageAssociationRow {
        package_family_name: row.get(0)?,
        product_id: row.get(1)?,
        content_id: row.get(2)?,
        identity_name: row.get(3)?,
        publisher: row.get(4)?,
        confidence: row.get(5)?,
        observed_at: row.get(6)?,
    })
}

impl TryFrom<PackageAssociationRow> for PackageAssociation {
    type Error = PersistenceError;

    fn try_from(row: PackageAssociationRow) -> Result<Self, Self::Error> {
        Ok(Self {
            package_family_name: row.package_family_name,
            product_id: row.product_id,
            content_id: row.content_id,
            identity_name: row.identity_name,
            publisher: row.publisher,
            confidence: parse_enum::<AssociationConfidence>(
                &row.confidence,
                "association.confidence",
            )?,
            observed_at: row.observed_at,
        })
    }
}

fn read_package_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PackageRow> {
    Ok(PackageRow {
        update_id: row.get(0)?,
        product_id: row.get(1)?,
        package_family_name: row.get(2)?,
        package_moniker: row.get(3)?,
        identity_name: row.get(4)?,
        publisher: row.get(5)?,
        resource_id: row.get(6)?,
        package_kind: row.get(7)?,
        version: row.get(8)?,
        architecture: row.get(9)?,
        language: row.get(10)?,
        market: row.get(11)?,
        format: row.get(12)?,
        minimum_os_version: row.get(13)?,
        is_neutral: row.get(14)?,
        content_id: row.get(15)?,
        file_size: row.get(16)?,
        sha256: row.get(17)?,
        install_source: row.get(18)?,
    })
}

impl TryFrom<PackageRow> for PackageRecord {
    type Error = PersistenceError;

    fn try_from(row: PackageRow) -> Result<Self, Self::Error> {
        Ok(Self {
            update_id: row.update_id,
            product_id: row.product_id,
            package_family_name: row.package_family_name,
            package_moniker: row.package_moniker,
            identity_name: row.identity_name,
            publisher: row.publisher,
            resource_id: row.resource_id,
            package_kind: parse_enum::<PackageKind>(&row.package_kind, "package.package_kind")?,
            version: row
                .version
                .parse::<PackageVersion>()
                .map_err(|_| PersistenceError::InvalidStoredValue("package.version"))?,
            architecture: parse_enum::<Architecture>(&row.architecture, "package.architecture")?,
            language: row.language,
            market: row.market,
            format: parse_enum::<PackageFormat>(&row.format, "package.format")?,
            minimum_os_version: row
                .minimum_os_version
                .map(|value| {
                    value.parse::<PackageVersion>().map_err(|_| {
                        PersistenceError::InvalidStoredValue("package.minimum_os_version")
                    })
                })
                .transpose()?,
            is_neutral: row
                .is_neutral
                .map(|value| match value {
                    0 => Ok(false),
                    1 => Ok(true),
                    _ => Err(PersistenceError::InvalidStoredValue("package.is_neutral")),
                })
                .transpose()?,
            content_id: row.content_id,
            file_size: row
                .file_size
                .map(|value| from_i64(value, "package.file_size"))
                .transpose()?,
            sha256: row.sha256,
            install_source: parse_enum::<InstallSource>(
                &row.install_source,
                "package.install_source",
            )?,
        })
    }
}

struct JobRow {
    job_id: String,
    kind: String,
    product_id: String,
    requested_market: String,
    requested_architectures_json: String,
    requested_languages_json: String,
    deployment_scope: String,
    selected_update_id: Option<String>,
    package_family_name: Option<String>,
    stage: String,
    bytes_done: i64,
    bytes_total: Option<i64>,
    version: Option<String>,
    architecture: Option<String>,
    language: Option<String>,
    error_json: Option<String>,
    created_at: i64,
    updated_at: i64,
}

fn read_job_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRow> {
    Ok(JobRow {
        job_id: row.get(0)?,
        kind: row.get(1)?,
        product_id: row.get(2)?,
        requested_market: row.get(3)?,
        requested_architectures_json: row.get(4)?,
        requested_languages_json: row.get(5)?,
        deployment_scope: row.get(6)?,
        selected_update_id: row.get(7)?,
        package_family_name: row.get(8)?,
        stage: row.get(9)?,
        bytes_done: row.get(10)?,
        bytes_total: row.get(11)?,
        version: row.get(12)?,
        architecture: row.get(13)?,
        language: row.get(14)?,
        error_json: row.get(15)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}

impl TryFrom<JobRow> for Job {
    type Error = PersistenceError;

    fn try_from(row: JobRow) -> Result<Self, Self::Error> {
        Ok(Self {
            job_id: row.job_id,
            kind: parse_enum::<JobKind>(&row.kind, "job.kind")?,
            product_id: row.product_id,
            requested_market: row.requested_market,
            requested_architectures: serde_json::from_str(&row.requested_architectures_json)?,
            requested_languages: serde_json::from_str(&row.requested_languages_json)?,
            deployment_scope: parse_enum::<DeploymentScope>(
                &row.deployment_scope,
                "job.deployment_scope",
            )?,
            selected_update_id: row.selected_update_id,
            package_family_name: row.package_family_name,
            stage: parse_enum::<JobStage>(&row.stage, "job.stage")?,
            bytes_done: from_i64(row.bytes_done, "job.bytes_done")?,
            bytes_total: row
                .bytes_total
                .map(|value| from_i64(value, "job.bytes_total"))
                .transpose()?,
            version: row.version,
            architecture: row
                .architecture
                .map(|value| parse_enum::<Architecture>(&value, "job.architecture"))
                .transpose()?,
            language: row.language,
            error: row
                .error_json
                .map(|value| serde_json::from_str::<AppErrorDto>(&value))
                .transpose()?,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

fn enum_text<T: Serialize>(value: T) -> Result<String, PersistenceError> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(value) => Ok(value),
        _ => Err(PersistenceError::InvalidStoredValue("enum")),
    }
}

fn parse_enum<T: DeserializeOwned>(
    value: &str,
    field: &'static str,
) -> Result<T, PersistenceError> {
    serde_json::from_value(serde_json::Value::String(value.to_owned()))
        .map_err(|_| PersistenceError::InvalidStoredValue(field))
}

fn to_i64(value: u64, field: &'static str) -> Result<i64, PersistenceError> {
    i64::try_from(value).map_err(|_| PersistenceError::IntegerOutOfRange(field))
}

fn from_i64(value: i64, field: &'static str) -> Result<u64, PersistenceError> {
    u64::try_from(value).map_err(|_| PersistenceError::InvalidStoredValue(field))
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default()
}
