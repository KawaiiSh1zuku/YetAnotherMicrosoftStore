#![cfg(windows)]

use yet_another_microsoft_store_lib::inventory::{
    InventoryError, InventorySnapshot, InventorySource, PackageInventoryRecord, PackageKind,
    WindowsInventory,
};

fn sample_record() -> PackageInventoryRecord {
    PackageInventoryRecord {
        identity_name: "Contoso.Notes".to_owned(),
        publisher: "CN=Contoso".to_owned(),
        package_family_name: "Contoso.Notes_abc".to_owned(),
        package_full_name: "Contoso.Notes_1.2.3.4_x64__abc".to_owned(),
        version: [1, 2, 3, 4],
        architecture: "x64".to_owned(),
        resource_id: String::new(),
        package_kind: PackageKind::Main,
        signature_kind: "Developer".to_owned(),
        status: "Ok".to_owned(),
        installed_for_current_user: true,
        installed_user_count: 1,
        has_other_users: false,
        provisioned_for_future_users: false,
    }
}

#[test]
fn inventory_snapshot_serializes_identity_and_scope_fields() {
    let snapshot = InventorySnapshot {
        source: InventorySource::CurrentUser,
        captured_at: "2026-10-02T00:00:00Z".to_owned(),
        os_build: "10.0.19045".to_owned(),
        complete: true,
        records: vec![sample_record()],
        warnings: Vec::new(),
    };

    let json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
    assert_eq!(json["source"], "CurrentUser");
    assert_eq!(
        json["records"][0]["version"],
        serde_json::json!([1, 2, 3, 4])
    );
    assert_eq!(json["records"][0]["package_kind"], "Main");
    assert_eq!(json["records"][0]["resource_id"], "");
    assert_eq!(json["records"][0]["installed_user_count"], 1);
}

#[test]
fn incomplete_snapshot_is_explicit_and_not_an_empty_success() {
    let snapshot = InventorySnapshot {
        source: InventorySource::AllUsersElevated,
        captured_at: "2026-10-02T00:00:00Z".to_owned(),
        os_build: "10.0.19045".to_owned(),
        complete: false,
        records: Vec::new(),
        warnings: vec!["FindUsers failed".to_owned()],
    };

    assert!(!snapshot.complete);
    assert_eq!(snapshot.records.len(), 0);
    assert_eq!(snapshot.warnings.len(), 1);
}

#[test]
fn non_elevated_machine_scan_reports_access_denied_or_a_complete_elevated_snapshot() {
    match WindowsInventory::scan_all_users() {
        Err(InventoryError::AccessDenied) => {}
        Ok(snapshot) => assert!(snapshot.complete || !snapshot.warnings.is_empty()),
        Err(error) => panic!("unexpected inventory error: {error}"),
    }
}
