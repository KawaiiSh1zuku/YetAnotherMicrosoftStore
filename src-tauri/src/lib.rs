// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn probe_deployment() -> Result<DeploymentProbe, String> {
    deployment::WindowsDeploymentBackend::probe().map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet, probe_deployment])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
pub mod broker;
pub mod catalog;
pub mod deployment;
pub mod resolver;

use deployment::DeploymentProbe;
