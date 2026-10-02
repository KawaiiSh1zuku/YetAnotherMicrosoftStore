#![cfg(windows)]

use yet_another_microsoft_store_lib::{
    domain::{AppSettings, Architecture, ProxyCredentialPolicy, ProxyMode},
    settings::{
        resolve_windows_system_proxy, DefaultProxyProvider, NetworkPolicy, NetworkPolicyError,
        ProxyCredentials, ProxyProvider, ProxyRoute, SettingsError, WindowsSystemProxyConfig,
    },
};

fn settings(mode: ProxyMode, host: Option<&str>, port: Option<u16>) -> AppSettings {
    AppSettings {
        region: "CN".to_owned(),
        market: "CN".to_owned(),
        preferred_architectures: vec![Architecture::X64],
        preferred_languages: vec!["zh-CN".to_owned()],
        proxy_mode: mode,
        proxy_host: host.map(str::to_owned),
        proxy_port: port,
        proxy_credentials: ProxyCredentialPolicy::PromptEveryTime,
        cache_enabled: true,
        cache_directory: r"E:\Cache".to_owned(),
        max_cache_bytes: 1024,
        retention_days: 7,
        keep_installed_payloads: false,
        max_concurrent_downloads: 2,
    }
}

#[test]
fn proxy_modes_resolve_without_persisting_runtime_credentials() {
    let provider = DefaultProxyProvider;

    assert_eq!(
        provider
            .resolve(&settings(ProxyMode::Disabled, None, None), None)
            .expect("disabled route"),
        ProxyRoute::Disabled
    );
    let credentials = ProxyCredentials::new("runtime-user", "runtime-password");
    let route = provider
        .resolve(
            &settings(ProxyMode::Https, Some("proxy.example.test"), Some(8443)),
            Some(credentials),
        )
        .expect("custom route");
    assert_eq!(route.endpoint(), Some("https://proxy.example.test:8443/"));
    assert_eq!(route.username(), Some("runtime-user"));

    let debug = format!("{route:?}");
    assert!(!debug.contains("runtime-user"));
    assert!(!debug.contains("runtime-password"));

    let json = serde_json::to_string(&settings(
        ProxyMode::Https,
        Some("proxy.example.test"),
        Some(8443),
    ))
    .expect("settings serialize");
    assert!(!json.contains("runtime-user"));
    assert!(!json.contains("runtime-password"));

    let http = provider
        .resolve(
            &settings(ProxyMode::Http, Some("proxy.example.test"), Some(8080)),
            None,
        )
        .expect("HTTP proxy route");
    assert_eq!(http.endpoint(), Some("http://proxy.example.test:8080/"));
    let socks = provider
        .resolve(
            &settings(ProxyMode::Socks5, Some("127.0.0.1"), Some(1080)),
            None,
        )
        .expect("SOCKS5 proxy route");
    assert_eq!(socks.endpoint(), Some("socks5h://127.0.0.1:1080/"));
}

#[test]
fn windows_system_proxy_prefers_the_https_static_route() {
    let route = resolve_windows_system_proxy(&WindowsSystemProxyConfig {
        proxy: Some(
            "http=proxy-http.example.test:8080;https=proxy-secure.example.test:8443".to_owned(),
        ),
        bypass: Some("localhost;*.example.test".to_owned()),
        auto_detect: false,
        auto_config_url_present: false,
    })
    .expect("static system proxy route");

    assert_eq!(
        route,
        ProxyRoute::System {
            endpoint: "http://proxy-secure.example.test:8443/".to_owned(),
            bypass: Some("localhost,.example.test".to_owned()),
        }
    );
}

#[test]
fn windows_system_proxy_rejects_unsupported_auto_configuration() {
    assert_eq!(
        resolve_windows_system_proxy(&WindowsSystemProxyConfig {
            proxy: None,
            bypass: None,
            auto_detect: true,
            auto_config_url_present: false,
        }),
        Err(SettingsError::SystemProxyAutoConfigUnsupported)
    );

    assert_eq!(
        resolve_windows_system_proxy(&WindowsSystemProxyConfig::default()),
        Ok(ProxyRoute::Disabled)
    );

    assert_eq!(
        resolve_windows_system_proxy(&WindowsSystemProxyConfig {
            proxy: Some("http=proxy-http.example.test:8080".to_owned()),
            ..WindowsSystemProxyConfig::default()
        }),
        Ok(ProxyRoute::Disabled)
    );
}

#[test]
fn custom_proxy_rejects_missing_or_invalid_runtime_configuration() {
    let provider = DefaultProxyProvider;

    assert_eq!(
        provider.resolve(&settings(ProxyMode::Http, None, Some(8080)), None),
        Err(SettingsError::ProxyHostMissing)
    );
    assert_eq!(
        provider.resolve(
            &settings(ProxyMode::Socks5, Some("proxy.example.test"), None),
            None,
        ),
        Err(SettingsError::ProxyPortMissing)
    );
    assert_eq!(
        provider.resolve(
            &settings(
                ProxyMode::Http,
                Some("https://proxy.example.test"),
                Some(8080)
            ),
            None,
        ),
        Err(SettingsError::ProxyHostInvalid)
    );
}

#[test]
fn production_policy_rechecks_scheme_and_host_for_redirects() {
    let policy =
        NetworkPolicy::production(["tlu.dl.delivery.mp.microsoft.com"]).expect("valid allowlist");

    assert!(policy
        .validate_url("https://tlu.dl.delivery.mp.microsoft.com/content/app.msix")
        .is_ok());
    assert_eq!(
        policy.validate_url("http://tlu.dl.delivery.mp.microsoft.com/content/app.msix"),
        Err(NetworkPolicyError::HttpsRequired)
    );
    assert_eq!(
        policy.validate_url("https://attacker.example/app.msix"),
        Err(NetworkPolicyError::HostNotAllowed)
    );
    assert_eq!(
        policy.validate_redirect(
            "https://tlu.dl.delivery.mp.microsoft.com/content/app.msix",
            "https://attacker.example/app.msix",
            1,
        ),
        Err(NetworkPolicyError::HostNotAllowed)
    );
    assert_eq!(
        policy.validate_redirect(
            "https://tlu.dl.delivery.mp.microsoft.com/content/app.msix",
            "https://tlu.dl.delivery.mp.microsoft.com/content/app2.msix",
            6,
        ),
        Err(NetworkPolicyError::TooManyRedirects)
    );
}

#[test]
fn loopback_http_requires_the_explicit_fixture_policy() {
    let policy = NetworkPolicy::loopback_fixture(5);

    assert!(policy
        .validate_url("http://127.0.0.1:30123/package")
        .is_ok());
    assert!(policy.validate_url("http://[::1]:30123/package").is_ok());
    assert_eq!(
        policy.validate_url("http://localhost.example/package"),
        Err(NetworkPolicyError::HttpsRequired)
    );
}
