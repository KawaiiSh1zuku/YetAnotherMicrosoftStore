use serde::{Deserialize, Serialize};

use crate::{
    domain::{Architecture, PackageKind},
    error::AppErrorDto,
    jobs::{Job, JobSnapshot, JobStage, RecoveryAction},
    persistence::PersistenceError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobEventKind {
    Created,
    Imported,
    StageChanged,
    ProgressRecorded,
    SelectionRecorded,
    Failed,
    Completed,
    Cancelled,
    Recovered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobTargetRole {
    Main,
    Dependency,
    Resource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobTarget {
    pub role: JobTargetRole,
    pub update_id: String,
    pub identity_name: String,
    pub publisher: String,
    pub version: String,
    pub architecture: Architecture,
    pub resource_id: Option<String>,
    pub package_kind: PackageKind,
    pub expected_size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JobEvent {
    Created {
        job: Job,
    },
    Imported {
        job: Job,
    },
    StageChanged {
        stage: JobStage,
    },
    ProgressRecorded {
        bytes_done: u64,
        bytes_total: Option<u64>,
    },
    SelectionRecorded {
        selected_update_id: String,
        package_family_name: String,
        version: String,
        architecture: Architecture,
        language: Option<String>,
        requires_elevation: bool,
        targets: Vec<JobTarget>,
    },
    Failed {
        error: AppErrorDto,
    },
    Completed,
    Cancelled,
    Recovered {
        stage: JobStage,
    },
}

impl JobEvent {
    pub fn kind(&self) -> JobEventKind {
        match self {
            Self::Created { .. } => JobEventKind::Created,
            Self::Imported { .. } => JobEventKind::Imported,
            Self::StageChanged { .. } => JobEventKind::StageChanged,
            Self::ProgressRecorded { .. } => JobEventKind::ProgressRecorded,
            Self::SelectionRecorded { .. } => JobEventKind::SelectionRecorded,
            Self::Failed { .. } => JobEventKind::Failed,
            Self::Completed => JobEventKind::Completed,
            Self::Cancelled => JobEventKind::Cancelled,
            Self::Recovered { .. } => JobEventKind::Recovered,
        }
    }

    pub(crate) fn fold(
        &self,
        previous: Option<&Job>,
        occurred_at: i64,
        allow_import: bool,
    ) -> Result<Job, PersistenceError> {
        match (self, previous) {
            (Self::Created { job }, None)
                if job.stage == JobStage::Queued
                    && job.created_at == occurred_at
                    && job.updated_at == occurred_at
                    && job.selected_update_id.is_none()
                    && job.package_family_name.is_none()
                    && job.bytes_done == 0
                    && job.bytes_total.is_none()
                    && job.version.is_none()
                    && job.architecture.is_none()
                    && job.language.is_none()
                    && !job.requires_elevation
                    && job.error.is_none() =>
            {
                validate_job(job)?;
                Ok(job.clone())
            }
            (Self::Imported { job }, None) if allow_import => {
                validate_job(job)?;
                Ok(job.clone())
            }
            (Self::Created { .. } | Self::Imported { .. }, _) => {
                Err(PersistenceError::EventHistoryInvalid)
            }
            (_, Some(old)) => {
                if occurred_at < old.updated_at {
                    return Err(PersistenceError::EventHistoryInvalid);
                }
                let mut next = old.clone();
                match self {
                    Self::StageChanged { stage } => {
                        if matches!(
                            stage,
                            JobStage::Completed
                                | JobStage::Failed
                                | JobStage::Cancelled
                                | JobStage::Interrupted
                                | JobStage::NeedsReconciliation
                        ) || !old.stage.can_transition_to(*stage)
                        {
                            return Err(PersistenceError::EventHistoryInvalid);
                        }
                        next.stage = *stage;
                        if *stage == JobStage::Resolving {
                            next.selected_update_id = None;
                            next.package_family_name = None;
                            next.version = None;
                            next.architecture = None;
                            next.language = None;
                            next.requires_elevation = false;
                            next.error = None;
                        }
                    }
                    Self::ProgressRecorded {
                        bytes_done,
                        bytes_total,
                    } => {
                        if old.stage != JobStage::Downloading
                            || *bytes_done < old.bytes_done
                            || bytes_total.is_some_and(|total| total < *bytes_done)
                            || (old.bytes_total.is_some() && bytes_total.is_none())
                            || matches!((old.bytes_total, bytes_total), (Some(before), Some(after)) if *after < before)
                        {
                            return Err(PersistenceError::EventHistoryInvalid);
                        }
                        next.bytes_done = *bytes_done;
                        next.bytes_total = *bytes_total;
                    }
                    Self::SelectionRecorded {
                        selected_update_id,
                        package_family_name,
                        version,
                        architecture,
                        language,
                        requires_elevation,
                        targets,
                    } => {
                        if old.stage != JobStage::Selecting || old.selected_update_id.is_some() {
                            return Err(PersistenceError::EventHistoryInvalid);
                        }
                        if !safe_identifier(selected_update_id)
                            || !safe_identifier(package_family_name)
                            || !safe_identifier(version)
                            || language
                                .as_deref()
                                .is_some_and(|value| !safe_language(value))
                            || targets.is_empty()
                            || targets.iter().any(|target| !target.is_safe())
                            || targets
                                .iter()
                                .filter(|target| target.role == JobTargetRole::Main)
                                .count()
                                != 1
                            || !targets.iter().any(|target| {
                                target.role == JobTargetRole::Main
                                    && target.update_id == *selected_update_id
                                    && target.version == *version
                                    && target.architecture == *architecture
                                    && target.package_kind == PackageKind::Main
                            })
                        {
                            return Err(PersistenceError::UnsafeJobEvent);
                        }
                        next.selected_update_id = Some(selected_update_id.clone());
                        next.package_family_name = Some(package_family_name.clone());
                        next.version = Some(version.clone());
                        next.architecture = Some(*architecture);
                        next.language = language.clone();
                        next.requires_elevation = *requires_elevation;
                    }
                    Self::Failed { error } => {
                        if !old.stage.can_transition_to(JobStage::Failed)
                            || !safe_error(error, &old.job_id)
                        {
                            return Err(PersistenceError::EventHistoryInvalid);
                        }
                        next.stage = JobStage::Failed;
                        next.error = Some(error.clone());
                    }
                    Self::Completed => {
                        if !old.stage.can_transition_to(JobStage::Completed) {
                            return Err(PersistenceError::EventHistoryInvalid);
                        }
                        next.stage = JobStage::Completed;
                    }
                    Self::Cancelled => {
                        if !old.stage.can_transition_to(JobStage::Cancelled) {
                            return Err(PersistenceError::EventHistoryInvalid);
                        }
                        next.stage = JobStage::Cancelled;
                    }
                    Self::Recovered { stage } => {
                        if matches!(
                            old.stage,
                            JobStage::Interrupted | JobStage::NeedsReconciliation
                        ) {
                            return Err(PersistenceError::EventHistoryInvalid);
                        }
                        let action = next.recover_after_restart(occurred_at);
                        let expected = match action {
                            RecoveryAction::ReResolve => JobStage::Interrupted,
                            RecoveryAction::ReconcileInventory => JobStage::NeedsReconciliation,
                            RecoveryAction::None => {
                                return Err(PersistenceError::EventHistoryInvalid)
                            }
                        };
                        if *stage != expected {
                            return Err(PersistenceError::EventHistoryInvalid);
                        }
                    }
                    Self::Created { .. } | Self::Imported { .. } => unreachable!(),
                }
                next.updated_at = occurred_at;
                validate_job(&next)?;
                Ok(next)
            }
            _ => Err(PersistenceError::EventHistoryInvalid),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredJobEvent {
    pub cursor: u64,
    pub job_id: String,
    pub sequence: u64,
    pub event: JobEvent,
    pub snapshot: JobSnapshot,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobControl {
    Pause,
    Resume,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandRejectReason {
    InvalidStage,
    StaleSequence,
    LeaseLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CommandOutcome {
    Applied,
    AlreadySatisfied,
    Rejected { reason: CommandRejectReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobCommand {
    pub command_id: String,
    pub job_id: String,
    pub control: JobControl,
    pub expected_sequence: u64,
    pub created_at: i64,
    pub processed_at: Option<i64>,
    pub outcome: Option<CommandOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerLease {
    pub owner_id: String,
    pub generation: u64,
    pub expires_at: i64,
}

pub(crate) fn safe_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:{}".contains(&byte))
}

fn safe_language(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn safe_error(error: &AppErrorDto, job_id: &str) -> bool {
    error.message_key == error.code.message_key()
        && error.job_id.as_deref().is_none_or(|id| id == job_id)
}

fn validate_job(job: &Job) -> Result<(), PersistenceError> {
    if safe_identifier(&job.job_id)
        && safe_identifier(&job.product_id)
        && safe_identifier(&job.requested_market)
        && job.requested_architectures.len() <= 16
        && job.requested_languages.len() <= 32
        && job
            .requested_languages
            .iter()
            .all(|value| safe_language(value))
        && job
            .selected_update_id
            .as_deref()
            .is_none_or(safe_identifier)
        && job
            .package_family_name
            .as_deref()
            .is_none_or(safe_identifier)
        && job.version.as_deref().is_none_or(safe_identifier)
        && job.language.as_deref().is_none_or(safe_language)
        && job
            .error
            .as_ref()
            .is_none_or(|error| safe_error(error, &job.job_id))
    {
        Ok(())
    } else {
        Err(PersistenceError::UnsafeJobEvent)
    }
}

impl JobTarget {
    pub(crate) fn is_safe(&self) -> bool {
        safe_identifier(&self.update_id)
            && safe_identifier(&self.identity_name)
            && safe_identifier(&self.version)
            && self.resource_id.as_deref().is_none_or(safe_identifier)
            && !self.publisher.is_empty()
            && self.publisher.len() <= 512
            && !self.publisher.contains('/')
            && !self.publisher.contains('\\')
            && !self.publisher.chars().any(char::is_control)
            && self.expected_size > 0
            && self.sha256.len() == 64
            && self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }
}

pub(crate) fn validate_command(command: &JobCommand) -> Result<(), PersistenceError> {
    if safe_identifier(&command.command_id)
        && safe_identifier(&command.job_id)
        && command.processed_at.is_none()
        && command.outcome.is_none()
    {
        Ok(())
    } else {
        Err(PersistenceError::CommandConflict)
    }
}
