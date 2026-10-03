#![cfg(windows)]

use yet_another_microsoft_store_lib::applicability::{
    select_packages, HostCapabilities, SelectionMode, SelectionPreferences,
};
use yet_another_microsoft_store_lib::catalog::{
    normalize_catalog_icon_url, CatalogError, CatalogProvider, DeviceFamily, StoreLibCatalogAdapter,
};
use yet_another_microsoft_store_lib::domain::{
    Architecture, PackageFormat, PackageKind, PackageVersion,
};
use yet_another_microsoft_store_lib::resolver::{
    normalize_delivery_url, DependencyKind, ResolverError, StoreLibResolverAdapter,
};

#[test]
fn fe3_delivery_urls_preserve_http_only_for_allowlisted_microsoft_hosts() {
    assert_eq!(
        normalize_delivery_url(Some(
            "http://tlu.dl.delivery.mp.microsoft.com/content/app.msix?token=redacted".to_owned()
        ))
        .expect("allowlisted delivery URL"),
        Some("http://tlu.dl.delivery.mp.microsoft.com/content/app.msix?token=redacted".to_owned())
    );
    assert!(matches!(
        normalize_delivery_url(Some("http://download.invalid/app.msix".to_owned())),
        Err(ResolverError::InvalidPackageUrl)
    ));
    assert!(matches!(
        normalize_delivery_url(Some(
            "https://user:secret@tlu.dl.delivery.mp.microsoft.com/app.msix".to_owned()
        )),
        Err(ResolverError::InvalidPackageUrl)
    ));
}

#[tokio::test]
async fn fe3_bundle_container_remains_a_main_package_when_install_data_is_not_main() {
    let fixture = include_str!("fixtures/fe3-applicability.xml")
        .replace("_x64_zh-cn_abc", "_neutral_~_abc")
        .replace("IsAppxBundle=\"false\"", "IsAppxBundle=\"true\"")
        .replace("resource.msix", "application.appxbundle");

    let graph = StoreLibResolverAdapter::parse_fixture(&fixture)
        .await
        .expect("bundle fixture should parse");

    assert_eq!(graph.packages[0].format, PackageFormat::AppxBundle);
    assert_eq!(graph.packages[0].package_kind, PackageKind::Main);
    assert_eq!(graph.packages[0].resource_id, None);
    assert_eq!(graph.packages[0].is_neutral, Some(true));
}

#[test]
fn dcat_fixtures_normalize_search_and_product_identity() {
    let products =
        StoreLibCatalogAdapter::parse_search_fixture(include_str!("fixtures/dcat-search.json"))
            .expect("search fixture should parse");

    assert_eq!(products.len(), 1);
    assert_eq!(products[0].product_id, "9WZDNCRFJ3TJ");
    assert_eq!(
        products[0].package_family_name.as_deref(),
        Some("Contoso.Notes_abc")
    );
    assert_eq!(products[0].app_name.as_deref(), Some("Contoso Notes"));

    let product =
        StoreLibCatalogAdapter::parse_product_fixture(include_str!("fixtures/dcat-product.json"))
            .expect("product fixture should parse");

    assert_eq!(product.product_id, "9WZDNCRFJ3TJ");
    assert_eq!(
        product.package_family_name.as_deref(),
        Some("Contoso.Notes_abc")
    );
    assert_eq!(product.package_formats, vec!["msix", "appx"]);
    assert_eq!(
        product.framework_dependencies,
        vec!["Microsoft.VCLibs.140.00"]
    );
}

#[test]
fn catalog_icons_use_an_exact_https_allowlist() {
    assert_eq!(
        normalize_catalog_icon_url("//store-images.s-microsoft.com/image.png")
            .expect("protocol-relative Store image should be upgraded"),
        "https://store-images.s-microsoft.com/image.png"
    );
    for invalid in [
        "http://store-images.s-microsoft.com/image.png",
        "https://user:secret@store-images.s-microsoft.com/image.png",
        "https://store-images.s-microsoft.com:444/image.png",
        "https://store-images.s-microsoft.com/image.png#fragment",
        "https://example.invalid/image.png",
    ] {
        assert!(normalize_catalog_icon_url(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn dcat_fixture_rejects_invalid_package_url() {
    let result = StoreLibCatalogAdapter::parse_product_fixture(include_str!(
        "fixtures/dcat-invalid-url.json"
    ));

    assert!(matches!(result, Err(CatalogError::InvalidUrl { .. })));
}

#[test]
fn catalog_provider_is_project_owned_and_uses_own_device_family() {
    fn assert_provider<T: CatalogProvider>() {}

    assert_provider::<StoreLibCatalogAdapter>();
    assert_eq!(
        DeviceFamily::Desktop.as_platform_dependency_name(),
        "Windows.Desktop"
    );
}

#[tokio::test]
async fn fe3_fixture_normalizes_packages_and_dependency_edges() {
    let graph =
        StoreLibResolverAdapter::parse_fixture(include_str!("fixtures/fe3-sync-updates.xml"))
            .await
            .expect("FE3 fixture should parse");

    assert_eq!(graph.packages.len(), 2);
    let app = graph
        .packages
        .iter()
        .find(|package| package.package_moniker == "Contoso.Notes_1.0.0.0_x64__abc")
        .expect("app package should be present");
    assert_eq!(app.package_type, "appx");
    assert_eq!(app.update_id, "app-update");
    assert_eq!(app.prerequisites, vec!["framework-category"]);
    assert_eq!(app.bundled_updates, vec!["resource-update"]);

    assert!(!graph.dependencies.iter().any(|edge| {
        edge.source_update_id == "app-update"
            && edge.target_update_id == "framework-category"
            && edge.kind == DependencyKind::Prerequisite
    }));
    assert!(graph.dependencies.iter().any(|edge| {
        edge.source_update_id == "app-update"
            && edge.target_update_id == "resource-update"
            && edge.kind == DependencyKind::Bundled
    }));
}

#[tokio::test]
async fn fe3_category_prerequisites_do_not_fail_package_selection() {
    let graph =
        StoreLibResolverAdapter::parse_fixture(include_str!("fixtures/fe3-sync-updates.xml"))
            .await
            .expect("FE3 fixture should parse");
    let host = HostCapabilities {
        os_version: PackageVersion::new(10, 0, 19045, 0),
        native_architecture: Architecture::X64,
        compatible_architectures: vec![Architecture::X64, Architecture::Neutral],
        supported_formats: vec![PackageFormat::Appx],
    };
    let preferences = SelectionPreferences {
        market: "US".to_owned(),
        preferred_architectures: vec![Architecture::X64, Architecture::Neutral],
        preferred_languages: vec!["en-US".to_owned()],
        mode: SelectionMode::Install,
    };

    let selected = select_packages(&graph, &host, &preferences, &[])
        .expect("WU category IDs are constraints, not package update IDs");

    assert_eq!(selected.packages[0].update_id, "app-update");
    assert!(selected
        .packages
        .iter()
        .any(|package| package.update_id == "resource-update"));
}

#[tokio::test]
async fn fe3_parser_rejects_malformed_or_missing_package_fields() {
    let malformed = StoreLibResolverAdapter::parse_fixture("<not-xml").await;
    assert!(matches!(malformed, Err(ResolverError::MalformedFixture(_))));

    let missing =
        StoreLibResolverAdapter::parse_fixture(include_str!("fixtures/fe3-missing-moniker.xml"))
            .await;
    assert!(matches!(
        missing,
        Err(ResolverError::MissingField("packageMoniker"))
    ));
}

#[tokio::test]
async fn fe3_fixture_maps_applicability_fields_without_leaking_vendor_types() {
    let graph =
        StoreLibResolverAdapter::parse_fixture(include_str!("fixtures/fe3-applicability.xml"))
            .await
            .expect("M3 FE3 fixture should parse");
    let package = graph.packages.first().expect("fixture package");

    assert_eq!(package.identity_name.as_deref(), Some("Contoso.Notes"));
    assert_eq!(package.publisher.as_deref(), Some("CN=Contoso"));
    assert_eq!(package.version, PackageVersion::new(2, 4, 6, 8));
    assert_eq!(package.architecture, Architecture::X64);
    assert_eq!(package.resource_id.as_deref(), Some("zh-cn"));
    assert_eq!(package.package_kind, PackageKind::Resource);
    assert_eq!(
        package.minimum_os_version,
        Some(PackageVersion::new(10, 0, 19045, 0))
    );
    assert_eq!(package.language.as_deref(), Some("zh-CN"));
    assert_eq!(package.is_neutral, Some(false));
    assert_eq!(package.content_id.as_deref(), Some("content-123"));
    assert_eq!(package.format, PackageFormat::Msix);
    let expected_sha256 = "00".repeat(32);
    assert_eq!(package.sha256.as_deref(), Some(expected_sha256.as_str()));
}

#[tokio::test]
async fn fe3_architecture_package_without_resource_identity_remains_a_main_package() {
    let fixture =
        include_str!("fixtures/fe3-applicability.xml").replace("_x64_zh-cn_abc", "_x64__abc");

    let graph = StoreLibResolverAdapter::parse_fixture(&fixture)
        .await
        .expect("architecture-specific application package should parse");
    let package = graph.packages.first().expect("fixture package");

    assert_eq!(package.resource_id, None);
    assert_eq!(package.language.as_deref(), Some("zh-CN"));
    assert_eq!(package.architecture, Architecture::X64);
    assert_eq!(package.package_kind, PackageKind::Main);
}

#[tokio::test]
async fn fe3_sha256_metadata_rejects_malformed_and_conflicting_values() {
    let fixture = include_str!("fixtures/fe3-applicability.xml");

    let malformed = fixture.replace("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", "not-base64");
    assert!(matches!(
        StoreLibResolverAdapter::parse_fixture(&malformed).await,
        Err(ResolverError::InvalidPackageDigest)
    ));

    let wrong_length = fixture.replace(
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    );
    assert!(matches!(
        StoreLibResolverAdapter::parse_fixture(&wrong_length).await,
        Err(ResolverError::InvalidPackageDigest)
    ));

    let conflicting = fixture.replace(
        "</AdditionalDigest>",
        "</AdditionalDigest><AdditionalDigest Algorithm=\"SHA256\">ERERERERERERERERERERERERERERERERERERERERERE=</AdditionalDigest>",
    );
    assert!(matches!(
        StoreLibResolverAdapter::parse_fixture(&conflicting).await,
        Err(ResolverError::ConflictingPackageDigest)
    ));

    let sha1_only = fixture.replace(
        "<AdditionalDigest Algorithm=\"SHA256\">AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=</AdditionalDigest>",
        "",
    );
    let graph = StoreLibResolverAdapter::parse_fixture(&sha1_only)
        .await
        .expect("SHA-1-only metadata remains displayable");
    assert_eq!(graph.packages[0].sha256, None);
}

#[tokio::test]
async fn fe3_arm32_package_is_represented_without_aborting_the_graph() {
    let graph =
        StoreLibResolverAdapter::parse_fixture(include_str!("fixtures/fe3-arm-package.xml"))
            .await
            .expect("ARM32 package should remain representable");

    assert_eq!(graph.packages.len(), 1);
    assert_eq!(graph.packages[0].architecture, Architecture::Arm);
}
