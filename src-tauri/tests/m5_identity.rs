use std::{
    fs,
    path::{Path, PathBuf},
};

use yet_another_microsoft_store_lib::{
    domain::{Architecture, PackageFormat, PackageKind, PackageVersion},
    identity::{
        compare_catalog_version, correlate_package, AssociationConfidence, CatalogIdentity,
        CatalogVersionState, PackageAssociation,
    },
    inventory::{PackageInventoryRecord, PackageKind as InventoryPackageKind},
    persistence::Persistence,
    resolver::ResolvedPackage,
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

fn inventory(version: [u16; 4]) -> PackageInventoryRecord {
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
        package_kind: InventoryPackageKind::Main,
        signature_kind: "Store".to_owned(),
        status: "Ok".to_owned(),
        installed_for_current_user: true,
        installed_user_count: 1,
        has_other_users: false,
        provisioned_for_future_users: false,
    }
}

fn catalog_identity(product_id: &str, pfn: Option<&str>) -> CatalogIdentity {
    CatalogIdentity {
        product_id: product_id.to_owned(),
        package_family_name: pfn.map(str::to_owned),
        content_id: Some(format!("content-{product_id}")),
        identity_name: "Example.App".to_owned(),
        publisher: "CN=Example".to_owned(),
    }
}

fn main_package(version: PackageVersion) -> ResolvedPackage {
    ResolvedPackage {
        package_moniker: format!("Example.App_{version}_x64__abc"),
        package_type: "appx".to_owned(),
        package_uri: None,
        file_name: Some("example.msix".to_owned()),
        file_size: Some(4096),
        sha256: Some("ab".repeat(32)),
        update_id: "update-main".to_owned(),
        identity_name: Some("Example.App".to_owned()),
        publisher: Some("CN=Example".to_owned()),
        version,
        architecture: Architecture::X64,
        resource_id: None,
        package_kind: PackageKind::Main,
        minimum_os_version: None,
        language: None,
        is_neutral: Some(true),
        content_id: Some("content-product".to_owned()),
        format: PackageFormat::Msix,
        prerequisites: Vec::new(),
        bundled_updates: Vec::new(),
    }
}

#[test]
fn package_family_name_match_has_priority_over_identity_fallback() {
    let candidates = vec![
        catalog_identity("exact", Some("Example.App_abc")),
        catalog_identity("fallback", None),
    ];

    let association = correlate_package(&inventory([2, 0, 0, 0]), &candidates, None, 100);

    assert_eq!(association.product_id.as_deref(), Some("exact"));
    assert_eq!(
        association.confidence,
        AssociationConfidence::ExactPackageFamilyName
    );
    assert_eq!(association.content_id.as_deref(), Some("content-exact"));
}

#[test]
fn unique_identity_and_publisher_match_is_linked_but_ambiguity_is_explicit() {
    let record = inventory([2, 0, 0, 0]);
    let unique = correlate_package(&record, &[catalog_identity("unique", None)], None, 100);
    assert_eq!(unique.product_id.as_deref(), Some("unique"));
    assert_eq!(
        unique.confidence,
        AssociationConfidence::ExactIdentityPublisher
    );

    let ambiguous = correlate_package(
        &record,
        &[
            catalog_identity("first", None),
            catalog_identity("second", None),
        ],
        None,
        101,
    );
    assert_eq!(ambiguous.product_id, None);
    assert_eq!(ambiguous.content_id, None);
    assert_eq!(ambiguous.confidence, AssociationConfidence::Ambiguous);

    let unresolved = correlate_package(&record, &[], None, 102);
    assert_eq!(unresolved.product_id, None);
    assert_eq!(unresolved.confidence, AssociationConfidence::Unresolved);
}

#[test]
fn verified_deployment_cache_is_used_only_for_the_same_windows_identity() {
    let cached = PackageAssociation {
        package_family_name: "Example.App_abc".to_owned(),
        product_id: Some("cached".to_owned()),
        content_id: Some("content-cached".to_owned()),
        identity_name: "Example.App".to_owned(),
        publisher: "CN=Example".to_owned(),
        confidence: AssociationConfidence::VerifiedDeployment,
        observed_at: 90,
    };

    let reused = correlate_package(&inventory([2, 0, 0, 0]), &[], Some(&cached), 100);
    assert_eq!(reused.product_id.as_deref(), Some("cached"));
    assert_eq!(reused.confidence, AssociationConfidence::VerifiedDeployment);

    let mut other_identity = inventory([2, 0, 0, 0]);
    other_identity.publisher = "CN=Other".to_owned();
    let rejected = correlate_package(&other_identity, &[], Some(&cached), 100);
    assert_eq!(rejected.product_id, None);
    assert_eq!(rejected.confidence, AssociationConfidence::Unresolved);
}

#[test]
fn fresh_catalog_comparison_distinguishes_update_current_ahead_and_identity_mismatch() {
    let linked = correlate_package(
        &inventory([2, 0, 0, 0]),
        &[catalog_identity("product", Some("Example.App_abc"))],
        None,
        100,
    );

    assert_eq!(
        compare_catalog_version(
            &inventory([2, 0, 0, 0]),
            &main_package(PackageVersion::new(3, 0, 0, 0)),
            &linked,
        ),
        CatalogVersionState::UpdateAvailable
    );
    assert_eq!(
        compare_catalog_version(
            &inventory([3, 0, 0, 0]),
            &main_package(PackageVersion::new(3, 0, 0, 0)),
            &linked,
        ),
        CatalogVersionState::UpToDate
    );
    assert_eq!(
        compare_catalog_version(
            &inventory([4, 0, 0, 0]),
            &main_package(PackageVersion::new(3, 0, 0, 0)),
            &linked,
        ),
        CatalogVersionState::VersionAheadOfCatalog
    );

    let mut mismatched = main_package(PackageVersion::new(3, 0, 0, 0));
    mismatched.publisher = Some("CN=Other".to_owned());
    assert_eq!(
        compare_catalog_version(&inventory([2, 0, 0, 0]), &mismatched, &linked),
        CatalogVersionState::SourceIdentityMismatch
    );

    let unresolved = correlate_package(&inventory([2, 0, 0, 0]), &[], None, 100);
    assert_eq!(
        compare_catalog_version(
            &inventory([2, 0, 0, 0]),
            &main_package(PackageVersion::new(3, 0, 0, 0)),
            &unresolved,
        ),
        CatalogVersionState::UnresolvedAssociation
    );
}

#[test]
fn initial_schema_round_trips_association_confidence() {
    let database = TestDatabase::new("m5-initial-schema");
    let store = Persistence::open(database.path()).expect("create initial database");
    let association = PackageAssociation {
        package_family_name: "Example.App_abc".to_owned(),
        product_id: Some("product".to_owned()),
        content_id: Some("content-product".to_owned()),
        identity_name: "Example.App".to_owned(),
        publisher: "CN=Example".to_owned(),
        confidence: AssociationConfidence::VerifiedDeployment,
        observed_at: 123,
    };
    store
        .upsert_package_association(&association)
        .expect("persist association");

    assert_eq!(store.schema_version().expect("schema version"), 2);
    assert_eq!(
        store
            .package_association("Example.App_abc")
            .expect("load association"),
        Some(association)
    );
}
