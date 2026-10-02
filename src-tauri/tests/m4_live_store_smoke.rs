#![cfg(windows)]

use std::time::{SystemTime, UNIX_EPOCH};

use yet_another_microsoft_store_lib::{
    catalog::{CatalogProvider, StoreLibCatalogAdapter},
    resolver::{PackageResolver, StoreLibResolverAdapter},
};

#[tokio::test]
#[ignore = "requires M4_LIVE_SMOKE=1 and explicit product/market/language"]
async fn production_adapters_return_normalized_product_and_package_graph() {
    assert_eq!(std::env::var("M4_LIVE_SMOKE").as_deref(), Ok("1"));
    let product_id = std::env::var("M4_PRODUCT_ID").expect("M4_PRODUCT_ID is required");
    let market = std::env::var("M4_MARKET").expect("M4_MARKET is required");
    let language = std::env::var("M4_LANGUAGE").expect("M4_LANGUAGE is required");

    let mut catalog = StoreLibCatalogAdapter::production_for_locale(&market, &language)
        .expect("supported Store locale");
    let product = catalog
        .product(&product_id)
        .await
        .expect("production catalog lookup");
    assert_eq!(product.product_id, product_id);

    let mut resolver = StoreLibResolverAdapter::production_for_locale(&market, &language)
        .expect("supported Store locale");
    let graph = resolver
        .resolve(&product_id)
        .await
        .expect("production package resolution");
    assert!(!graph.packages.is_empty());

    let observed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after Unix epoch")
        .as_secs();
    println!(
        "M4 live smoke: product={product_id} market={market} language={language} observed_at_unix={observed_at} packages={} dependencies={}",
        graph.packages.len(),
        graph.dependencies.len()
    );
}
