use std::{fs, path::PathBuf};

use yet_another_microsoft_store_lib::{
    applicability::SelectionMode,
    deployment::DeploymentScope,
    deployment_coordinator::CoordinatorError,
    deployment_orchestrator::{
        DeploymentBackend, DeploymentDisposition, DeploymentOrchestrator, PackagePreflight,
    },
    deployment_plan::DeploymentPlan,
    domain::{InstallSource, PackageVersion},
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    inventory::{InventorySnapshot, InventorySource, PackageInventoryRecord, PackageKind},
    package::{PackageFileRequest, PackageIdentity},
    package_validation::{ValidationError, VerifiedPackageSet},
    persistence::Persistence,
};

struct TestDatabase {
    path: PathBuf,
}

impl TestDatabase {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!(
                "yamstore-m5-orchestration-{}.sqlite3",
                uuid::Uuid::new_v4()
            )),
        }
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.path.with_extension("sqlite3-shm"));
        let _ = fs::remove_file(self.path.with_extension("sqlite3-wal"));
    }
}

#[derive(Default)]
struct FakeBackend {
    pre_scan: Option<Result<InventorySnapshot, CoordinatorError>>,
    post_install: Option<Result<InventorySnapshot, CoordinatorError>>,
    scan_scopes: Vec<DeploymentScope>,
    install_scopes: Vec<DeploymentScope>,
}

impl DeploymentBackend for FakeBackend {
    fn scan(&mut self, scope: DeploymentScope) -> Result<InventorySnapshot, CoordinatorError> {
        self.scan_scopes.push(scope);
        self.pre_scan
            .take()
            .expect("test must provide a pre-scan result")
    }

    fn install(
        &mut self,
        scope: DeploymentScope,
        _package: &VerifiedPackageSet,
    ) -> Result<InventorySnapshot, CoordinatorError> {
        self.install_scopes.push(scope);
        self.post_install
            .take()
            .expect("test must provide an install result")
    }
}

struct FakePreflight {
    result: Result<(), ValidationError>,
}

impl PackagePreflight for FakePreflight {
    fn verify(&self, _package: &VerifiedPackageSet) -> Result<(), ValidationError> {
        self.result.clone()
    }
}

fn identity(version: [u16; 4]) -> PackageIdentity {
    PackageIdentity {
        name: "Example.App".to_owned(),
        publisher: "CN=Example".to_owned(),
        version,
        architecture: "x64".to_owned(),
        resource_id: String::new(),
    }
}

fn plan(version: [u16; 4]) -> DeploymentPlan {
    DeploymentPlan {
        product_id: "product".to_owned(),
        main_update_id: "main".to_owned(),
        main_version: PackageVersion::new(version[0], version[1], version[2], version[3]),
        content_id: Some("content-main".to_owned()),
        package_set: VerifiedPackageSet {
            main: PackageFileRequest {
                path: PathBuf::from(r"C:\cache\main.msix"),
                sha256_hex: "ab".repeat(32),
                expected_identity: Some(identity(version)),
            },
            dependencies: Vec::new(),
        },
    }
}

fn plan_with_framework(version: [u16; 4]) -> DeploymentPlan {
    let mut plan = plan(version);
    plan.package_set.dependencies.push(PackageFileRequest {
        path: PathBuf::from(r"C:\cache\framework.msix"),
        sha256_hex: "cd".repeat(32),
        expected_identity: Some(PackageIdentity {
            name: "Example.Framework".to_owned(),
            publisher: "CN=Example".to_owned(),
            version: [2, 0, 0, 0],
            architecture: "x64".to_owned(),
            resource_id: String::new(),
        }),
    });
    plan
}

fn record(version: [u16; 4], signature_kind: &str) -> PackageInventoryRecord {
    record_with_provisioning(version, signature_kind, false)
}

fn record_with_provisioning(
    version: [u16; 4],
    signature_kind: &str,
    provisioned_for_future_users: bool,
) -> PackageInventoryRecord {
    PackageInventoryRecord {
        app_name: "Example App".to_owned(),
        package_name: "Example.App".to_owned(),
        identity_name: "Example.App".to_owned(),
        publisher: "CN=Example".to_owned(),
        package_family_name: "Example.App_abc".to_owned(),
        package_full_name: format!(
            "Example.App_{}.{}.{}.{}_x64__abc",
            version[0], version[1], version[2], version[3]
        ),
        version,
        architecture: "x64".to_owned(),
        resource_id: String::new(),
        package_kind: PackageKind::Main,
        signature_kind: signature_kind.to_owned(),
        status: "Ok".to_owned(),
        installed_for_current_user: true,
        installed_user_count: 1,
        has_other_users: false,
        provisioned_for_future_users,
    }
}

fn staged_framework_record() -> PackageInventoryRecord {
    let mut record = record([2, 0, 0, 0], "Store");
    record.identity_name = "Example.Framework".to_owned();
    record.package_family_name = "Example.Framework_abc".to_owned();
    record.package_full_name = "Example.Framework_2.0.0.0_x64__abc".to_owned();
    record.package_kind = PackageKind::Framework;
    record.installed_for_current_user = false;
    record.installed_user_count = 0;
    record.provisioned_for_future_users = false;
    record
}

fn snapshot(source: InventorySource, records: Vec<PackageInventoryRecord>) -> InventorySnapshot {
    InventorySnapshot {
        source,
        captured_at: "100".to_owned(),
        os_build: "10.0.19045".to_owned(),
        complete: true,
        records,
        warnings: Vec::new(),
    }
}

fn store(database: &TestDatabase) -> Persistence {
    Persistence::open(&database.path).expect("open test database")
}

#[test]
fn store_installed_package_updates_through_current_user_and_records_verified_source() {
    let database = TestDatabase::new();
    let persistence = store(&database);
    let backend = FakeBackend {
        pre_scan: Some(Ok(snapshot(
            InventorySource::CurrentUser,
            vec![record([2, 0, 0, 0], "Store")],
        ))),
        post_install: Some(Ok(snapshot(
            InventorySource::CurrentUser,
            vec![record([2, 0, 0, 0], "Store"), record([3, 0, 0, 0], "Store")],
        ))),
        ..FakeBackend::default()
    };
    let mut orchestrator = DeploymentOrchestrator::new(backend, FakePreflight { result: Ok(()) });

    let outcome = orchestrator
        .execute(
            DeploymentScope::CurrentUser,
            SelectionMode::Update,
            &plan([3, 0, 0, 0]),
            &persistence,
            123,
        )
        .expect("Store-installed package should be a source-independent update candidate");

    assert_eq!(outcome.disposition, DeploymentDisposition::Updated);
    assert_eq!(
        orchestrator.backend().scan_scopes,
        vec![DeploymentScope::CurrentUser]
    );
    assert_eq!(
        orchestrator.backend().install_scopes,
        vec![DeploymentScope::CurrentUser]
    );
    let observation = persistence
        .install_observation("Example.App_abc")
        .expect("read observation")
        .expect("successful deployment records source");
    assert_eq!(observation.source, InstallSource::ThisClient);
    assert_eq!(observation.product_id.as_deref(), Some("product"));
    assert_eq!(
        persistence
            .package_association("Example.App_abc")
            .expect("read association"),
        Some(outcome.association)
    );

    let backend = FakeBackend {
        pre_scan: Some(Ok(snapshot(
            InventorySource::CurrentUser,
            vec![record([3, 0, 0, 0], "Store")],
        ))),
        ..FakeBackend::default()
    };
    let mut orchestrator = DeploymentOrchestrator::new(backend, FakePreflight { result: Ok(()) });
    orchestrator
        .execute(
            DeploymentScope::CurrentUser,
            SelectionMode::Update,
            &plan([3, 0, 0, 0]),
            &persistence,
            124,
        )
        .expect("no-op update should preserve verified deployment association");
    assert_eq!(
        persistence
            .package_association("Example.App_abc")
            .expect("read preserved association")
            .expect("association remains present")
            .confidence,
        yet_another_microsoft_store_lib::identity::AssociationConfidence::VerifiedDeployment
    );
}

#[test]
fn all_users_scope_is_forwarded_without_current_user_fallback() {
    let database = TestDatabase::new();
    let persistence = store(&database);
    let backend = FakeBackend {
        pre_scan: Some(Ok(snapshot(InventorySource::AllUsersElevated, Vec::new()))),
        post_install: Some(Ok(snapshot(
            InventorySource::AllUsersElevated,
            vec![record_with_provisioning([1, 0, 0, 0], "Developer", true)],
        ))),
        ..FakeBackend::default()
    };
    let mut orchestrator = DeploymentOrchestrator::new(backend, FakePreflight { result: Ok(()) });

    let outcome = orchestrator
        .execute(
            DeploymentScope::AllUsers,
            SelectionMode::Install,
            &plan([1, 0, 0, 0]),
            &persistence,
            123,
        )
        .expect("all-users orchestration should converge through its backend");

    assert_eq!(outcome.disposition, DeploymentDisposition::Installed);
    assert_eq!(
        orchestrator.backend().scan_scopes,
        vec![DeploymentScope::AllUsers]
    );
    assert_eq!(
        orchestrator.backend().install_scopes,
        vec![DeploymentScope::AllUsers]
    );
}

#[test]
fn all_users_equal_version_is_not_current_until_it_is_provisioned() {
    let database = TestDatabase::new();
    let persistence = store(&database);
    let backend = FakeBackend {
        pre_scan: Some(Ok(snapshot(
            InventorySource::AllUsersElevated,
            vec![record([1, 0, 0, 0], "Store")],
        ))),
        post_install: Some(Ok(snapshot(
            InventorySource::AllUsersElevated,
            vec![record_with_provisioning([1, 0, 0, 0], "Store", true)],
        ))),
        ..FakeBackend::default()
    };
    let mut orchestrator = DeploymentOrchestrator::new(backend, FakePreflight { result: Ok(()) });

    let outcome = orchestrator
        .execute(
            DeploymentScope::AllUsers,
            SelectionMode::Install,
            &plan([1, 0, 0, 0]),
            &persistence,
            123,
        )
        .expect("all-users deployment should provision an existing user-scoped package");

    assert_eq!(outcome.disposition, DeploymentDisposition::Updated);
    assert_eq!(
        orchestrator.backend().install_scopes,
        vec![DeploymentScope::AllUsers]
    );
}

#[test]
fn all_users_postcondition_rejects_an_unprovisioned_package() {
    let database = TestDatabase::new();
    let persistence = store(&database);
    let backend = FakeBackend {
        pre_scan: Some(Ok(snapshot(InventorySource::AllUsersElevated, Vec::new()))),
        post_install: Some(Ok(snapshot(
            InventorySource::AllUsersElevated,
            vec![record([1, 0, 0, 0], "Store")],
        ))),
        ..FakeBackend::default()
    };
    let mut orchestrator = DeploymentOrchestrator::new(backend, FakePreflight { result: Ok(()) });

    let error = orchestrator
        .execute(
            DeploymentScope::AllUsers,
            SelectionMode::Install,
            &plan([1, 0, 0, 0]),
            &persistence,
            123,
        )
        .expect_err("unprovisioned all-users result must not converge");

    assert_eq!(error.code, ErrorCode::SourceIdentityMismatch);
}

#[test]
fn all_users_accepts_a_staged_framework_behind_a_provisioned_main() {
    let database = TestDatabase::new();
    let persistence = store(&database);
    let backend = FakeBackend {
        pre_scan: Some(Ok(snapshot(InventorySource::AllUsersElevated, Vec::new()))),
        post_install: Some(Ok(snapshot(
            InventorySource::AllUsersElevated,
            vec![
                record_with_provisioning([1, 0, 0, 0], "Store", true),
                staged_framework_record(),
            ],
        ))),
        ..FakeBackend::default()
    };
    let mut orchestrator = DeploymentOrchestrator::new(backend, FakePreflight { result: Ok(()) });

    let outcome = orchestrator
        .execute(
            DeploymentScope::AllUsers,
            SelectionMode::Install,
            &plan_with_framework([1, 0, 0, 0]),
            &persistence,
            123,
        )
        .expect("provisioned main should retain its staged framework dependency");

    assert_eq!(outcome.disposition, DeploymentDisposition::Installed);
}

#[test]
fn version_ahead_detection_is_independent_of_inventory_order() {
    for records in [
        vec![record([2, 0, 0, 0], "Store"), record([4, 0, 0, 0], "Store")],
        vec![record([4, 0, 0, 0], "Store"), record([2, 0, 0, 0], "Store")],
    ] {
        let database = TestDatabase::new();
        let persistence = store(&database);
        let backend = FakeBackend {
            pre_scan: Some(Ok(snapshot(InventorySource::CurrentUser, records))),
            ..FakeBackend::default()
        };
        let mut orchestrator =
            DeploymentOrchestrator::new(backend, FakePreflight { result: Ok(()) });

        let error = orchestrator
            .execute(
                DeploymentScope::CurrentUser,
                SelectionMode::Update,
                &plan([3, 0, 0, 0]),
                &persistence,
                123,
            )
            .expect_err("any installed version ahead of catalog must block downgrade");

        assert_eq!(error.code, ErrorCode::VersionAheadOfCatalog);
    }
}

#[test]
fn equal_version_is_already_current_and_does_not_claim_this_client_source() {
    let database = TestDatabase::new();
    let persistence = store(&database);
    let backend = FakeBackend {
        pre_scan: Some(Ok(snapshot(
            InventorySource::CurrentUser,
            vec![record([3, 0, 0, 0], "Store")],
        ))),
        post_install: None,
        ..FakeBackend::default()
    };
    let mut orchestrator = DeploymentOrchestrator::new(backend, FakePreflight { result: Ok(()) });

    let outcome = orchestrator
        .execute(
            DeploymentScope::CurrentUser,
            SelectionMode::Update,
            &plan([3, 0, 0, 0]),
            &persistence,
            123,
        )
        .expect("equal version should converge without deployment");

    assert_eq!(outcome.disposition, DeploymentDisposition::AlreadyCurrent);
    assert!(orchestrator.backend().install_scopes.is_empty());
    assert_eq!(
        persistence
            .install_observation("Example.App_abc")
            .expect("read observation"),
        None
    );
}

#[test]
fn downgrade_signature_failure_and_postcondition_mismatch_never_record_source() {
    let cases = [
        (
            FakeBackend {
                pre_scan: Some(Ok(snapshot(
                    InventorySource::CurrentUser,
                    vec![record([4, 0, 0, 0], "Store")],
                ))),
                ..FakeBackend::default()
            },
            Ok(()),
            ErrorCode::VersionAheadOfCatalog,
        ),
        (
            FakeBackend {
                pre_scan: Some(Ok(snapshot(InventorySource::CurrentUser, Vec::new()))),
                ..FakeBackend::default()
            },
            Err(ValidationError::SignatureInvalid),
            ErrorCode::SignatureInvalid,
        ),
        (
            FakeBackend {
                pre_scan: Some(Ok(snapshot(
                    InventorySource::CurrentUser,
                    vec![record([2, 0, 0, 0], "Store")],
                ))),
                post_install: Some(Ok(snapshot(
                    InventorySource::CurrentUser,
                    vec![record([2, 0, 0, 0], "Store")],
                ))),
                ..FakeBackend::default()
            },
            Ok(()),
            ErrorCode::SourceIdentityMismatch,
        ),
    ];

    for (backend, preflight, expected_code) in cases {
        let database = TestDatabase::new();
        let persistence = store(&database);
        let mut orchestrator =
            DeploymentOrchestrator::new(backend, FakePreflight { result: preflight });
        let error = orchestrator
            .execute(
                DeploymentScope::CurrentUser,
                SelectionMode::Update,
                &plan([3, 0, 0, 0]),
                &persistence,
                123,
            )
            .expect_err("unsafe deployment state must fail closed");

        assert_eq!(error.code, expected_code);
        assert_eq!(
            persistence
                .install_observation("Example.App_abc")
                .expect("read observation"),
            None
        );
    }
}

#[test]
fn m0_internal_errors_map_to_closed_frontend_errors_without_message_leakage() {
    let postcondition = AppErrorDto::from(&CoordinatorError {
        code: "postcondition_missing".to_owned(),
        message: "raw HRESULT 0x80000000".to_owned(),
    });
    assert_eq!(postcondition.code, ErrorCode::DeploymentFailed);
    assert_eq!(postcondition.retry, RetryAdvice::ReconcileInventory);
    assert!(!serde_json::to_string(&postcondition)
        .expect("serialize error")
        .contains("80000000"));
}
