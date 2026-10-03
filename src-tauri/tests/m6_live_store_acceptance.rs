#![cfg(windows)]

use std::{collections::HashSet, fs, path::PathBuf};

use yet_another_microsoft_store_lib::{
    app_runtime::{ProductionApiBackend, RuntimePaths},
    deployment::{DeploymentScope, WindowsDeploymentBackend},
    domain::{Architecture, PackageFormat, PackageKind, PackageVersion, ProxyMode},
    error::AppErrorDto,
    job_events::{JobTarget, JobTargetRole},
    job_worker::RunOnceOutcome,
    jobs::JobStage,
    persistence::Persistence,
    resolver::{PackageGraph, PackageResolver, ResolvedPackage, StoreLibResolverAdapter},
    settings::{NetworkPolicy, MICROSOFT_PACKAGE_HOSTS},
    tauri_api::{ApiBackend, ApiDeploymentScope, DetailsRequest, StartJobRequest, TauriApi},
};

use yet_another_microsoft_store_lib::inventory::{InventorySnapshot, PackageInventoryRecord};

const LIVE_GATE: &str = "M6_LIVE_ACCEPTANCE";
const PRODUCT_ID: &str = "M6_PRODUCT_ID";
const MARKET: &str = "M6_MARKET";
const LANGUAGE: &str = "M6_LANGUAGE";

struct IsolatedRuntime {
    root: PathBuf,
    paths: RuntimePaths,
}

impl IsolatedRuntime {
    fn new() -> Result<Self, String> {
        let root = std::env::temp_dir().join(format!("yamstore-m6-live-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root)
            .map_err(|error| format!("could not create isolated runtime: {error}"))?;
        let paths = RuntimePaths::new(root.join("state.sqlite3"), root.join("cache"))
            .map_err(|error| safe_api_error("isolated runtime paths were rejected", &error))?;
        Ok(Self { root, paths })
    }
}

impl Drop for IsolatedRuntime {
    fn drop(&mut self) {
        let Some(name) = self.root.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        if name.starts_with("yamstore-m6-live-") && self.root.starts_with(std::env::temp_dir()) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

struct CleanupGuard {
    baseline: InventorySnapshot,
    database_path: PathBuf,
    job_id: Option<String>,
    targets: Vec<JobTarget>,
    armed: bool,
}

impl CleanupGuard {
    fn new(baseline: InventorySnapshot, database_path: PathBuf) -> Self {
        Self {
            baseline,
            database_path,
            job_id: None,
            targets: Vec::new(),
            armed: true,
        }
    }

    fn track_job(&mut self, job_id: String) {
        self.job_id = Some(job_id);
    }

    fn set_targets(&mut self, targets: Vec<JobTarget>) {
        self.targets = targets;
    }

    fn cleanup(&mut self) -> Result<(), String> {
        self.reload_targets_if_needed();
        let result = cleanup_to_baseline(&self.baseline, &self.targets);
        if result.is_ok() {
            self.armed = false;
        }
        result
    }

    fn reload_targets_if_needed(&mut self) {
        if !self.targets.is_empty() {
            return;
        }
        let Some(job_id) = self.job_id.as_deref() else {
            return;
        };
        if let Ok(persistence) = Persistence::open(&self.database_path) {
            if let Ok(targets) = persistence.job_targets(job_id) {
                self.targets = targets;
            }
        }
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        if self.armed {
            self.reload_targets_if_needed();
            if let Err(error) = cleanup_to_baseline(&self.baseline, &self.targets) {
                eprintln!("M6 live acceptance emergency cleanup failed: {error}");
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires M6_LIVE_ACCEPTANCE=1 and explicit product/market/language"]
async fn production_current_user_install_round_trip_restores_exact_inventory() {
    assert_eq!(
        std::env::var(LIVE_GATE).as_deref(),
        Ok("1"),
        "set M6_LIVE_ACCEPTANCE=1 to authorize a real CurrentUser install/uninstall round trip"
    );
    let product_id = required_env(PRODUCT_ID);
    let market = required_env(MARKET).to_ascii_uppercase();
    let language = required_env(LANGUAGE);

    let runtime = IsolatedRuntime::new().expect("isolated runtime must be available");
    let backend = ProductionApiBackend::new(runtime.paths.clone());
    let api = TauriApi::new(backend.clone());

    configure_proxy_from_environment(&api)
        .await
        .expect("explicit live proxy settings must be valid");

    let baseline = backend
        .scan_installed_packages(DeploymentScope::CurrentUser)
        .await
        .expect("CurrentUser pre-scan must succeed");
    assert_complete_inventory(&baseline, "pre-scan").expect("pre-scan must be complete");
    let mut cleanup = CleanupGuard::new(baseline, runtime.paths.database_path().to_path_buf());

    let acceptance = run_acceptance(
        &api,
        &backend,
        &runtime.paths,
        &product_id,
        &market,
        &language,
        &mut cleanup,
    )
    .await;
    let cleanup_result = cleanup.cleanup();

    if let Err(error) = cleanup_result {
        panic!("M6 live acceptance did not restore the exact CurrentUser baseline: {error}");
    }
    if let Err(error) = acceptance {
        panic!("M6 live acceptance failed before the verified round trip completed: {error}");
    }

    println!(
        "M6 live acceptance: product={product_id} market={market} language={language} scope=current_user completed=true exact_baseline_restored=true"
    );
}

async fn run_acceptance(
    api: &TauriApi<ProductionApiBackend>,
    backend: &ProductionApiBackend,
    paths: &RuntimePaths,
    product_id: &str,
    market: &str,
    language: &str,
    cleanup: &mut CleanupGuard,
) -> Result<(), String> {
    let details = api
        .get_app_details(DetailsRequest {
            product_id: product_id.to_owned(),
            market: market.to_owned(),
            language: language.to_owned(),
        })
        .await
        .map_err(|error| safe_api_error("production details lookup failed", &error))?;
    if details.product_id != product_id || !details.market.eq_ignore_ascii_case(market) {
        return Err("production details identity or market did not match the request".to_owned());
    }
    let package_family_name = details
        .package_family_name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "production details did not provide an unambiguous package family".to_owned()
        })?;

    let mut resolver = StoreLibResolverAdapter::production_for_locale(market, language)
        .map_err(|error| format!("production resolver locale was rejected: {error}"))?;
    let graph = resolver
        .resolve(product_id)
        .await
        .map_err(|error| format!("production package resolution failed: {error}"))?;
    validate_graph(&graph, product_id, market)?;
    reject_unsafe_baseline(&cleanup.baseline, package_family_name, &graph)?;

    let started = api
        .start_install(StartJobRequest {
            product_id: product_id.to_owned(),
            market: market.to_owned(),
            language: language.to_owned(),
            scope: ApiDeploymentScope::CurrentUser,
        })
        .await
        .map_err(|error| safe_api_error("production install enqueue failed", &error))?;
    cleanup.track_job(started.job_id.clone());

    let worker_result = match backend.create_worker() {
        Ok(mut worker) => worker.run_once().await.map_err(|error| error.to_string()),
        Err(error) => Err(safe_api_error("production worker creation failed", &error)),
    };
    let persistence = Persistence::open(paths.database_path())
        .map_err(|error| format!("could not reopen isolated job store: {error}"))?;
    let targets = persistence
        .job_targets(&started.job_id)
        .map_err(|error| format!("could not read frozen job targets: {error}"))?;
    cleanup.set_targets(targets.clone());

    let outcome = worker_result
        .map_err(|error| format!("production worker infrastructure failed: {error}"))?;
    match outcome {
        RunOnceOutcome::Processed { job_id, stage }
            if job_id == started.job_id && stage == JobStage::Completed => {}
        other => {
            let failed = api
                .get_job(started.job_id.clone())
                .await
                .map_err(|error| safe_api_error("failed job lookup failed", &error))?
                .ok_or_else(|| "failed job disappeared from the durable store".to_owned())?;
            let (code, retry) = failed.error.as_ref().map_or_else(
                || ("none".to_owned(), "none".to_owned()),
                |error| (format!("{:?}", error.code), format!("{:?}", error.retry)),
            );
            return Err(format!(
                "production worker did not complete the job: outcome={other:?} stage={:?} sequence={} code={code} retry={retry} bytes_done={} bytes_total={:?} frozen_targets={}",
                failed.stage,
                failed.sequence,
                failed.bytes_done,
                failed.bytes_total,
                targets.len(),
            ));
        }
    }
    if targets.is_empty()
        || targets
            .iter()
            .filter(|target| target.role == JobTargetRole::Main)
            .count()
            != 1
    {
        return Err("completed job did not freeze exactly one main target".to_owned());
    }

    let completed = api
        .get_job(started.job_id.clone())
        .await
        .map_err(|error| safe_api_error("completed job lookup failed", &error))?
        .ok_or_else(|| "completed job disappeared from the durable store".to_owned())?;
    if completed.stage != JobStage::Completed
        || completed.package_family_name.as_deref() != Some(package_family_name)
    {
        return Err("durable completed snapshot did not preserve the expected identity".to_owned());
    }

    let post = backend
        .scan_installed_packages(DeploymentScope::CurrentUser)
        .await
        .map_err(|error| safe_api_error("CurrentUser post-scan failed", &error))?;
    assert_complete_inventory(&post, "post-scan")?;
    assert_exact_postcondition(&cleanup.baseline, &post, &targets)?;
    Ok(())
}

async fn configure_proxy_from_environment(
    api: &TauriApi<ProductionApiBackend>,
) -> Result<(), String> {
    let mut settings = api
        .get_settings()
        .await
        .map_err(|error| safe_api_error("could not load live settings", &error))?;
    let mode = std::env::var("M6_PROXY_MODE")
        .unwrap_or_else(|_| "disabled".to_owned())
        .to_ascii_lowercase();
    settings.proxy_mode = match mode.as_str() {
        "disabled" => ProxyMode::Disabled,
        "system" => ProxyMode::System,
        "http" => ProxyMode::Http,
        "https" => ProxyMode::Https,
        "socks5" => ProxyMode::Socks5,
        _ => {
            return Err("M6_PROXY_MODE must be disabled, system, http, https, or socks5".to_owned())
        }
    };
    match settings.proxy_mode {
        ProxyMode::Disabled | ProxyMode::System => {
            settings.proxy_host = None;
            settings.proxy_port = None;
        }
        ProxyMode::Http | ProxyMode::Https | ProxyMode::Socks5 => {
            settings.proxy_host = Some(required_env("M6_PROXY_HOST"));
            settings.proxy_port = Some(
                required_env("M6_PROXY_PORT")
                    .parse::<u16>()
                    .map_err(|_| "M6_PROXY_PORT must be a non-zero TCP port".to_owned())?,
            );
        }
    }
    api.update_settings(settings)
        .await
        .map_err(|error| safe_api_error("live proxy settings were rejected", &error))?;
    Ok(())
}

fn validate_graph(graph: &PackageGraph, product_id: &str, market: &str) -> Result<(), String> {
    if graph.product_id.as_deref() != Some(product_id)
        || graph
            .market
            .as_deref()
            .is_none_or(|value| !value.eq_ignore_ascii_case(market))
        || graph.packages.is_empty()
    {
        return Err("resolved graph identity, market, or package set was invalid".to_owned());
    }

    let mut main_identities = HashSet::new();
    for package in &graph.packages {
        validate_resolved_package(package)?;
        if package.package_kind == PackageKind::Main {
            main_identities.insert((
                package.identity_name.as_deref().unwrap_or_default(),
                package.publisher.as_deref().unwrap_or_default(),
            ));
        }
    }
    if main_identities.len() != 1 {
        return Err(
            "resolved graph did not contain one unambiguous main package identity".to_owned(),
        );
    }
    Ok(())
}

fn validate_resolved_package(package: &ResolvedPackage) -> Result<(), String> {
    let url = package
        .package_uri
        .as_deref()
        .ok_or_else(|| "resolved package did not contain a download URL".to_owned())?;
    let policy = NetworkPolicy::production(MICROSOFT_PACKAGE_HOSTS)
        .map_err(|_| "production network policy was invalid".to_owned())?;
    policy
        .validate_url(url)
        .map_err(|_| "resolved package URL violated the Microsoft delivery policy".to_owned())?;
    if package.file_size.is_none_or(|size| size == 0) {
        return Err("resolved package did not contain a positive size".to_owned());
    }
    let sha256 = package
        .sha256
        .as_deref()
        .ok_or_else(|| "resolved package did not contain SHA-256".to_owned())?;
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("resolved package SHA-256 was not normalized lowercase hexadecimal".to_owned());
    }
    if package.identity_name.as_deref().is_none_or(str::is_empty)
        || package.publisher.as_deref().is_none_or(str::is_empty)
        || package.file_name.as_deref().is_none_or(str::is_empty)
    {
        return Err("resolved package identity metadata was incomplete".to_owned());
    }
    if !matches!(
        package.format,
        PackageFormat::Msix
            | PackageFormat::Appx
            | PackageFormat::MsixBundle
            | PackageFormat::AppxBundle
    ) {
        return Err("resolved graph contained an unsupported package format".to_owned());
    }
    Ok(())
}

fn reject_unsafe_baseline(
    baseline: &InventorySnapshot,
    package_family_name: &str,
    graph: &PackageGraph,
) -> Result<(), String> {
    assert_complete_inventory(baseline, "pre-scan")?;
    let mains = graph
        .packages
        .iter()
        .filter(|package| package.package_kind == PackageKind::Main)
        .collect::<Vec<_>>();
    let main_matches = baseline
        .records
        .iter()
        .filter(|record| {
            record
                .package_family_name
                .eq_ignore_ascii_case(package_family_name)
                || mains.iter().any(|package| same_identity(record, package))
        })
        .count();
    if main_matches != 0 {
        return Err(
            "target main package is already present; refusing a live install or upgrade".to_owned(),
        );
    }

    for dependency in graph
        .packages
        .iter()
        .filter(|package| package.package_kind != PackageKind::Main)
    {
        if baseline.records.iter().any(|record| {
            same_identity(record, dependency) && inventory_version(record) < dependency.version
        }) {
            return Err(
                "a resolved dependency could upgrade an existing package; refusing before download"
                    .to_owned(),
            );
        }
    }
    Ok(())
}

fn assert_exact_postcondition(
    baseline: &InventorySnapshot,
    post: &InventorySnapshot,
    targets: &[JobTarget],
) -> Result<(), String> {
    let baseline_names = full_name_set(baseline);
    let introduced = post
        .records
        .iter()
        .filter(|record| !baseline_names.contains(&record.package_full_name))
        .collect::<Vec<_>>();
    if introduced.is_empty() {
        return Err("completed job introduced no new package full names".to_owned());
    }
    for target in targets {
        let matches = post
            .records
            .iter()
            .filter(|record| exact_target_match(record, target))
            .count();
        if matches != 1 {
            return Err(
                "a frozen target did not map to exactly one post-scan full name".to_owned(),
            );
        }
    }
    for record in introduced {
        if targets
            .iter()
            .filter(|target| exact_target_match(record, target))
            .count()
            != 1
        {
            return Err(
                "post-scan contained a new package outside the frozen target set".to_owned(),
            );
        }
    }
    Ok(())
}

fn cleanup_to_baseline(baseline: &InventorySnapshot, targets: &[JobTarget]) -> Result<(), String> {
    let current =
        yet_another_microsoft_store_lib::deployment_coordinator::DeploymentCoordinator::scan(
            DeploymentScope::CurrentUser,
        )
        .map_err(|error| format!("cleanup pre-scan failed: {error}"))?;
    assert_complete_inventory(&current, "cleanup pre-scan")?;
    let baseline_names = full_name_set(baseline);
    let mut removable = Vec::new();
    let mut unknown_introduced = false;
    for record in current
        .records
        .iter()
        .filter(|record| !baseline_names.contains(&record.package_full_name))
    {
        let matched = targets
            .iter()
            .filter(|target| exact_target_match(record, target))
            .collect::<Vec<_>>();
        if matched.len() != 1 {
            unknown_introduced = true;
            continue;
        }
        removable.push((role_rank(matched[0].role), record.package_full_name.clone()));
    }
    removable.sort_by_key(|(rank, full_name)| (*rank, full_name.clone()));
    if unknown_introduced {
        removable.retain(|(rank, _)| *rank == role_rank(JobTargetRole::Main));
    }

    for (_, full_name) in removable {
        let refreshed =
            yet_another_microsoft_store_lib::deployment_coordinator::DeploymentCoordinator::scan(
                DeploymentScope::CurrentUser,
            )
            .map_err(|error| format!("cleanup inventory refresh failed: {error}"))?;
        assert_complete_inventory(&refreshed, "cleanup inventory refresh")?;
        let still_present = refreshed
            .records
            .into_iter()
            .any(|record| record.package_full_name == full_name);
        if still_present {
            WindowsDeploymentBackend::remove_current_user(&full_name)
                .map_err(|error| format!("exact CurrentUser removal failed: {error}"))?;
        }
    }

    let final_snapshot =
        yet_another_microsoft_store_lib::deployment_coordinator::DeploymentCoordinator::scan(
            DeploymentScope::CurrentUser,
        )
        .map_err(|error| format!("cleanup final scan failed: {error}"))?;
    assert_complete_inventory(&final_snapshot, "cleanup final scan")?;
    if full_name_set(&final_snapshot) != baseline_names {
        let reason = if unknown_introduced {
            "cleanup preserved a new full name outside the frozen target set"
        } else {
            "final CurrentUser full-name set differs from the complete pre-scan baseline"
        };
        return Err(reason.to_owned());
    }
    Ok(())
}

fn assert_complete_inventory(snapshot: &InventorySnapshot, phase: &str) -> Result<(), String> {
    if snapshot.complete && snapshot.warnings.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{phase} inventory was incomplete; refusing to mutate package state"
        ))
    }
}

fn same_identity(record: &PackageInventoryRecord, package: &ResolvedPackage) -> bool {
    package.identity_name.as_deref().is_some_and(|identity| {
        record.identity_name.eq_ignore_ascii_case(identity)
            && package
                .publisher
                .as_deref()
                .is_some_and(|publisher| record.publisher == publisher)
    })
}

fn exact_target_match(record: &PackageInventoryRecord, target: &JobTarget) -> bool {
    record
        .identity_name
        .eq_ignore_ascii_case(&target.identity_name)
        && record.publisher == target.publisher
        && inventory_version(record).to_string() == target.version
        && architecture_matches(&record.architecture, target.architecture)
        && record
            .resource_id
            .eq_ignore_ascii_case(target.resource_id.as_deref().unwrap_or_default())
}

fn architecture_matches(installed: &str, target: Architecture) -> bool {
    target == Architecture::Neutral
        || match target {
            Architecture::X64 => installed.eq_ignore_ascii_case("x64"),
            Architecture::Arm64 => installed.eq_ignore_ascii_case("arm64"),
            Architecture::Arm => installed.eq_ignore_ascii_case("arm"),
            Architecture::X86 => installed.eq_ignore_ascii_case("x86"),
            Architecture::Neutral => true,
        }
}

fn inventory_version(record: &PackageInventoryRecord) -> PackageVersion {
    PackageVersion::new(
        record.version[0],
        record.version[1],
        record.version[2],
        record.version[3],
    )
}

fn full_name_set(snapshot: &InventorySnapshot) -> HashSet<String> {
    snapshot
        .records
        .iter()
        .map(|record| record.package_full_name.clone())
        .collect()
}

fn role_rank(role: JobTargetRole) -> u8 {
    match role {
        JobTargetRole::Main => 0,
        JobTargetRole::Resource => 1,
        JobTargetRole::Dependency => 2,
    }
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn safe_api_error(context: &str, error: &AppErrorDto) -> String {
    format!(
        "{context}: code={:?} retry={:?} job_id={}",
        error.code,
        error.retry,
        error.job_id.as_deref().unwrap_or("none")
    )
}
