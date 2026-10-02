#![cfg(windows)]

use yet_another_microsoft_store_lib::catalog::{
    CatalogError, CatalogProvider, DeviceFamily, StoreLibCatalogAdapter,
};
use yet_another_microsoft_store_lib::domain::{
    Architecture, PackageFormat, PackageKind, PackageVersion,
};
use yet_another_microsoft_store_lib::resolver::{
    DependencyKind, ResolverError, StoreLibResolverAdapter,
};

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
    assert_eq!(products[0].title.as_deref(), Some("Contoso Notes"));

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

    assert!(graph.dependencies.iter().any(|edge| {
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
