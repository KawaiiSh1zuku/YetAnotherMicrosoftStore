use std::{fs, io::Write, path::PathBuf};

use sha2::{Digest, Sha256};
use yet_another_microsoft_store_lib::{
    applicability::SelectionResult,
    broker_protocol::{PackageFileRequest, PackageIdentity},
    deployment_plan::{build_deployment_plan, DeploymentPlanError},
    domain::{Architecture, CacheEntry, CacheState, PackageFormat, PackageKind, PackageVersion},
    package_validation::{verify_package_request, verify_package_signature, ValidationError},
    resolver::{DependencyEdge, DependencyKind, PackageGraph, ResolvedPackage},
};

struct TestFiles {
    root: PathBuf,
}

impl TestFiles {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("yamstore-m5-plan-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create test root");
        Self { root }
    }

    fn cache_entry(&self, update_id: &str, sha256: &str, size: u64) -> CacheEntry {
        let path = self.root.join(format!("{update_id}.msix"));
        fs::write(&path, vec![0_u8; size as usize]).expect("write cached package");
        CacheEntry {
            cache_key: format!("cache-{update_id}"),
            job_id: Some("job".to_owned()),
            update_id: update_id.to_owned(),
            path: path.to_string_lossy().into_owned(),
            size,
            sha256: sha256.to_owned(),
            state: CacheState::Verified,
            last_accessed_at: 100,
        }
    }
}

impl Drop for TestFiles {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn package(update_id: &str, kind: PackageKind, version: PackageVersion) -> ResolvedPackage {
    ResolvedPackage {
        package_moniker: format!("{update_id}_{version}_x64__abc"),
        package_type: "appx".to_owned(),
        package_uri: None,
        file_name: Some(format!("{update_id}.msix")),
        file_size: Some(16),
        sha256: Some("ab".repeat(32)),
        update_id: update_id.to_owned(),
        identity_name: Some(format!("Example.{update_id}")),
        publisher: Some("CN=Example".to_owned()),
        version,
        architecture: Architecture::X64,
        resource_id: None,
        package_kind: kind,
        minimum_os_version: None,
        language: None,
        is_neutral: Some(true),
        content_id: Some(format!("content-{update_id}")),
        format: PackageFormat::Msix,
        prerequisites: Vec::new(),
        bundled_updates: Vec::new(),
    }
}

fn graph(packages: Vec<ResolvedPackage>, dependencies: Vec<DependencyEdge>) -> PackageGraph {
    PackageGraph {
        product_id: Some("product".to_owned()),
        market: Some("US".to_owned()),
        packages,
        dependencies,
        framework_requirements: Vec::new(),
    }
}

fn selected(packages: Vec<ResolvedPackage>) -> SelectionResult {
    SelectionResult {
        packages,
        decisions: Vec::new(),
    }
}

#[test]
fn verified_selection_becomes_a_dependency_first_m0_package_set() {
    let files = TestFiles::new();
    let main = package("main", PackageKind::Main, PackageVersion::new(3, 0, 0, 0));
    let framework = package(
        "framework",
        PackageKind::Framework,
        PackageVersion::new(2, 0, 0, 0),
    );
    let runtime = package(
        "runtime",
        PackageKind::Framework,
        PackageVersion::new(1, 0, 0, 0),
    );
    let graph = graph(
        vec![main.clone(), framework.clone(), runtime.clone()],
        vec![
            DependencyEdge {
                source_update_id: "main".to_owned(),
                target_update_id: "framework".to_owned(),
                kind: DependencyKind::Prerequisite,
            },
            DependencyEdge {
                source_update_id: "framework".to_owned(),
                target_update_id: "runtime".to_owned(),
                kind: DependencyKind::Prerequisite,
            },
        ],
    );
    let cache = vec![
        files.cache_entry("main", &"ab".repeat(32), 16),
        files.cache_entry("framework", &"ab".repeat(32), 16),
        files.cache_entry("runtime", &"ab".repeat(32), 16),
    ];

    let plan = build_deployment_plan(&graph, &selected(vec![main, runtime, framework]), &cache)
        .expect("verified graph should produce a deployment plan");

    assert_eq!(plan.product_id, "product");
    assert_eq!(plan.main_update_id, "main");
    assert_eq!(plan.main_version, PackageVersion::new(3, 0, 0, 0));
    assert_eq!(plan.content_id.as_deref(), Some("content-main"));
    assert!(plan.package_set.main.path.ends_with("main.msix"));
    assert_eq!(
        plan.package_set
            .dependencies
            .iter()
            .map(|request| {
                request
                    .expected_identity
                    .as_ref()
                    .expect("identity is required")
                    .name
                    .as_str()
            })
            .collect::<Vec<_>>(),
        vec!["Example.runtime", "Example.framework"]
    );
    assert_eq!(
        plan.package_set
            .main
            .expected_identity
            .as_ref()
            .expect("main identity")
            .version,
        [3, 0, 0, 0]
    );
}

#[test]
fn partial_missing_or_metadata_mismatched_cache_is_rejected() {
    let files = TestFiles::new();
    let main = package("main", PackageKind::Main, PackageVersion::new(3, 0, 0, 0));
    let graph = graph(vec![main.clone()], Vec::new());
    let selection = selected(vec![main]);

    assert_eq!(
        build_deployment_plan(&graph, &selection, &[]),
        Err(DeploymentPlanError::MissingVerifiedCache {
            update_id: "main".to_owned()
        })
    );

    let mut partial = files.cache_entry("main", &"ab".repeat(32), 16);
    partial.state = CacheState::Partial;
    assert_eq!(
        build_deployment_plan(&graph, &selection, &[partial]),
        Err(DeploymentPlanError::CacheNotVerified {
            update_id: "main".to_owned()
        })
    );

    let wrong_hash = files.cache_entry("main", &"cd".repeat(32), 16);
    assert_eq!(
        build_deployment_plan(&graph, &selection, &[wrong_hash]),
        Err(DeploymentPlanError::CacheMetadataMismatch {
            update_id: "main".to_owned()
        })
    );
}

#[test]
fn ambiguous_main_or_msixvc_selection_never_reaches_m0_deployment() {
    let files = TestFiles::new();
    let main = package("main", PackageKind::Main, PackageVersion::new(3, 0, 0, 0));
    let second = package("second", PackageKind::Main, PackageVersion::new(3, 0, 0, 0));
    let cache = vec![
        files.cache_entry("main", &"ab".repeat(32), 16),
        files.cache_entry("second", &"ab".repeat(32), 16),
    ];
    let ambiguous_graph = graph(vec![main.clone(), second.clone()], Vec::new());
    assert_eq!(
        build_deployment_plan(&ambiguous_graph, &selected(vec![main, second]), &cache),
        Err(DeploymentPlanError::AmbiguousMainPackage)
    );

    let mut msixvc = package("main", PackageKind::Main, PackageVersion::new(3, 0, 0, 0));
    msixvc.format = PackageFormat::Msixvc;
    let graph = graph(vec![msixvc.clone()], Vec::new());
    assert_eq!(
        build_deployment_plan(
            &graph,
            &selected(vec![msixvc]),
            &[files.cache_entry("main", &"ab".repeat(32), 16)],
        ),
        Err(DeploymentPlanError::UnsupportedPackageFormat {
            update_id: "main".to_owned()
        })
    );
}

#[cfg(windows)]
#[test]
fn unsigned_payload_fails_native_signature_preflight() {
    let files = TestFiles::new();
    let path = files.root.join("unsigned.msix");
    fs::write(&path, b"not-a-signed-package").expect("write unsigned payload");

    assert_eq!(
        verify_package_signature(&path),
        Err(ValidationError::SignatureInvalid)
    );
    assert_eq!(
        verify_package_signature(&path.canonicalize().expect("canonical package path")),
        Err(ValidationError::SignatureInvalid)
    );
}

#[cfg(windows)]
#[test]
fn valid_manifest_and_hash_reach_native_signature_preflight() {
    let files = TestFiles::new();
    let path = files.root.join("unsigned-manifest.msix");
    let file = fs::File::create(&path).expect("create unsigned package");
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file("AppxManifest.xml", zip::write::SimpleFileOptions::default())
        .expect("start manifest");
    archive
        .write_all(
            br#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"><Identity Name="Example.App" Publisher="CN=Example" Version="1.0.0.0" ProcessorArchitecture="x64" ResourceId="" /></Package>"#,
        )
        .expect("write manifest");
    archive.finish().expect("finish unsigned package");
    let bytes = fs::read(&path).expect("read unsigned package");
    let request = PackageFileRequest {
        path,
        sha256_hex: format!("{:x}", Sha256::digest(&bytes)),
        expected_identity: Some(PackageIdentity {
            name: "Example.App".to_owned(),
            publisher: "CN=Example".to_owned(),
            version: [1, 0, 0, 0],
            architecture: "x64".to_owned(),
            resource_id: String::new(),
        }),
    };

    assert_eq!(
        verify_package_request(&request),
        Err(ValidationError::SignatureInvalid)
    );
}

#[cfg(windows)]
#[test]
fn valid_bundle_manifest_and_hash_reach_native_signature_preflight() {
    let files = TestFiles::new();
    let path = files.root.join("unsigned-manifest.msixbundle");
    let file = fs::File::create(&path).expect("create unsigned bundle");
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file(
            "AppxMetadata/AppxBundleManifest.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .expect("start bundle manifest");
    archive
        .write_all(
            br#"<Bundle xmlns="http://schemas.microsoft.com/appx/2013/bundle"><Identity Name="Example.App" Publisher="CN=Example" Version="1.0.0.0" /></Bundle>"#,
        )
        .expect("write bundle manifest");
    archive.finish().expect("finish unsigned bundle");
    let bytes = fs::read(&path).expect("read unsigned bundle");
    let request = PackageFileRequest {
        path,
        sha256_hex: format!("{:x}", Sha256::digest(&bytes)),
        expected_identity: Some(PackageIdentity {
            name: "Example.App".to_owned(),
            publisher: "CN=Example".to_owned(),
            version: [1, 0, 0, 0],
            architecture: "neutral".to_owned(),
            resource_id: String::new(),
        }),
    };

    assert_eq!(
        verify_package_request(&request),
        Err(ValidationError::SignatureInvalid)
    );
}

#[cfg(windows)]
#[test]
#[ignore = "requires M5_SIGNED_PACKAGE_PATH pointing to a trusted signed MSIX/AppX payload"]
fn trusted_signed_package_passes_native_signature_preflight() {
    let path = std::env::var_os("M5_SIGNED_PACKAGE_PATH")
        .map(PathBuf::from)
        .expect("M5_SIGNED_PACKAGE_PATH is required");
    verify_package_signature(&path).expect("trusted package signature should verify");
}
