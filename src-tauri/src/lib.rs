#[cfg(not(feature = "broker-dependency"))]
// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg(not(feature = "broker-dependency"))]
#[tauri::command]
fn probe_deployment() -> Result<DeploymentProbe, String> {
    deployment::WindowsDeploymentBackend::probe().map_err(|error| error.to_string())
}

#[cfg(not(feature = "broker-dependency"))]
#[tauri::command]
fn scan_installed_packages(
    scope: deployment::DeploymentScope,
) -> Result<inventory::InventorySnapshot, String> {
    deployment_coordinator::DeploymentCoordinator::scan(scope).map_err(|error| error.to_string())
}

#[cfg(not(feature = "broker-dependency"))]
#[tauri::command]
fn install_package(
    scope: deployment::DeploymentScope,
    package: package_validation::VerifiedPackageSet,
) -> Result<inventory::InventorySnapshot, String> {
    deployment_coordinator::DeploymentCoordinator::install(scope, &package)
        .map_err(|error| error.to_string())
}

#[cfg(not(feature = "broker-dependency"))]
#[tauri::command]
fn uninstall_package(
    scope: deployment::DeploymentScope,
    package_family_name: String,
    package_full_names: Vec<String>,
) -> Result<inventory::InventorySnapshot, String> {
    deployment_coordinator::DeploymentCoordinator::uninstall(
        scope,
        &package_family_name,
        &package_full_names,
    )
    .map_err(|error| error.to_string())
}

#[cfg(not(feature = "broker-dependency"))]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            probe_deployment,
            scan_installed_packages,
            install_package,
            uninstall_package
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
pub mod applicability;
pub mod broker;
pub mod broker_launcher;
pub mod broker_protocol;
pub mod cache;
pub mod catalog;
pub mod deployment;
pub mod deployment_coordinator;
pub mod domain;
pub mod download;
pub mod error;
pub mod inventory;
pub mod jobs;
pub mod package_validation;
pub mod persistence;
pub mod resolver;
pub mod settings;
pub mod verification;

#[cfg(not(feature = "broker-dependency"))]
use deployment::DeploymentProbe;
