use std::{fs, process::Command};

use yet_another_microsoft_store_lib::diagnostics::{
    DiagnosticEvent, DiagnosticService, SingleInstanceGuard, NETWORK_HOST_ALLOWLIST,
};
use yet_another_microsoft_store_lib::settings::MICROSOFT_PACKAGE_HOSTS;

struct TestRoot(std::path::PathBuf);

impl TestRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "yamstore-m8-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("create test root");
        Self(path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn diagnostics_export_contains_only_closed_redacted_fields() {
    let root = TestRoot::new();
    let service = DiagnosticService::new(root.0.join("state"), root.0.join("exports"), true)
        .expect("create diagnostics service");
    service
        .record(DiagnosticEvent::WorkerFailure)
        .expect("record safe event");

    let exported = service.export().expect("export diagnostics");
    let second_export = service.export().expect("export diagnostics again");
    let report = fs::read_to_string(root.0.join("exports").join(&exported.file_name))
        .expect("read diagnostics export");
    let value: serde_json::Value = serde_json::from_str(&report).expect("parse report");

    assert_eq!(exported.destination, "downloads");
    assert_ne!(exported.file_name, second_export.file_name);
    assert!(root
        .0
        .join("exports")
        .join(second_export.file_name)
        .is_file());
    assert_eq!(value["schemaVersion"], 1);
    assert_eq!(
        value["networkHostAllowlist"],
        serde_json::json!(NETWORK_HOST_ALLOWLIST)
    );
    assert!(report.contains("worker_failure"));
    for forbidden in ["https://", "http://", "token=", "C:\\\\", "E:\\\\"] {
        assert!(
            !report.contains(forbidden),
            "diagnostics leaked {forbidden}"
        );
    }
}

#[test]
fn unclean_session_is_reported_once_and_clean_shutdown_removes_marker() {
    let root = TestRoot::new();
    let state = root.0.join("state");
    let exports = root.0.join("exports");

    let first = DiagnosticService::new(&state, &exports, true).expect("first session");
    assert!(!first.previous_session_unclean());
    drop(first);

    let recovered = DiagnosticService::new(&state, &exports, true).expect("recover session");
    assert!(recovered.previous_session_unclean());
    recovered.clean_shutdown().expect("clean shutdown");

    let clean = DiagnosticService::new(&state, &exports, true).expect("clean session");
    assert!(!clean.previous_session_unclean());
    clean.clean_shutdown().expect("clean final session");
}

#[test]
fn disabled_diagnostics_do_not_persist_and_the_event_log_is_bounded() {
    let root = TestRoot::new();
    let state = root.0.join("state");
    let exports = root.0.join("exports");
    let service = DiagnosticService::new(&state, &exports, false).expect("disabled service");
    service
        .record(DiagnosticEvent::WorkerFailure)
        .expect("disabled record is a no-op");
    assert!(!state.join("events.jsonl").exists());
    assert!(!state.join("session-active").exists());

    service.set_enabled(true).expect("enable diagnostics");
    for _ in 0..2_000 {
        service
            .record(DiagnosticEvent::WorkerFailure)
            .expect("record bounded event");
    }
    assert!(
        fs::metadata(state.join("events.jsonl"))
            .expect("event log")
            .len()
            < 70 * 1024
    );

    service.set_enabled(false).expect("disable diagnostics");
    assert!(!state.join("events.jsonl").exists());
    assert!(!state.join("session-active").exists());
}

#[test]
fn diagnostic_network_audit_matches_production_endpoints() {
    assert_eq!(
        NETWORK_HOST_ALLOWLIST,
        [
            "displaycatalog.mp.microsoft.com",
            "fe3.delivery.mp.microsoft.com",
            MICROSOFT_PACKAGE_HOSTS[0],
            MICROSOFT_PACKAGE_HOSTS[1],
        ]
    );
}

#[test]
fn elevated_runtime_configuration_has_no_broker_sidecar() {
    let manifest = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/windows/app.manifest"))
        .expect("main executable manifest");
    assert!(manifest.contains("level=\"requireAdministrator\""));
    assert!(manifest.contains("name=\"Microsoft.Windows.Common-Controls\""));
    assert!(manifest.contains("version=\"6.0.0.0\""));

    let tauri: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("parse tauri config");
    assert!(tauri["bundle"].get("externalBin").is_none());

    let package: serde_json::Value =
        serde_json::from_str(include_str!("../../package.json")).expect("parse package scripts");
    assert!(package["scripts"].get("build:broker").is_none());
}

#[test]
fn direct_cargo_build_defaults_to_the_embedded_frontend_protocol() {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
            concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"),
        ])
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse cargo metadata");
    let package = metadata["packages"]
        .as_array()
        .and_then(|packages| {
            packages
                .iter()
                .find(|package| package["name"] == "yet-another-microsoft-store")
        })
        .expect("application package metadata");

    assert_eq!(
        package["features"]["default"],
        serde_json::json!(["custom-protocol"])
    );
    assert_eq!(
        package["features"]["custom-protocol"],
        serde_json::json!(["tauri/custom-protocol"])
    );
}

#[test]
fn single_instance_guard_rejects_a_second_owner_and_recovers_after_drop() {
    let first = SingleInstanceGuard::acquire().expect("first instance");
    let error = match SingleInstanceGuard::acquire() {
        Ok(_) => panic!("second instance must fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    drop(first);
    SingleInstanceGuard::acquire().expect("guard can be reacquired after release");
}
