use std::{fs, path::PathBuf};

use yet_another_microsoft_store_lib::{
    app_runtime::{
        compatible_architectures, compatible_main_architectures, ProductionApiBackend, RuntimePaths,
    },
    applicability::HostCapabilities,
    deployment::DeploymentScope,
    domain::{
        Architecture, CacheEntry, CacheState, PackageFormat, PackageKind, PackageVersion,
        ProductRecord, ProxyCredentialPolicy, ProxyMode, ThemeMode,
    },
    error::ErrorCode,
    job_events::{JobControl, JobEvent},
    persistence::Persistence,
    resolver::{PackageGraph, ResolvedPackage},
    tauri_api::{ApiBackend, JobControlRequest, ListJobEventsRequest, StartJobSpec},
};

fn detail_package(
    update_id: &str,
    architecture: Architecture,
    format: PackageFormat,
    minimum_os_version: Option<PackageVersion>,
) -> ResolvedPackage {
    ResolvedPackage {
        package_moniker: format!("Example.App_1.0.0.0_{architecture:?}__publisher"),
        package_type: "appx".to_owned(),
        package_uri: Some("https://tlu.dl.delivery.mp.microsoft.com/app.appx".to_owned()),
        file_name: Some("app.appx".to_owned()),
        file_size: Some(1),
        sha256: Some("00".repeat(32)),
        update_id: update_id.to_owned(),
        identity_name: Some("Example.App".to_owned()),
        publisher: Some("CN=Example".to_owned()),
        version: PackageVersion::new(1, 0, 0, 0),
        architecture,
        resource_id: None,
        package_kind: PackageKind::Main,
        minimum_os_version,
        language: None,
        is_neutral: Some(true),
        content_id: None,
        format,
        prerequisites: Vec::new(),
        bundled_updates: Vec::new(),
    }
}

struct RuntimeFixture {
    root: PathBuf,
    paths: RuntimePaths,
}

impl RuntimeFixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("yamstore-m6-runtime-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("runtime root");
        let paths = RuntimePaths::new(root.join("state.sqlite3"), root.join("cache"))
            .expect("absolute isolated runtime paths");
        Self { root, paths }
    }

    fn backend(&self) -> ProductionApiBackend {
        ProductionApiBackend::new(self.paths.clone())
    }

    fn persistence(&self) -> Persistence {
        Persistence::open(self.paths.database_path()).expect("runtime database")
    }

    fn seed_product(&self) {
        self.persistence()
            .upsert_product(&ProductRecord {
                product_id: "9NBLGGH4NNS1".to_owned(),
                package_family_name: Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe".to_owned()),
                app_name: Some("Windows Terminal".to_owned()),
                publisher: Some("Microsoft Corporation".to_owned()),
                market: "US".to_owned(),
                languages: vec!["en-US".to_owned()],
                updated_at: 10,
            })
            .expect("seed product");
    }
}

impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn start_spec() -> StartJobSpec {
    StartJobSpec {
        product_id: "9NBLGGH4NNS1".to_owned(),
        market: "US".to_owned(),
        language: "en-US".to_owned(),
        scope: DeploymentScope::CurrentUser,
    }
}

#[tokio::test]
async fn settings_round_trip_preserves_the_internal_cache_path() {
    let fixture = RuntimeFixture::new();
    let backend = fixture.backend();
    let mut settings = backend.get_settings().await.expect("default settings");
    settings.region = "CN".to_owned();
    settings.market = "US".to_owned();
    settings.preferred_architectures = vec![Architecture::X64];
    settings.preferred_languages = vec!["zh-CN".to_owned(), "en-US".to_owned()];
    settings.proxy_mode = ProxyMode::Socks5;
    settings.proxy_host = Some("127.0.0.1".to_owned());
    settings.proxy_port = Some(7890);
    settings.proxy_credentials = ProxyCredentialPolicy::PromptEveryTime;
    settings.theme = ThemeMode::Dark;
    settings.diagnostics_enabled = true;

    let updated = backend
        .update_settings(settings.clone())
        .await
        .expect("update settings");
    assert_eq!(updated, settings);
    assert_eq!(
        backend.get_settings().await.expect("stored settings"),
        settings
    );

    let stored = fixture
        .persistence()
        .settings()
        .expect("settings query")
        .expect("settings row");
    assert_eq!(
        PathBuf::from(stored.cache_directory),
        fixture.paths.cache_root()
    );
}

#[tokio::test]
async fn start_list_and_event_replay_use_the_persisted_product_title() {
    let fixture = RuntimeFixture::new();
    fixture.seed_product();
    let backend = fixture.backend();
    let wake_before = backend.worker_wake().generation();

    let started = backend
        .start_install(start_spec())
        .await
        .expect("start install");
    assert_eq!(started.title.as_deref(), Some("Windows Terminal"));
    assert_eq!(started.snapshot.sequence, 1);
    assert!(backend.worker_wake().generation() > wake_before);

    let listed = backend.list_jobs().await.expect("list jobs");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title.as_deref(), Some("Windows Terminal"));

    let events = backend
        .list_job_events(ListJobEventsRequest {
            after_cursor: None,
            limit: 10,
        })
        .await
        .expect("list events");
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0].event, JobEvent::Created { .. }));
    assert_eq!(events[0].snapshot.sequence, 1);
}

#[tokio::test]
async fn repeated_job_control_request_is_deduplicated_durably() {
    let fixture = RuntimeFixture::new();
    fixture.seed_product();
    let backend = fixture.backend();
    let started = backend
        .start_install(start_spec())
        .await
        .expect("start install");
    let request = JobControlRequest {
        job_id: started.snapshot.job.job_id.clone(),
        command_id: "command-1".to_owned(),
        expected_sequence: started.snapshot.sequence,
        control: JobControl::Cancel,
    };

    backend
        .request_job_control(request.clone())
        .await
        .expect("first request");
    backend
        .request_job_control(request)
        .await
        .expect("deduplicated request");

    let pending = fixture
        .persistence()
        .pending_job_commands(&started.snapshot.job.job_id)
        .expect("pending commands");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].command_id, "command-1");
}

#[tokio::test]
async fn runtime_paths_and_cache_clear_fail_closed_on_unsafe_paths() {
    assert!(RuntimePaths::new("relative.sqlite3", r"C:\safe\cache").is_err());
    assert!(RuntimePaths::new(r"C:\safe\cache\state.sqlite3", r"C:\safe\cache").is_err());
    assert!(RuntimePaths::new(r"C:\safe2\..\safe\cache\state.sqlite3", r"C:\safe\cache").is_err());

    let fixture = RuntimeFixture::new();
    let backend = fixture.backend();
    let external = fixture.root.join("outside.bin");
    fs::write(&external, b"do not remove").expect("external fixture");
    fixture
        .persistence()
        .upsert_cache_entry(&CacheEntry {
            cache_key: "unsafe-entry".to_owned(),
            job_id: None,
            update_id: "update-1".to_owned(),
            path: external.to_string_lossy().into_owned(),
            size: 13,
            sha256: "ab".repeat(32),
            state: CacheState::Verified,
            last_accessed_at: 1,
        })
        .expect("unsafe metadata fixture");

    assert!(backend.clear_cache().await.is_err());
    assert_eq!(
        fs::read(&external).expect("external survives"),
        b"do not remove"
    );
    assert!(fixture
        .persistence()
        .cache_entry("unsafe-entry")
        .expect("cache metadata")
        .is_some());
}

#[tokio::test]
async fn cache_clear_preserves_payloads_referenced_by_a_resumable_job() {
    let fixture = RuntimeFixture::new();
    fixture.seed_product();
    let backend = fixture.backend();
    let started = backend
        .start_install(start_spec())
        .await
        .expect("start install");
    let verified = fixture.paths.cache_root().join("verified");
    fs::create_dir_all(&verified).expect("verified root");
    let payload = verified.join("active.msix");
    fs::write(&payload, b"active payload").expect("active payload");
    fixture
        .persistence()
        .upsert_cache_entry(&CacheEntry {
            cache_key: "active-entry".to_owned(),
            job_id: Some(started.snapshot.job.job_id),
            update_id: "update-active".to_owned(),
            path: payload.to_string_lossy().into_owned(),
            size: 14,
            sha256: "cd".repeat(32),
            state: CacheState::Verified,
            last_accessed_at: 1,
        })
        .expect("active cache metadata");

    assert!(backend.clear_cache().await.is_err());
    assert_eq!(
        fs::read(&payload).expect("active payload survives"),
        b"active payload"
    );
    assert!(fixture
        .persistence()
        .cache_entry("active-entry")
        .expect("active cache metadata")
        .is_some());
}

#[test]
fn production_worker_factory_uses_verified_platform_capabilities() {
    let fixture = RuntimeFixture::new();
    let backend = fixture.backend();
    #[cfg(windows)]
    assert!(backend.create_worker().is_ok());
    #[cfg(not(windows))]
    assert!(backend.create_worker().is_err());
}

#[test]
fn compatibility_matrix_includes_x86_without_assuming_x64_emulation_on_arm64() {
    assert_eq!(
        compatible_architectures(Architecture::X64),
        vec![Architecture::X64, Architecture::X86, Architecture::Neutral]
    );
    assert_eq!(
        compatible_architectures(Architecture::Arm64),
        vec![
            Architecture::Arm64,
            Architecture::X86,
            Architecture::Neutral
        ]
    );
    assert!(!compatible_architectures(Architecture::Arm64).contains(&Architecture::X64));
}

#[test]
fn detail_architectures_reuse_os_format_and_architecture_applicability() {
    let host = HostCapabilities {
        os_version: PackageVersion::new(10, 0, 19045, 0),
        native_architecture: Architecture::X64,
        compatible_architectures: vec![Architecture::X64, Architecture::X86, Architecture::Neutral],
        supported_formats: vec![PackageFormat::Appx],
    };
    let graph = PackageGraph {
        product_id: Some("product".to_owned()),
        market: Some("US".to_owned()),
        packages: vec![
            detail_package(
                "too-new",
                Architecture::X64,
                PackageFormat::Appx,
                Some(PackageVersion::new(10, 0, 22621, 0)),
            ),
            detail_package("compatible", Architecture::X86, PackageFormat::Appx, None),
        ],
        dependencies: Vec::new(),
        framework_requirements: Vec::new(),
    };

    assert_eq!(
        compatible_main_architectures(&graph, &host).expect("one compatible package"),
        vec![Architecture::X86]
    );

    let unsupported = PackageGraph {
        packages: vec![detail_package(
            "unsupported-format",
            Architecture::X64,
            PackageFormat::Msix,
            None,
        )],
        ..graph
    };
    assert_eq!(
        compatible_main_architectures(&unsupported, &host)
            .expect_err("unsupported formats are not shown")
            .code,
        ErrorCode::NoCompatiblePackage
    );
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires live Store catalog access for PFN association"]
async fn update_scan_associates_machine_packages_by_pfn() {
    let fixture = RuntimeFixture::new();
    let updates = fixture
        .backend()
        .scan_updates()
        .await
        .expect("unassociated packages are not update candidates");
    assert!(updates.candidates.is_empty());
    assert!(updates.scanned_main_packages > 0);
    assert!(!updates.skipped.is_empty());
    assert!(!updates.complete);
}
