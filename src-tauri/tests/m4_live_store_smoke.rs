#![cfg(windows)]

use std::{
    error::Error as _,
    time::{SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use reqwest::header::{CONTENT_RANGE, RANGE};
use yet_another_microsoft_store_lib::{
    catalog::{CatalogProvider, StoreLibCatalogAdapter},
    resolver::{DependencyKind, PackageResolver, StoreLibResolverAdapter},
    settings::{NetworkPolicy, ProxyRoute, MICROSOFT_PACKAGE_HOSTS},
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
    let network_policy =
        NetworkPolicy::production(MICROSOFT_PACKAGE_HOSTS).expect("production network policy");
    for package in &graph.packages {
        let package_url = package
            .package_uri
            .as_deref()
            .expect("live package metadata must include a download URL");
        let parsed_url = reqwest::Url::parse(package_url).expect("live package URL must parse");
        let host = parsed_url
            .host_str()
            .expect("live package URL must contain a host")
            .to_owned();
        println!(
            "M4 live transport: update_id={} scheme={} host={}",
            package.update_id,
            parsed_url.scheme(),
            host
        );
        network_policy
            .validate_url(package_url)
            .expect("live package URL must satisfy the production policy");
        assert!(
            package.file_size.is_some_and(|size| size > 0),
            "live package metadata must include a positive file size"
        );
        let sha256 = package
            .sha256
            .as_deref()
            .expect("live package metadata must include an explicit SHA-256 digest");
        assert_eq!(sha256.len(), 64, "SHA-256 must contain 64 hex digits");
        assert!(
            sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "SHA-256 must be normalized lowercase hexadecimal"
        );
        println!(
            "M4 live package: update_id={} identity={} version={} architecture={:?} kind={:?} format={:?} bytes={} host={} sha256_prefix={}",
            package.update_id,
            package.identity_name.as_deref().unwrap_or("none"),
            package.version,
            package.architecture,
            package.package_kind,
            package.format,
            package.file_size.unwrap_or_default(),
            host,
            &sha256[..12]
        );
    }

    let observed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after Unix epoch")
        .as_secs();
    let prerequisite_edges = graph
        .dependencies
        .iter()
        .filter(|edge| edge.kind == DependencyKind::Prerequisite)
        .count();
    let bundled_edges = graph.dependencies.len() - prerequisite_edges;
    let unresolved_edges = graph
        .dependencies
        .iter()
        .filter(|edge| {
            !graph
                .packages
                .iter()
                .any(|package| package.update_id == edge.target_update_id)
        })
        .count();
    println!(
        "M4 live smoke: product={product_id} pfn={} market={market} language={language} observed_at_unix={observed_at} packages={} dependencies={} framework_requirements={} prerequisite_edges={prerequisite_edges} bundled_edges={bundled_edges} unresolved_edges={unresolved_edges} normalized_sha256=true",
        product.package_family_name.as_deref().unwrap_or("none"),
        graph.packages.len(),
        graph.dependencies.len(),
        graph.framework_requirements.len()
    );
}

#[tokio::test]
#[ignore = "requires M4_LIVE_SMOKE=1, explicit product/locale, and M4_PROXY_URL route selection"]
async fn production_delivery_supports_bounded_range_through_selected_route() {
    assert_eq!(std::env::var("M4_LIVE_SMOKE").as_deref(), Ok("1"));
    let product_id = std::env::var("M4_PRODUCT_ID").expect("M4_PRODUCT_ID is required");
    let market = std::env::var("M4_MARKET").expect("M4_MARKET is required");
    let language = std::env::var("M4_LANGUAGE").expect("M4_LANGUAGE is required");
    let proxy_url = std::env::var("M4_PROXY_URL").expect("M4_PROXY_URL is required");
    let mut resolver = StoreLibResolverAdapter::production_for_locale(&market, &language)
        .expect("supported Store locale");
    let graph = resolver
        .resolve(&product_id)
        .await
        .expect("production package resolution");
    let package = graph.packages.first().expect("resolved package");
    let mut url = reqwest::Url::parse(package.package_uri.as_deref().expect("delivery URL"))
        .expect("delivery URL parses");
    if std::env::var("M4_FORCE_HTTPS").as_deref() == Ok("1") {
        url.set_scheme("https")
            .expect("HTTP can be upgraded to HTTPS");
        url.set_port(None).expect("default HTTPS port");
    }
    let url = url.as_str();
    NetworkPolicy::production(MICROSOFT_PACKAGE_HOSTS)
        .expect("network policy")
        .validate_url(url)
        .expect("delivery URL satisfies policy");

    let route = if proxy_url == "disabled" {
        ProxyRoute::Disabled
    } else {
        ProxyRoute::Custom {
            endpoint: proxy_url.clone(),
            credentials: None,
        }
    };
    let client = route
        .apply(
            reqwest::Client::builder()
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .no_zstd(),
        )
        .expect("proxy route")
        .build()
        .expect("HTTP client");
    let response = client
        .get(url)
        .header(RANGE, "bytes=0-1048575")
        .send()
        .await
        .unwrap_or_else(|error| {
            let connect = error.is_connect();
            let timeout = error.is_timeout();
            let request = error.is_request();
            let body = error.is_body();
            let decode = error.is_decode();
            let status = error.status();
            let mut sources = Vec::new();
            let mut source = error.source();
            while let Some(current) = source {
                sources.push(
                    current
                        .to_string()
                        .replace(url, "[redacted-url]")
                        .replace(&proxy_url, "[redacted-proxy]"),
                );
                source = current.source();
            }
            let error = error.without_url();
            panic!(
                "bounded delivery request failed: connect={connect} timeout={timeout} request={request} body={body} decode={decode} status={status:?} error={error} sources={sources:?}"
            )
        });
    let status = response.status();
    assert!(
        status == reqwest::StatusCode::PARTIAL_CONTENT || status == reqwest::StatusCode::OK,
        "bounded delivery returned status {status}"
    );
    let content_range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut stream = response.bytes_stream();
    let mut received = 0_usize;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.unwrap_or_else(|error| {
            let connect = error.is_connect();
            let timeout = error.is_timeout();
            let body = error.is_body();
            let status = error.status();
            let error = error.without_url();
            panic!(
                "bounded delivery body failed: connect={connect} timeout={timeout} body={body} status={status:?} error={error}"
            )
        });
        received = received.saturating_add(chunk.len());
        if received >= 1024 * 1024 {
            break;
        }
    }
    assert!(received > 0, "bounded delivery body was empty");
    println!(
        "M4 bounded delivery: status={} received={} content_range_present={}",
        status.as_u16(),
        received,
        content_range.is_some()
    );
}
