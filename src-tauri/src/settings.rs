use std::{collections::HashSet, fmt, net::IpAddr};

use reqwest::{ClientBuilder, NoProxy, Proxy, Url};

#[cfg(windows)]
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{GlobalFree, HGLOBAL},
        Networking::WinHttp::{
            WinHttpGetIEProxyConfigForCurrentUser, WINHTTP_CURRENT_USER_IE_PROXY_CONFIG,
        },
    },
};

use crate::domain::{AppSettings, ProxyMode};

#[derive(Clone, PartialEq, Eq)]
pub struct ProxyCredentials {
    username: String,
    password: String,
}

impl ProxyCredentials {
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }
}

impl fmt::Debug for ProxyCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProxyCredentials([redacted])")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum ProxyRoute {
    Disabled,
    System {
        endpoint: String,
        bypass: Option<String>,
    },
    Custom {
        endpoint: String,
        credentials: Option<ProxyCredentials>,
    },
}

impl ProxyRoute {
    pub fn endpoint(&self) -> Option<&str> {
        match self {
            Self::System { endpoint, .. } | Self::Custom { endpoint, .. } => Some(endpoint),
            Self::Disabled => None,
        }
    }

    pub fn username(&self) -> Option<&str> {
        match self {
            Self::Custom {
                credentials: Some(credentials),
                ..
            } => Some(&credentials.username),
            _ => None,
        }
    }

    pub fn apply(&self, builder: ClientBuilder) -> Result<ClientBuilder, SettingsError> {
        match self {
            Self::Disabled => Ok(builder.no_proxy()),
            Self::System { endpoint, bypass } => {
                let proxy = Proxy::all(endpoint)
                    .map_err(|_| SettingsError::SystemProxyInvalid)?
                    .no_proxy(bypass.as_deref().and_then(NoProxy::from_string));
                Ok(builder.no_proxy().proxy(proxy))
            }
            Self::Custom {
                endpoint,
                credentials,
            } => {
                let mut proxy =
                    Proxy::all(endpoint).map_err(|_| SettingsError::ProxyHostInvalid)?;
                if let Some(credentials) = credentials {
                    proxy = proxy.basic_auth(&credentials.username, &credentials.password);
                }
                Ok(builder.no_proxy().proxy(proxy))
            }
        }
    }
}

impl fmt::Debug for ProxyRoute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => formatter.write_str("Disabled"),
            Self::System { endpoint, bypass } => formatter
                .debug_struct("System")
                .field("endpoint", endpoint)
                .field("bypass", bypass)
                .finish(),
            Self::Custom { endpoint, .. } => formatter
                .debug_struct("Custom")
                .field("endpoint", endpoint)
                .field("credentials", &"[redacted]")
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsError {
    ProxyHostMissing,
    ProxyHostInvalid,
    ProxyPortMissing,
    SystemProxyReadFailed,
    SystemProxyInvalid,
    SystemProxyAutoConfigUnsupported,
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProxyHostMissing => formatter.write_str("proxy host is required"),
            Self::ProxyHostInvalid => formatter.write_str("proxy host is invalid"),
            Self::ProxyPortMissing => formatter.write_str("proxy port is required"),
            Self::SystemProxyReadFailed => {
                formatter.write_str("Windows system proxy settings could not be read")
            }
            Self::SystemProxyInvalid => {
                formatter.write_str("Windows system proxy settings are invalid")
            }
            Self::SystemProxyAutoConfigUnsupported => {
                formatter.write_str("Windows automatic proxy configuration is not supported yet")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

pub trait ProxyProvider {
    fn resolve(
        &self,
        settings: &AppSettings,
        credentials: Option<ProxyCredentials>,
    ) -> Result<ProxyRoute, SettingsError>;

    fn configure(
        &self,
        settings: &AppSettings,
        credentials: Option<ProxyCredentials>,
        builder: ClientBuilder,
    ) -> Result<ClientBuilder, SettingsError> {
        self.resolve(settings, credentials)?.apply(builder)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultProxyProvider;

impl ProxyProvider for DefaultProxyProvider {
    fn resolve(
        &self,
        settings: &AppSettings,
        credentials: Option<ProxyCredentials>,
    ) -> Result<ProxyRoute, SettingsError> {
        let scheme = match settings.proxy_mode {
            ProxyMode::Disabled => return Ok(ProxyRoute::Disabled),
            ProxyMode::System => {
                return read_windows_system_proxy()
                    .and_then(|config| resolve_windows_system_proxy(&config))
            }
            ProxyMode::Http => "http",
            ProxyMode::Https => "https",
            ProxyMode::Socks5 => "socks5h",
        };
        let host = settings
            .proxy_host
            .as_deref()
            .filter(|host| !host.trim().is_empty())
            .ok_or(SettingsError::ProxyHostMissing)?;
        if host != host.trim()
            || host.contains("://")
            || host.contains('/')
            || host.contains('@')
            || host.chars().any(char::is_whitespace)
        {
            return Err(SettingsError::ProxyHostInvalid);
        }
        let port = settings.proxy_port.ok_or(SettingsError::ProxyPortMissing)?;
        let endpoint = format!("{scheme}://{host}:{port}/");
        Url::parse(&endpoint).map_err(|_| SettingsError::ProxyHostInvalid)?;
        Ok(ProxyRoute::Custom {
            endpoint,
            credentials,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowsSystemProxyConfig {
    pub proxy: Option<String>,
    pub bypass: Option<String>,
    pub auto_detect: bool,
    pub auto_config_url_present: bool,
}

pub fn resolve_windows_system_proxy(
    config: &WindowsSystemProxyConfig,
) -> Result<ProxyRoute, SettingsError> {
    if let Some(proxy) = config
        .proxy
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        let Some(endpoint) = select_https_proxy(proxy) else {
            return Ok(ProxyRoute::Disabled);
        };
        let endpoint = normalize_system_proxy_endpoint(endpoint)?;
        let bypass = config.bypass.as_deref().and_then(normalize_system_bypass);
        return Ok(ProxyRoute::System { endpoint, bypass });
    }
    if config.auto_detect || config.auto_config_url_present {
        return Err(SettingsError::SystemProxyAutoConfigUnsupported);
    }
    Ok(ProxyRoute::Disabled)
}

fn select_https_proxy(value: &str) -> Option<&str> {
    let mut general = None;
    for item in value
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        match item.split_once('=') {
            Some((scheme, endpoint)) if scheme.eq_ignore_ascii_case("https") => {
                return Some(endpoint.trim())
            }
            Some(_) => {}
            None => general = Some(item),
        }
    }
    general.filter(|endpoint| !endpoint.is_empty())
}

fn normalize_system_proxy_endpoint(value: &str) -> Result<String, SettingsError> {
    let endpoint = if value.contains("://") {
        value.to_owned()
    } else {
        format!("http://{value}")
    };
    let url = Url::parse(&endpoint).map_err(|_| SettingsError::SystemProxyInvalid)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(SettingsError::SystemProxyInvalid);
    }
    Ok(url.to_string())
}

fn normalize_system_bypass(value: &str) -> Option<String> {
    let normalized = value
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty() && !item.eq_ignore_ascii_case("<local>"))
        .map(|item| {
            item.strip_prefix("*.")
                .map_or(item.to_owned(), |item| format!(".{item}"))
        })
        .collect::<Vec<_>>()
        .join(",");
    (!normalized.is_empty()).then_some(normalized)
}

#[cfg(windows)]
fn read_windows_system_proxy() -> Result<WindowsSystemProxyConfig, SettingsError> {
    let mut native = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut native) }
        .map_err(|_| SettingsError::SystemProxyReadFailed)?;
    let native = NativeProxyConfig(native);
    Ok(WindowsSystemProxyConfig {
        proxy: copy_native_string(native.0.lpszProxy)?,
        bypass: copy_native_string(native.0.lpszProxyBypass)?,
        auto_detect: native.0.fAutoDetect.as_bool(),
        auto_config_url_present: !native.0.lpszAutoConfigUrl.is_null(),
    })
}

#[cfg(not(windows))]
fn read_windows_system_proxy() -> Result<WindowsSystemProxyConfig, SettingsError> {
    Err(SettingsError::SystemProxyReadFailed)
}

#[cfg(windows)]
fn copy_native_string(value: PWSTR) -> Result<Option<String>, SettingsError> {
    if value.is_null() {
        return Ok(None);
    }
    unsafe { value.to_string() }
        .map(Some)
        .map_err(|_| SettingsError::SystemProxyInvalid)
}

#[cfg(windows)]
struct NativeProxyConfig(WINHTTP_CURRENT_USER_IE_PROXY_CONFIG);

#[cfg(windows)]
impl Drop for NativeProxyConfig {
    fn drop(&mut self) {
        for value in [
            self.0.lpszAutoConfigUrl,
            self.0.lpszProxy,
            self.0.lpszProxyBypass,
        ] {
            if !value.is_null() {
                unsafe {
                    let _ = GlobalFree(Some(HGLOBAL(value.as_ptr().cast())));
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicyError {
    InvalidUrl,
    HttpsRequired,
    CredentialsForbidden,
    HostNotAllowed,
    TooManyRedirects,
}

impl fmt::Display for NetworkPolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => formatter.write_str("download URL is invalid"),
            Self::HttpsRequired => formatter.write_str("download URL must use HTTPS"),
            Self::CredentialsForbidden => {
                formatter.write_str("download URL credentials are forbidden")
            }
            Self::HostNotAllowed => formatter.write_str("download host is not allowed"),
            Self::TooManyRedirects => formatter.write_str("download redirect limit exceeded"),
        }
    }
}

impl std::error::Error for NetworkPolicyError {}

#[derive(Debug, Clone)]
pub struct NetworkPolicy {
    allowed_hosts: HashSet<String>,
    max_redirects: usize,
    allow_loopback_http: bool,
}

impl NetworkPolicy {
    pub fn production<I, S>(allowed_hosts: I) -> Result<Self, NetworkPolicyError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let allowed_hosts = allowed_hosts
            .into_iter()
            .map(|host| normalize_host(host.as_ref()))
            .collect::<Result<HashSet<_>, _>>()?;
        if allowed_hosts.is_empty() {
            return Err(NetworkPolicyError::HostNotAllowed);
        }
        Ok(Self {
            allowed_hosts,
            max_redirects: 5,
            allow_loopback_http: false,
        })
    }

    pub fn loopback_fixture(max_redirects: usize) -> Self {
        Self {
            allowed_hosts: HashSet::new(),
            max_redirects,
            allow_loopback_http: true,
        }
    }

    pub fn validate_url(&self, value: &str) -> Result<(), NetworkPolicyError> {
        let url = Url::parse(value).map_err(|_| NetworkPolicyError::InvalidUrl)?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(NetworkPolicyError::CredentialsForbidden);
        }
        let host = url.host_str().ok_or(NetworkPolicyError::InvalidUrl)?;
        let loopback_http = self.allow_loopback_http
            && url.scheme() == "http"
            && host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if url.scheme() != "https" && !loopback_http {
            return Err(NetworkPolicyError::HttpsRequired);
        }
        if loopback_http {
            return Ok(());
        }
        let host = host.to_ascii_lowercase();
        if self.allowed_hosts.contains(&host) {
            Ok(())
        } else {
            Err(NetworkPolicyError::HostNotAllowed)
        }
    }

    pub fn validate_redirect(
        &self,
        previous: &str,
        next: &str,
        redirect_count: usize,
    ) -> Result<(), NetworkPolicyError> {
        if redirect_count > self.max_redirects {
            return Err(NetworkPolicyError::TooManyRedirects);
        }
        self.validate_url(previous)?;
        self.validate_url(next)
    }

    pub fn max_redirects(&self) -> usize {
        self.max_redirects
    }
}

fn normalize_host(host: &str) -> Result<String, NetworkPolicyError> {
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty()
        || host.contains('/')
        || host.contains('@')
        || host.contains("://")
        || host.chars().any(char::is_whitespace)
    {
        return Err(NetworkPolicyError::HostNotAllowed);
    }
    Ok(host)
}
