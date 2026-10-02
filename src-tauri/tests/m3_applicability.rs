use yet_another_microsoft_store_lib::{
    applicability::{
        select_packages, ApplicabilityError, DecisionReason, HostCapabilities, InstalledPackage,
        SelectionMode, SelectionPreferences,
    },
    domain::{Architecture, PackageFormat, PackageKind, PackageVersion},
    error::{AppErrorDto, ErrorCode, RetryAdvice},
    resolver::{DependencyEdge, DependencyKind, PackageGraph, ResolvedPackage},
};

fn package(
    update_id: &str,
    version: PackageVersion,
    architecture: Architecture,
    format: PackageFormat,
    kind: PackageKind,
) -> ResolvedPackage {
    ResolvedPackage {
        package_moniker: format!("Example.App_{version}_x64__abc"),
        package_type: "appx".to_owned(),
        package_uri: None,
        file_name: None,
        file_size: Some(4096),
        digest: None,
        update_id: update_id.to_owned(),
        identity_name: Some("Example.App".to_owned()),
        publisher: Some("CN=Example".to_owned()),
        version,
        architecture,
        resource_id: None,
        package_kind: kind,
        minimum_os_version: Some(PackageVersion::new(10, 0, 17763, 0)),
        language: None,
        is_neutral: Some(true),
        content_id: Some(format!("content-{update_id}")),
        format,
        prerequisites: Vec::new(),
        bundled_updates: Vec::new(),
    }
}

fn graph(packages: Vec<ResolvedPackage>) -> PackageGraph {
    PackageGraph {
        product_id: Some("9WZDNCRFJ3Q8".to_owned()),
        market: Some("CN".to_owned()),
        packages,
        dependencies: Vec::new(),
    }
}

fn x64_host() -> HostCapabilities {
    HostCapabilities {
        os_version: PackageVersion::new(10, 0, 19045, 0),
        native_architecture: Architecture::X64,
        compatible_architectures: vec![Architecture::X64, Architecture::X86, Architecture::Neutral],
        supported_formats: vec![
            PackageFormat::Msix,
            PackageFormat::Appx,
            PackageFormat::MsixBundle,
            PackageFormat::AppxBundle,
        ],
    }
}

fn preferences(mode: SelectionMode) -> SelectionPreferences {
    SelectionPreferences {
        market: "CN".to_owned(),
        preferred_architectures: vec![Architecture::X64, Architecture::X86],
        preferred_languages: vec!["zh-Hant-TW".to_owned(), "en-US".to_owned()],
        mode,
    }
}

#[test]
fn preferred_compatible_architecture_wins_at_the_same_version() {
    let packages = graph(vec![
        package(
            "x86",
            PackageVersion::new(2, 0, 0, 0),
            Architecture::X86,
            PackageFormat::Msix,
            PackageKind::Main,
        ),
        package(
            "x64",
            PackageVersion::new(2, 0, 0, 0),
            Architecture::X64,
            PackageFormat::Msix,
            PackageKind::Main,
        ),
    ]);

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &[],
    )
    .expect("compatible package should be selected");

    assert_eq!(result.packages[0].update_id, "x64");
}

#[test]
fn arm64_compatibility_uses_host_capabilities_instead_of_a_hardcoded_matrix() {
    let packages = graph(vec![package(
        "x64",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Main,
    )]);
    let mut host = x64_host();
    host.native_architecture = Architecture::Arm64;
    host.compatible_architectures = vec![Architecture::Arm64, Architecture::Neutral];

    let rejected = select_packages(&packages, &host, &preferences(SelectionMode::Install), &[]);
    assert!(matches!(
        rejected,
        Err(ApplicabilityError::NoCompatiblePackage)
    ));

    host.compatible_architectures.push(Architecture::X64);
    let selected = select_packages(&packages, &host, &preferences(SelectionMode::Install), &[])
        .expect("declared x64 emulation should make the package compatible");
    assert_eq!(selected.packages[0].update_id, "x64");
}

#[test]
fn higher_version_cannot_bypass_minimum_os_or_format_gates() {
    let mut too_new = package(
        "too-new",
        PackageVersion::new(9, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Main,
    );
    too_new.minimum_os_version = Some(PackageVersion::new(10, 0, 22000, 0));
    let unsupported = package(
        "msixvc",
        PackageVersion::new(8, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msixvc,
        PackageKind::Main,
    );
    let compatible = package(
        "compatible",
        PackageVersion::new(7, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Main,
    );

    let result = select_packages(
        &graph(vec![too_new, unsupported, compatible]),
        &x64_host(),
        &preferences(SelectionMode::Install),
        &[],
    )
    .expect("one package is compatible");

    assert_eq!(
        result
            .packages
            .iter()
            .map(|package| package.update_id.as_str())
            .collect::<Vec<_>>(),
        vec!["compatible"]
    );
    assert!(result.decisions.iter().any(|decision| {
        decision.update_id == "too-new" && decision.reason == DecisionReason::MinimumOsNotMet
    }));
    assert!(result.decisions.iter().any(|decision| {
        decision.update_id == "msixvc" && decision.reason == DecisionReason::UnsupportedFormat
    }));
}

#[test]
fn bcp47_fallback_selects_best_resource_and_neutral_resource() {
    let root = package(
        "bundle",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::Neutral,
        PackageFormat::MsixBundle,
        PackageKind::Main,
    );
    let mut hant = package(
        "resource-hant",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::Neutral,
        PackageFormat::Msix,
        PackageKind::Resource,
    );
    hant.resource_id = Some("zh-hant".to_owned());
    hant.language = Some("zh-Hant".to_owned());
    hant.is_neutral = Some(false);
    let mut hans = hant.clone();
    hans.update_id = "resource-hans".to_owned();
    hans.resource_id = Some("zh-hans".to_owned());
    hans.language = Some("zh-Hans".to_owned());
    let mut neutral = hant.clone();
    neutral.update_id = "resource-neutral".to_owned();
    neutral.resource_id = None;
    neutral.language = None;
    neutral.is_neutral = Some(true);
    let mut packages = graph(vec![root, hant, hans, neutral]);
    packages.dependencies = vec![
        DependencyEdge {
            source_update_id: "bundle".to_owned(),
            target_update_id: "resource-hant".to_owned(),
            kind: DependencyKind::Bundled,
        },
        DependencyEdge {
            source_update_id: "bundle".to_owned(),
            target_update_id: "resource-hans".to_owned(),
            kind: DependencyKind::Bundled,
        },
        DependencyEdge {
            source_update_id: "bundle".to_owned(),
            target_update_id: "resource-neutral".to_owned(),
            kind: DependencyKind::Bundled,
        },
    ];

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &[],
    )
    .expect("bundle resources should resolve");
    let selected = result
        .packages
        .iter()
        .map(|package| package.update_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        selected,
        vec!["bundle", "resource-hant", "resource-neutral"]
    );
}

#[test]
fn installed_framework_satisfies_prerequisite_without_reselection() {
    let root = package(
        "main",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Main,
    );
    let mut framework = package(
        "framework",
        PackageVersion::new(1, 5, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Framework,
    );
    framework.identity_name = Some("Microsoft.Framework".to_owned());
    let mut packages = graph(vec![root, framework]);
    packages.dependencies.push(DependencyEdge {
        source_update_id: "main".to_owned(),
        target_update_id: "framework".to_owned(),
        kind: DependencyKind::Prerequisite,
    });
    let installed = [InstalledPackage {
        identity_name: "Microsoft.Framework".to_owned(),
        publisher: Some("CN=Example".to_owned()),
        version: PackageVersion::new(1, 6, 0, 0),
        architecture: Architecture::X64,
    }];

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &installed,
    )
    .expect("installed framework should satisfy dependency");

    assert_eq!(result.packages.len(), 1);
    assert!(result.decisions.iter().any(|decision| {
        decision.update_id == "framework" && decision.reason == DecisionReason::SatisfiedByInstalled
    }));
}

#[test]
fn strict_update_blocks_catalog_downgrade_but_repair_allows_equal_version() {
    let candidate = package(
        "main",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Main,
    );
    let installed_ahead = [InstalledPackage {
        identity_name: "Example.App".to_owned(),
        publisher: Some("CN=Example".to_owned()),
        version: PackageVersion::new(3, 0, 0, 0),
        architecture: Architecture::X64,
    }];

    let downgrade = select_packages(
        &graph(vec![candidate.clone()]),
        &x64_host(),
        &preferences(SelectionMode::Update),
        &installed_ahead,
    );
    assert!(matches!(
        downgrade,
        Err(ApplicabilityError::VersionAheadOfCatalog { .. })
    ));

    let installed_equal = [InstalledPackage {
        identity_name: "Example.App".to_owned(),
        publisher: Some("CN=Example".to_owned()),
        version: PackageVersion::new(2, 0, 0, 0),
        architecture: Architecture::X64,
    }];
    let repaired = select_packages(
        &graph(vec![candidate]),
        &x64_host(),
        &preferences(SelectionMode::Repair),
        &installed_equal,
    )
    .expect("explicit repair should allow the installed version");
    assert_eq!(repaired.packages[0].update_id, "main");
}

#[test]
fn market_mismatch_is_reported_before_package_selection() {
    let mut packages = graph(vec![package(
        "main",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Main,
    )]);
    packages.market = Some("US".to_owned());

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &[],
    );

    assert!(matches!(result, Err(ApplicabilityError::MarketMismatch)));
}

#[test]
fn missing_bundled_update_is_a_dependency_error() {
    let mut packages = graph(vec![package(
        "bundle",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::Neutral,
        PackageFormat::MsixBundle,
        PackageKind::Main,
    )]);
    packages.dependencies.push(DependencyEdge {
        source_update_id: "bundle".to_owned(),
        target_update_id: "missing-resource".to_owned(),
        kind: DependencyKind::Bundled,
    });

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &[],
    );

    assert!(matches!(
        result,
        Err(ApplicabilityError::DependencyUnresolved { update_id })
            if update_id == "missing-resource"
    ));
}

#[test]
fn non_language_resource_is_not_filtered_by_language_preferences() {
    let root = package(
        "bundle",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::Neutral,
        PackageFormat::MsixBundle,
        PackageKind::Main,
    );
    let mut scale = package(
        "scale-200",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::Neutral,
        PackageFormat::Msix,
        PackageKind::Resource,
    );
    scale.resource_id = Some("scale-200".to_owned());
    scale.language = None;
    scale.is_neutral = Some(false);
    let mut packages = graph(vec![root, scale]);
    packages.dependencies.push(DependencyEdge {
        source_update_id: "bundle".to_owned(),
        target_update_id: "scale-200".to_owned(),
        kind: DependencyKind::Bundled,
    });

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &[],
    )
    .expect("scale resource should remain selected");

    assert_eq!(result.packages[1].update_id, "scale-200");
}

#[test]
fn applicability_errors_map_to_stable_frontend_codes() {
    let cases = [
        (
            ApplicabilityError::MarketMismatch,
            ErrorCode::MarketUnavailable,
            RetryAdvice::Never,
        ),
        (
            ApplicabilityError::NoCompatiblePackage,
            ErrorCode::NoCompatiblePackage,
            RetryAdvice::Never,
        ),
        (
            ApplicabilityError::DependencyUnresolved {
                update_id: "dependency".to_owned(),
            },
            ErrorCode::DependencyUnresolved,
            RetryAdvice::ReResolve,
        ),
        (
            ApplicabilityError::DependencyCycle {
                update_id: "dependency".to_owned(),
            },
            ErrorCode::DependencyUnresolved,
            RetryAdvice::ReResolve,
        ),
        (
            ApplicabilityError::PackageNotInstalled {
                identity_name: "Example.App".to_owned(),
            },
            ErrorCode::PackageNotInstalled,
            RetryAdvice::ReconcileInventory,
        ),
        (
            ApplicabilityError::VersionAheadOfCatalog {
                identity_name: "Example.App".to_owned(),
                installed: PackageVersion::new(3, 0, 0, 0),
                catalog: PackageVersion::new(2, 0, 0, 0),
            },
            ErrorCode::VersionAheadOfCatalog,
            RetryAdvice::Never,
        ),
    ];

    for (error, code, retry) in cases {
        let dto = AppErrorDto::from(&error);
        assert_eq!((dto.code, dto.retry), (code, retry));
        assert!(dto.details.is_empty());
    }
}

#[test]
fn prerequisite_cycles_return_a_controlled_error() {
    let root = package(
        "main",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Main,
    );
    let framework = package(
        "framework",
        PackageVersion::new(1, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Framework,
    );
    let mut packages = graph(vec![root, framework]);
    packages.dependencies = vec![
        DependencyEdge {
            source_update_id: "main".to_owned(),
            target_update_id: "framework".to_owned(),
            kind: DependencyKind::Prerequisite,
        },
        DependencyEdge {
            source_update_id: "framework".to_owned(),
            target_update_id: "main".to_owned(),
            kind: DependencyKind::Prerequisite,
        },
    ];

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &[],
    );

    assert!(matches!(
        result,
        Err(ApplicabilityError::DependencyCycle { update_id }) if update_id == "main"
    ));
}

#[test]
fn bundled_packages_include_their_transitive_prerequisites() {
    let root = package(
        "bundle",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::Neutral,
        PackageFormat::MsixBundle,
        PackageKind::Main,
    );
    let child = package(
        "child",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Main,
    );
    let framework = package(
        "framework",
        PackageVersion::new(1, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Framework,
    );
    let mut packages = graph(vec![root, child, framework]);
    packages.dependencies = vec![
        DependencyEdge {
            source_update_id: "bundle".to_owned(),
            target_update_id: "child".to_owned(),
            kind: DependencyKind::Bundled,
        },
        DependencyEdge {
            source_update_id: "child".to_owned(),
            target_update_id: "framework".to_owned(),
            kind: DependencyKind::Prerequisite,
        },
    ];

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &[],
    )
    .expect("bundled dependency graph should be traversed");
    let selected = result
        .packages
        .iter()
        .map(|package| package.update_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(selected, vec!["bundle", "child", "framework"]);
}

#[test]
fn installed_dependency_must_match_required_architecture() {
    let root = package(
        "main",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Main,
    );
    let mut framework = package(
        "framework-x64",
        PackageVersion::new(1, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Framework,
    );
    framework.identity_name = Some("Microsoft.Framework".to_owned());
    let mut packages = graph(vec![root, framework]);
    packages.dependencies.push(DependencyEdge {
        source_update_id: "main".to_owned(),
        target_update_id: "framework-x64".to_owned(),
        kind: DependencyKind::Prerequisite,
    });
    let installed = [InstalledPackage {
        identity_name: "Microsoft.Framework".to_owned(),
        publisher: Some("CN=Example".to_owned()),
        version: PackageVersion::new(2, 0, 0, 0),
        architecture: Architecture::Arm64,
    }];

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &installed,
    )
    .expect("x64 dependency should remain selected");

    assert!(result
        .packages
        .iter()
        .any(|package| package.update_id == "framework-x64"));
}

#[test]
fn language_fallback_preserves_script_specificity_independent_of_response_order() {
    let root = package(
        "bundle",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::Neutral,
        PackageFormat::MsixBundle,
        PackageKind::Main,
    );
    let mut hans = package(
        "resource-hans",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::Neutral,
        PackageFormat::Msix,
        PackageKind::Resource,
    );
    hans.language = Some("zh-Hans".to_owned());
    hans.is_neutral = Some(false);
    let mut hant = hans.clone();
    hant.update_id = "resource-hant".to_owned();
    hant.language = Some("zh-Hant".to_owned());
    let mut packages = graph(vec![root, hans, hant]);
    packages.dependencies = vec![
        DependencyEdge {
            source_update_id: "bundle".to_owned(),
            target_update_id: "resource-hans".to_owned(),
            kind: DependencyKind::Bundled,
        },
        DependencyEdge {
            source_update_id: "bundle".to_owned(),
            target_update_id: "resource-hant".to_owned(),
            kind: DependencyKind::Bundled,
        },
    ];

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &[],
    )
    .expect("script-specific fallback should select a resource");

    assert!(result
        .packages
        .iter()
        .any(|package| package.update_id == "resource-hant"));
    assert!(!result
        .packages
        .iter()
        .any(|package| package.update_id == "resource-hans"));
}

#[test]
fn language_resources_are_selected_once_per_identity_group() {
    let root = package(
        "bundle",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::Neutral,
        PackageFormat::MsixBundle,
        PackageKind::Main,
    );
    let mut first = package(
        "resource-first",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::Neutral,
        PackageFormat::Msix,
        PackageKind::Resource,
    );
    first.identity_name = Some("Example.First.Resources".to_owned());
    first.language = Some("zh-Hant".to_owned());
    first.is_neutral = Some(false);
    let mut second = first.clone();
    second.update_id = "resource-second".to_owned();
    second.identity_name = Some("Example.Second.Resources".to_owned());
    let mut packages = graph(vec![root, first, second]);
    packages.dependencies = vec![
        DependencyEdge {
            source_update_id: "bundle".to_owned(),
            target_update_id: "resource-first".to_owned(),
            kind: DependencyKind::Bundled,
        },
        DependencyEdge {
            source_update_id: "bundle".to_owned(),
            target_update_id: "resource-second".to_owned(),
            kind: DependencyKind::Bundled,
        },
    ];

    let result = select_packages(
        &packages,
        &x64_host(),
        &preferences(SelectionMode::Install),
        &[],
    )
    .expect("each language resource identity should be retained");
    let selected = result
        .packages
        .iter()
        .map(|package| package.update_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        selected,
        vec!["bundle", "resource-first", "resource-second"]
    );
}

#[test]
fn strict_update_requires_a_matching_installed_package() {
    let candidate = package(
        "main",
        PackageVersion::new(2, 0, 0, 0),
        Architecture::X64,
        PackageFormat::Msix,
        PackageKind::Main,
    );

    let result = select_packages(
        &graph(vec![candidate]),
        &x64_host(),
        &preferences(SelectionMode::Update),
        &[],
    );

    assert!(matches!(
        result,
        Err(ApplicabilityError::PackageNotInstalled { identity_name })
            if identity_name == "Example.App"
    ));
}
