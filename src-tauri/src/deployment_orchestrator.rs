use crate::{
    applicability::SelectionMode,
    deployment::DeploymentScope,
    deployment_coordinator::{CoordinatorError, DeploymentCoordinator},
    deployment_plan::DeploymentPlan,
    domain::{InstallObservation, InstallSource, PackageVersion},
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    identity::{AssociationConfidence, PackageAssociation},
    inventory::{InventorySnapshot, PackageInventoryRecord},
    package::PackageIdentity,
    package_validation::{verify_package_request, ValidationError, VerifiedPackageSet},
    persistence::Persistence,
};

pub trait DeploymentBackend {
    fn scan(&mut self, scope: DeploymentScope) -> Result<InventorySnapshot, CoordinatorError>;
    fn install(
        &mut self,
        scope: DeploymentScope,
        package: &VerifiedPackageSet,
    ) -> Result<InventorySnapshot, CoordinatorError>;
}

pub struct SystemDeploymentBackend;

impl DeploymentBackend for SystemDeploymentBackend {
    fn scan(&mut self, scope: DeploymentScope) -> Result<InventorySnapshot, CoordinatorError> {
        DeploymentCoordinator::scan(scope)
    }

    fn install(
        &mut self,
        scope: DeploymentScope,
        package: &VerifiedPackageSet,
    ) -> Result<InventorySnapshot, CoordinatorError> {
        DeploymentCoordinator::install(scope, package)
    }
}

pub trait PackagePreflight {
    fn verify(&self, package: &VerifiedPackageSet) -> Result<(), ValidationError>;
}

pub struct SystemPackagePreflight;

impl PackagePreflight for SystemPackagePreflight {
    fn verify(&self, package: &VerifiedPackageSet) -> Result<(), ValidationError> {
        verify_package_request(&package.main)?;
        for dependency in &package.dependencies {
            verify_package_request(dependency)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentDisposition {
    Installed,
    Updated,
    AlreadyCurrent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationOutcome {
    pub disposition: DeploymentDisposition,
    pub inventory: InventorySnapshot,
    pub association: PackageAssociation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDeployment {
    scope: DeploymentScope,
    plan: DeploymentPlan,
    disposition: DeploymentDisposition,
    observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestrationPreparation {
    AlreadyCurrent(OrchestrationOutcome),
    Ready(PreparedDeployment),
}

pub struct DeploymentOrchestrator<B, V> {
    backend: B,
    preflight: V,
}

impl<B, V> DeploymentOrchestrator<B, V>
where
    B: DeploymentBackend,
    V: PackagePreflight,
{
    pub fn new(backend: B, preflight: V) -> Self {
        Self { backend, preflight }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn execute(
        &mut self,
        scope: DeploymentScope,
        mode: SelectionMode,
        plan: &DeploymentPlan,
        persistence: &Persistence,
        observed_at: i64,
    ) -> Result<OrchestrationOutcome, AppErrorDto> {
        match self.prepare(scope, mode, plan, persistence, observed_at)? {
            OrchestrationPreparation::AlreadyCurrent(outcome) => Ok(outcome),
            OrchestrationPreparation::Ready(prepared) => self.commit(prepared, persistence),
        }
    }

    pub fn prepare(
        &mut self,
        scope: DeploymentScope,
        mode: SelectionMode,
        plan: &DeploymentPlan,
        persistence: &Persistence,
        observed_at: i64,
    ) -> Result<OrchestrationPreparation, AppErrorDto> {
        self.preflight
            .verify(&plan.package_set)
            .map_err(|error| AppErrorDto::from(&error))?;

        let pre_scan = self
            .backend
            .scan(scope)
            .map_err(|error| AppErrorDto::from(&error))?;
        ensure_complete(&pre_scan)?;
        let main_identity = plan
            .package_set
            .main
            .expected_identity
            .as_ref()
            .ok_or_else(identity_error)?;
        let installed = matching_records(&pre_scan, main_identity)
            .into_iter()
            .filter(|record| scope_contains(record, scope))
            .collect::<Vec<_>>();

        if mode == SelectionMode::Update && installed.is_empty() {
            return Err(AppErrorDto::new(
                ErrorCode::PackageNotInstalled,
                RetryAdvice::ReconcileInventory,
            ));
        }
        if installed
            .iter()
            .any(|record| installed_version(record) > plan.main_version)
        {
            return Err(AppErrorDto::new(
                ErrorCode::VersionAheadOfCatalog,
                RetryAdvice::Never,
            ));
        }
        let current = installed.iter().copied().find(|record| {
            installed_version(record) == plan.main_version && scope_converged(record, scope)
        });
        if let Some(installed) = current.filter(|_| mode != SelectionMode::Repair) {
            let confidence = preserved_confidence(persistence, installed, plan)?;
            let association = association_from_record(installed, plan, confidence, observed_at);
            persistence
                .upsert_package_association(&association)
                .map_err(|_| storage_error())?;
            return Ok(OrchestrationPreparation::AlreadyCurrent(
                OrchestrationOutcome {
                    disposition: DeploymentDisposition::AlreadyCurrent,
                    inventory: pre_scan,
                    association,
                },
            ));
        }

        let disposition = if installed.is_empty() {
            DeploymentDisposition::Installed
        } else {
            DeploymentDisposition::Updated
        };
        Ok(OrchestrationPreparation::Ready(PreparedDeployment {
            scope,
            plan: plan.clone(),
            disposition,
            observed_at,
        }))
    }

    pub fn commit(
        &mut self,
        prepared: PreparedDeployment,
        persistence: &Persistence,
    ) -> Result<OrchestrationOutcome, AppErrorDto> {
        let PreparedDeployment {
            scope,
            plan,
            disposition,
            observed_at,
        } = prepared;
        let main_identity = plan
            .package_set
            .main
            .expected_identity
            .as_ref()
            .ok_or_else(identity_error)?;
        let post_scan = self
            .backend
            .install(scope, &plan.package_set)
            .map_err(|error| AppErrorDto::from(&error))?;
        ensure_complete(&post_scan)?;
        verify_postcondition(&post_scan, scope, &plan)?;
        let installed_main = matching_records(&post_scan, main_identity)
            .into_iter()
            .find(|record| {
                installed_version(record) == plan.main_version && scope_converged(record, scope)
            })
            .ok_or_else(identity_error)?;
        let association = association_from_record(
            installed_main,
            &plan,
            AssociationConfidence::VerifiedDeployment,
            observed_at,
        );
        let observation = InstallObservation {
            package_family_name: installed_main.package_family_name.clone(),
            product_id: Some(plan.product_id.clone()),
            source: InstallSource::ThisClient,
            observed_at,
        };
        persistence
            .record_deployment_success(&association, &observation)
            .map_err(|_| storage_error())?;

        Ok(OrchestrationOutcome {
            disposition,
            inventory: post_scan,
            association,
        })
    }
}

fn ensure_complete(snapshot: &InventorySnapshot) -> Result<(), AppErrorDto> {
    if snapshot.complete {
        Ok(())
    } else {
        Err(AppErrorDto::new(
            ErrorCode::DeploymentFailed,
            RetryAdvice::ReconcileInventory,
        ))
    }
}

fn verify_postcondition(
    snapshot: &InventorySnapshot,
    scope: DeploymentScope,
    plan: &DeploymentPlan,
) -> Result<(), AppErrorDto> {
    let requests = std::iter::once((&plan.package_set.main, true))
        .chain(
            plan.package_set
                .dependencies
                .iter()
                .map(|request| (request, false)),
        )
        .collect::<Vec<_>>();
    for (request, is_main) in requests {
        let identity = request
            .expected_identity
            .as_ref()
            .ok_or_else(identity_error)?;
        let expected = PackageVersion::new(
            identity.version[0],
            identity.version[1],
            identity.version[2],
            identity.version[3],
        );
        let converged = matching_records(snapshot, identity)
            .into_iter()
            .filter(|record| postcondition_scope_satisfied(record, scope, is_main))
            .any(|record| {
                let actual = installed_version(record);
                if is_main {
                    actual == expected
                } else {
                    actual >= expected
                }
            });
        if !converged {
            return Err(identity_error());
        }
    }
    Ok(())
}

fn matching_records<'a>(
    snapshot: &'a InventorySnapshot,
    identity: &PackageIdentity,
) -> Vec<&'a PackageInventoryRecord> {
    snapshot
        .records
        .iter()
        .filter(|record| {
            record.identity_name.eq_ignore_ascii_case(&identity.name)
                && record.publisher == identity.publisher
                && architecture_matches(&record.architecture, &identity.architecture)
                && record
                    .resource_id
                    .eq_ignore_ascii_case(&identity.resource_id)
        })
        .collect()
}

fn scope_contains(record: &PackageInventoryRecord, scope: DeploymentScope) -> bool {
    match scope {
        DeploymentScope::CurrentUser => record.installed_for_current_user,
        DeploymentScope::AllUsers => {
            record.installed_user_count > 0 || record.provisioned_for_future_users
        }
    }
}

fn scope_converged(record: &PackageInventoryRecord, scope: DeploymentScope) -> bool {
    match scope {
        DeploymentScope::CurrentUser => record.installed_for_current_user,
        DeploymentScope::AllUsers => record.provisioned_for_future_users,
    }
}

fn postcondition_scope_satisfied(
    record: &PackageInventoryRecord,
    scope: DeploymentScope,
    is_main: bool,
) -> bool {
    match scope {
        DeploymentScope::CurrentUser => record.installed_for_current_user,
        DeploymentScope::AllUsers if is_main => record.provisioned_for_future_users,
        DeploymentScope::AllUsers => {
            record.package_kind == crate::inventory::PackageKind::Framework
                || record.provisioned_for_future_users
        }
    }
}

fn architecture_matches(installed: &str, expected: &str) -> bool {
    expected.eq_ignore_ascii_case("neutral") || installed.eq_ignore_ascii_case(expected)
}

fn installed_version(record: &PackageInventoryRecord) -> PackageVersion {
    PackageVersion::new(
        record.version[0],
        record.version[1],
        record.version[2],
        record.version[3],
    )
}

fn association_from_record(
    record: &PackageInventoryRecord,
    plan: &DeploymentPlan,
    confidence: AssociationConfidence,
    observed_at: i64,
) -> PackageAssociation {
    PackageAssociation {
        package_family_name: record.package_family_name.clone(),
        product_id: Some(plan.product_id.clone()),
        content_id: plan.content_id.clone(),
        identity_name: record.identity_name.clone(),
        publisher: record.publisher.clone(),
        confidence,
        observed_at,
    }
}

fn preserved_confidence(
    persistence: &Persistence,
    record: &PackageInventoryRecord,
    plan: &DeploymentPlan,
) -> Result<AssociationConfidence, AppErrorDto> {
    let existing = persistence
        .package_association(&record.package_family_name)
        .map_err(|_| storage_error())?;
    let verified = existing.is_some_and(|association| {
        association.confidence == AssociationConfidence::VerifiedDeployment
            && association.product_id.as_deref() == Some(plan.product_id.as_str())
            && association
                .package_family_name
                .eq_ignore_ascii_case(&record.package_family_name)
            && association
                .identity_name
                .eq_ignore_ascii_case(&record.identity_name)
            && association.publisher == record.publisher
    });
    Ok(if verified {
        AssociationConfidence::VerifiedDeployment
    } else {
        AssociationConfidence::ExactIdentityPublisher
    })
}

fn identity_error() -> AppErrorDto {
    AppErrorDto::new(
        ErrorCode::SourceIdentityMismatch,
        RetryAdvice::ReconcileInventory,
    )
}

fn storage_error() -> AppErrorDto {
    AppErrorDto::new(ErrorCode::DeploymentFailed, RetryAdvice::ReconcileInventory)
}
