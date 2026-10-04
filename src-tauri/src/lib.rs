struct RuntimeState {
    backend: app_runtime::ProductionApiBackend,
    diagnostics: diagnostics::DiagnosticService,
}

fn api(
    state: &tauri::State<'_, RuntimeState>,
) -> tauri_api::TauriApi<app_runtime::ProductionApiBackend> {
    tauri_api::TauriApi::new(state.backend.clone())
}

#[tauri::command]
async fn search_apps(
    state: tauri::State<'_, RuntimeState>,
    request: tauri_api::SearchRequest,
) -> Result<Vec<tauri_api::ApiCatalogProduct>, error::AppErrorDto> {
    api(&state).search_apps(request).await
}

#[tauri::command]
async fn get_app_details(
    state: tauri::State<'_, RuntimeState>,
    request: tauri_api::DetailsRequest,
) -> Result<tauri_api::ApiAppDetails, error::AppErrorDto> {
    api(&state).get_app_details(request).await
}

#[tauri::command]
async fn launch_installed_app(
    state: tauri::State<'_, RuntimeState>,
    product_id: String,
) -> Result<(), error::AppErrorDto> {
    api(&state).launch_installed_app(product_id).await
}

#[tauri::command]
async fn scan_installed_packages(
    state: tauri::State<'_, RuntimeState>,
    scope: tauri_api::ApiDeploymentScope,
) -> Result<tauri_api::ApiInventorySnapshot, error::AppErrorDto> {
    api(&state).scan_installed_packages(scope).await
}

#[tauri::command]
async fn scan_updates(
    state: tauri::State<'_, RuntimeState>,
) -> Result<tauri_api::ApiUpdateScanResult, error::AppErrorDto> {
    api(&state).scan_updates().await
}

#[tauri::command]
async fn start_install(
    state: tauri::State<'_, RuntimeState>,
    request: tauri_api::StartJobRequest,
) -> Result<tauri_api::ApiJobSnapshot, error::AppErrorDto> {
    api(&state).start_install(request).await
}

#[tauri::command]
async fn start_update(
    state: tauri::State<'_, RuntimeState>,
    request: tauri_api::StartJobRequest,
) -> Result<tauri_api::ApiJobSnapshot, error::AppErrorDto> {
    api(&state).start_update(request).await
}

#[tauri::command]
async fn request_job_control(
    state: tauri::State<'_, RuntimeState>,
    request: tauri_api::JobControlRequest,
) -> Result<tauri_api::ApiJobSnapshot, error::AppErrorDto> {
    api(&state).request_job_control(request).await
}

#[tauri::command]
async fn terminate_job_package_processes(
    state: tauri::State<'_, RuntimeState>,
    job_id: String,
) -> Result<package_process::TerminatePackageProcessesResult, error::AppErrorDto> {
    api(&state).terminate_job_package_processes(job_id).await
}

#[tauri::command]
async fn get_job(
    state: tauri::State<'_, RuntimeState>,
    job_id: String,
) -> Result<Option<tauri_api::ApiJobSnapshot>, error::AppErrorDto> {
    api(&state).get_job(job_id).await
}

#[tauri::command]
async fn list_jobs(
    state: tauri::State<'_, RuntimeState>,
) -> Result<Vec<tauri_api::ApiJobSnapshot>, error::AppErrorDto> {
    api(&state).list_jobs().await
}

#[tauri::command]
async fn list_job_events(
    state: tauri::State<'_, RuntimeState>,
    request: tauri_api::ListJobEventsRequest,
) -> Result<tauri_api::ApiJobEventPage, error::AppErrorDto> {
    api(&state).list_job_events(request).await
}

#[tauri::command]
async fn get_settings(
    state: tauri::State<'_, RuntimeState>,
) -> Result<tauri_api::ApiAppSettings, error::AppErrorDto> {
    api(&state).get_settings().await
}

#[tauri::command]
async fn update_settings(
    state: tauri::State<'_, RuntimeState>,
    settings: tauri_api::ApiAppSettings,
) -> Result<tauri_api::ApiAppSettings, error::AppErrorDto> {
    let previous_diagnostics = state.diagnostics.is_enabled();
    let requested_diagnostics = settings.diagnostics_enabled;
    state
        .diagnostics
        .set_enabled(requested_diagnostics)
        .map_err(|_| diagnostics_error())?;
    match api(&state).update_settings(settings).await {
        Ok(settings) => Ok(settings),
        Err(error) => {
            let _ = state.diagnostics.set_enabled(previous_diagnostics);
            Err(error)
        }
    }
}

#[tauri::command]
async fn clear_cache(state: tauri::State<'_, RuntimeState>) -> Result<(), error::AppErrorDto> {
    api(&state).clear_cache().await
}

#[tauri::command]
async fn cleanup_database(
    state: tauri::State<'_, RuntimeState>,
) -> Result<tauri_api::ApiDatabaseCleanupReport, error::AppErrorDto> {
    api(&state).cleanup_database().await
}

#[tauri::command]
fn export_diagnostics(
    state: tauri::State<'_, RuntimeState>,
) -> Result<diagnostics::DiagnosticExport, error::AppErrorDto> {
    state.diagnostics.export().map_err(|_| diagnostics_error())
}

fn diagnostics_error() -> error::AppErrorDto {
    error::AppErrorDto::new(
        error::ErrorCode::DeploymentFailed,
        error::RetryAdvice::Retry,
    )
}

fn start_runtime_tasks(
    app: tauri::AppHandle,
    backend: app_runtime::ProductionApiBackend,
    diagnostics: diagnostics::DiagnosticService,
) -> Result<(), std::io::Error> {
    use std::time::Duration;
    use tauri::Emitter;
    use tauri_api::ApiBackend;

    let worker_backend = backend.clone();
    let wake = worker_backend.worker_wake();
    std::thread::Builder::new()
        .name("yamstore-worker".to_owned())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                loop {
                    match worker_backend.run_worker_once().await {
                        Ok(job_worker::RunOnceOutcome::Processed { .. }) => continue,
                        Ok(
                            job_worker::RunOnceOutcome::Idle
                            | job_worker::RunOnceOutcome::LeaseBusy,
                        ) => {
                            tokio::select! {
                                () = wake.notified() => {},
                                () = tokio::time::sleep(Duration::from_secs(1)) => {},
                            }
                        }
                        Ok(job_worker::RunOnceOutcome::LeaseLost) => {
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                        Err(_) => {
                            let _ = diagnostics.record(diagnostics::DiagnosticEvent::WorkerFailure);
                            tokio::time::sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
            });
        })
        .map_err(|_| std::io::Error::other("worker thread initialization failed"))?;

    tauri::async_runtime::spawn(async move {
        let mut cursor = 0_u64;
        loop {
            let request = tauri_api::ListJobEventsRequest {
                after_cursor: Some(cursor),
                limit: 100,
            };
            match backend.list_job_events(request).await {
                Ok(events) if !events.is_empty() => {
                    for event in events {
                        cursor = event.cursor;
                        let hint = tauri_api::JobChangedHint {
                            job_id: event.job_id,
                            sequence: event.sequence,
                            updated_at: event.snapshot.job.updated_at,
                        };
                        let _ = app.emit(tauri_api::JOB_CHANGED_EVENT, hint);
                    }
                }
                Ok(_) | Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;

    let single_instance = match diagnostics::SingleInstanceGuard::acquire() {
        Ok(guard) => guard,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return,
        Err(error) => panic!("failed to acquire application instance guard: {error}"),
    };
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_root = app.path().app_data_dir()?;
            let cache_root = app.path().app_cache_dir()?.join("packages");
            let download_root = app.path().download_dir()?;
            let paths = app_runtime::RuntimePaths::new(data_root.join("state.sqlite3"), cache_root)
                .map_err(|_| std::io::Error::other("runtime path initialization failed"))?;
            let backend = app_runtime::ProductionApiBackend::new(paths);
            let diagnostics_enabled = backend
                .diagnostics_enabled()
                .map_err(|_| std::io::Error::other("settings initialization failed"))?;
            let diagnostics = diagnostics::DiagnosticService::new(
                data_root.join("diagnostics"),
                download_root,
                diagnostics_enabled,
            )?;
            let panic_diagnostics = diagnostics.clone();
            let previous_panic_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |panic_info| {
                let _ = panic_diagnostics.record(diagnostics::DiagnosticEvent::Panic);
                previous_panic_hook(panic_info);
            }));
            start_runtime_tasks(app.handle().clone(), backend.clone(), diagnostics.clone())?;
            app.manage(RuntimeState {
                backend,
                diagnostics,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            search_apps,
            get_app_details,
            launch_installed_app,
            scan_installed_packages,
            scan_updates,
            start_install,
            start_update,
            request_job_control,
            terminate_job_package_processes,
            get_job,
            list_jobs,
            list_job_events,
            get_settings,
            update_settings,
            clear_cache,
            cleanup_database,
            export_diagnostics
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            let _ = handle.state::<RuntimeState>().diagnostics.clean_shutdown();
        }
    });
    drop(single_instance);
}
pub mod app_runtime;
pub mod applicability;
pub mod cache;
pub mod catalog;
pub mod deployment;
pub mod deployment_coordinator;
pub mod deployment_orchestrator;
pub mod deployment_plan;
pub mod diagnostics;
pub mod domain;
pub mod download;
pub mod error;
pub mod identity;
pub mod inventory;
pub mod job_events;
pub mod job_store;
pub mod job_worker;
pub mod jobs;
pub mod package;
pub mod package_application;
pub mod package_process;
pub mod package_validation;
pub mod persistence;
pub mod resolver;
pub mod settings;
pub mod tauri_api;
pub mod verification;
