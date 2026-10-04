use serde::{Deserialize, Serialize};

use crate::{
    deployment::DeploymentScope, domain::Architecture, error::AppErrorDto,
    package_process::ProcessDescriptor,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Install,
    Update,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStage {
    Queued,
    Resolving,
    Selecting,
    Downloading,
    Paused,
    Verifying,
    Preparing,
    Deploying,
    AwaitingProcessExit,
    Interrupted,
    NeedsReconciliation,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    None,
    ReResolve,
    ReconcileInventory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobTransitionError {
    pub from: JobStage,
    pub to: JobStage,
}

impl std::fmt::Display for JobTransitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid job transition from {:?} to {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for JobTransitionError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub job_id: String,
    pub kind: JobKind,
    pub product_id: String,
    pub requested_market: String,
    pub requested_architectures: Vec<Architecture>,
    pub requested_languages: Vec<String>,
    pub deployment_scope: DeploymentScope,
    pub selected_update_id: Option<String>,
    pub package_family_name: Option<String>,
    pub stage: JobStage,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    #[serde(default)]
    pub deployment_progress: Option<u8>,
    pub version: Option<String>,
    pub architecture: Option<Architecture>,
    pub language: Option<String>,
    pub error: Option<AppErrorDto>,
    #[serde(default)]
    pub blocked_processes: Vec<ProcessDescriptor>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Job {
    pub fn transition_to(
        &mut self,
        next: JobStage,
        updated_at: i64,
    ) -> Result<(), JobTransitionError> {
        if !self.stage.can_transition_to(next) {
            return Err(JobTransitionError {
                from: self.stage,
                to: next,
            });
        }
        self.stage = next;
        self.updated_at = updated_at;
        Ok(())
    }

    pub fn recover_after_restart(&mut self, updated_at: i64) -> RecoveryAction {
        if self.stage == JobStage::Interrupted {
            return RecoveryAction::ReResolve;
        }
        if self.stage == JobStage::NeedsReconciliation {
            return RecoveryAction::ReconcileInventory;
        }
        let (stage, action) = match self.stage {
            JobStage::Resolving
            | JobStage::Selecting
            | JobStage::Downloading
            | JobStage::Verifying => (JobStage::Interrupted, RecoveryAction::ReResolve),
            JobStage::Preparing | JobStage::Deploying => (
                JobStage::NeedsReconciliation,
                RecoveryAction::ReconcileInventory,
            ),
            _ => return RecoveryAction::None,
        };
        self.stage = stage;
        self.updated_at = updated_at;
        action
    }
}

impl JobStage {
    pub(crate) const fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::Queued => matches!(next, Self::Resolving | Self::Cancelled),
            Self::Resolving => matches!(
                next,
                Self::Selecting | Self::Failed | Self::Cancelled | Self::Interrupted
            ),
            Self::Selecting => matches!(
                next,
                Self::Downloading | Self::Failed | Self::Cancelled | Self::Interrupted
            ),
            Self::Downloading => matches!(
                next,
                Self::Paused | Self::Verifying | Self::Failed | Self::Cancelled | Self::Interrupted
            ),
            Self::Paused | Self::Interrupted => {
                matches!(next, Self::Resolving | Self::Cancelled)
            }
            Self::Verifying => matches!(
                next,
                Self::Preparing
                    | Self::Completed
                    | Self::Failed
                    | Self::Cancelled
                    | Self::Interrupted
            ),
            Self::Preparing => matches!(
                next,
                Self::Deploying
                    | Self::AwaitingProcessExit
                    | Self::Completed
                    | Self::Failed
                    | Self::Cancelled
                    | Self::NeedsReconciliation
            ),
            Self::Deploying => matches!(
                next,
                Self::AwaitingProcessExit
                    | Self::Completed
                    | Self::Failed
                    | Self::NeedsReconciliation
            ),
            Self::AwaitingProcessExit => matches!(next, Self::Preparing | Self::Cancelled),
            Self::NeedsReconciliation => {
                matches!(next, Self::Completed | Self::Failed | Self::Resolving)
            }
            Self::Failed => matches!(next, Self::Resolving | Self::Cancelled),
            Self::Completed | Self::Cancelled => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub job: Job,
    pub sequence: u64,
}
