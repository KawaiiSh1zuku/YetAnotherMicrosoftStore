use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use storelib_rs::{
    DCatEndpoint, DisplayCatalogHandler, FE3Handler, IdentifierType, Lang, Locale, Market,
    PackageType,
};

use crate::domain::{Architecture, PackageFormat, PackageKind, PackageVersion};

pub type ResolverFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyKind {
    Prerequisite,
    Bundled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub source_update_id: String,
    pub target_update_id: String,
    pub kind: DependencyKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameworkRequirement {
    pub identity_name: String,
    pub minimum_version: Option<PackageVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPackage {
    pub package_moniker: String,
    pub package_type: String,
    pub package_uri: Option<String>,
    pub file_name: Option<String>,
    pub file_size: Option<u64>,
    pub sha256: Option<String>,
    pub update_id: String,
    pub identity_name: Option<String>,
    pub publisher: Option<String>,
    pub version: PackageVersion,
    pub architecture: Architecture,
    pub resource_id: Option<String>,
    pub package_kind: PackageKind,
    pub minimum_os_version: Option<PackageVersion>,
    pub language: Option<String>,
    pub is_neutral: Option<bool>,
    pub content_id: Option<String>,
    pub format: PackageFormat,
    pub prerequisites: Vec<String>,
    pub bundled_updates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PackageGraph {
    pub product_id: Option<String>,
    pub market: Option<String>,
    pub packages: Vec<ResolvedPackage>,
    pub dependencies: Vec<DependencyEdge>,
    #[serde(default)]
    pub framework_requirements: Vec<FrameworkRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolverError {
    MalformedFixture(String),
    MissingField(&'static str),
    InvalidPackageSize,
    InvalidPackageDigest,
    ConflictingPackageDigest,
    InvalidPackageUrl,
    InvalidPackageMoniker,
    UnsupportedPackageFormat,
    InvalidMinimumOsVersion,
    InvalidFrameworkVersion,
    UnsupportedLocale,
    StoreLib,
}

impl std::fmt::Display for ResolverError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MalformedFixture(message) => {
                write!(formatter, "malformed FE3 fixture: {message}")
            }
            Self::MissingField(field) => write!(formatter, "FE3 field is missing: {field}"),
            Self::InvalidPackageSize => formatter.write_str("FE3 package size is invalid"),
            Self::InvalidPackageDigest => formatter.write_str("FE3 package digest is invalid"),
            Self::ConflictingPackageDigest => {
                formatter.write_str("FE3 package SHA-256 digests conflict")
            }
            Self::InvalidPackageUrl => formatter.write_str("FE3 package URL is invalid"),
            Self::InvalidPackageMoniker => formatter.write_str("FE3 package moniker is invalid"),
            Self::UnsupportedPackageFormat => {
                formatter.write_str("FE3 package format is unsupported")
            }
            Self::InvalidMinimumOsVersion => {
                formatter.write_str("FE3 minimum OS version is invalid")
            }
            Self::InvalidFrameworkVersion => {
                formatter.write_str("DCAT framework minimum version is invalid")
            }
            Self::UnsupportedLocale => formatter.write_str("Store locale is unsupported"),
            Self::StoreLib => formatter.write_str("store protocol request failed"),
        }
    }
}

impl std::error::Error for ResolverError {}

pub trait PackageResolver {
    fn resolve<'a>(
        &'a mut self,
        product_id: &'a str,
    ) -> ResolverFuture<'a, Result<PackageGraph, ResolverError>>;
}

/// Adapter that keeps FE3 and storelib_rs package models behind the project DTO.
pub struct StoreLibResolverAdapter {
    handler: DisplayCatalogHandler,
}

impl StoreLibResolverAdapter {
    pub fn production() -> Self {
        Self {
            handler: DisplayCatalogHandler::production(),
        }
    }

    pub fn production_for_locale(market: &str, language: &str) -> Result<Self, ResolverError> {
        let market = Market::from_str(&market.trim().to_ascii_uppercase())
            .map_err(|_| ResolverError::UnsupportedLocale)?;
        let language = language
            .split('-')
            .next()
            .ok_or(ResolverError::UnsupportedLocale)?
            .trim()
            .to_ascii_lowercase();
        let language = Lang::from_str(&language).map_err(|_| ResolverError::UnsupportedLocale)?;
        let locale = Locale::new(market, language, true).with_full_tag(true);
        Ok(Self {
            handler: DisplayCatalogHandler::new(DCatEndpoint::Production, locale),
        })
    }

    /// Parse and normalize a captured FE3 response without network IO.
    pub async fn parse_fixture(xml: &str) -> Result<PackageGraph, ResolverError> {
        validate_appx_metadata(xml)?;
        // Keep the upstream parser responsible for the FE3 package shape, but
        // attach update IDs by package moniker because its low-level parser
        // intentionally leaves that field for the higher-level handler.
        let _ = FE3Handler::process_update_ids(xml)
            .map_err(|error| ResolverError::MalformedFixture(error.to_string()))?;
        let mut instances = FE3Handler::get_package_instances(xml)
            .await
            .map_err(|error| ResolverError::MalformedFixture(error.to_string()))?;
        attach_update_ids(xml, &mut instances)?;
        normalize_instances(instances, Vec::new())
    }
}

impl Default for StoreLibResolverAdapter {
    fn default() -> Self {
        Self::production()
    }
}

impl PackageResolver for StoreLibResolverAdapter {
    fn resolve<'a>(
        &'a mut self,
        product_id: &'a str,
    ) -> ResolverFuture<'a, Result<PackageGraph, ResolverError>> {
        Box::pin(async move {
            if product_id.trim().is_empty() {
                return Err(ResolverError::MissingField("productId"));
            }

            self.handler
                .query_dcat(product_id, IdentifierType::ProductId, None)
                .await
                .map_err(|_| ResolverError::StoreLib)?;
            let framework_requirements = self
                .handler
                .framework_dependencies()
                .into_iter()
                .filter_map(|dependency| {
                    let identity_name = dependency.package_identity.as_deref()?.trim().to_owned();
                    (!identity_name.is_empty())
                        .then_some((identity_name, dependency.min_version.as_ref()))
                })
                .map(|(identity_name, minimum_version)| {
                    Ok(FrameworkRequirement {
                        identity_name,
                        minimum_version: minimum_version
                            .map(parse_framework_version)
                            .transpose()?,
                    })
                })
                .collect::<Result<Vec<_>, ResolverError>>()?;
            let instances = self
                .handler
                .get_packages_for_product(None)
                .await
                .map_err(|_| ResolverError::StoreLib)?;
            let mut graph = normalize_instances(instances, framework_requirements)?;
            graph.product_id = Some(product_id.to_owned());
            graph.market = Some(self.handler.selected_locale.market.as_str().to_owned());
            Ok(graph)
        })
    }
}

fn normalize_instances(
    instances: Vec<storelib_rs::PackageInstance>,
    framework_requirements: Vec<FrameworkRequirement>,
) -> Result<PackageGraph, ResolverError> {
    let mut graph = PackageGraph {
        framework_requirements,
        ..PackageGraph::default()
    };
    for instance in instances {
        if instance.package_moniker.trim().is_empty() {
            return Err(ResolverError::MissingField("packageMoniker"));
        }
        if instance.update_id.trim().is_empty() {
            return Err(ResolverError::MissingField("updateId"));
        }

        let file_size = instance
            .file_size
            .map(|size| u64::try_from(size).map_err(|_| ResolverError::InvalidPackageSize))
            .transpose()?;
        let moniker = parse_package_moniker(&instance.package_moniker)?;
        let identity_name = instance
            .package_identity_name
            .clone()
            .or_else(|| Some(moniker.identity_name.to_owned()));
        let publisher = instance
            .family_metadata
            .as_ref()
            .and_then(|metadata| metadata.publisher.clone());
        let language = instance.default_properties_language.clone();
        let format = package_format(
            instance.file_name.as_deref(),
            &instance.package_type,
            instance.is_appx_bundle,
        )?;
        let is_framework = instance.is_appx_framework == Some(true)
            || instance
                .update_properties
                .as_ref()
                .and_then(|properties| properties.is_appx_framework)
                == Some(true);
        let is_bundle = matches!(
            format,
            PackageFormat::MsixBundle | PackageFormat::AppxBundle | PackageFormat::EappxBundle
        );
        let is_resource = !is_bundle && moniker.resource_id.is_some();
        let package_kind = if is_framework {
            PackageKind::Framework
        } else if is_resource {
            PackageKind::Resource
        } else {
            PackageKind::Main
        };
        let minimum_os_version = minimum_os_version(&instance)?;
        let content_id = instance.package_content_id.clone().or_else(|| {
            instance
                .applicability_blob
                .as_ref()
                .and_then(|blob| blob.content_package_id.clone())
        });
        let is_neutral = Some(is_bundle || (moniker.resource_id.is_none() && language.is_none()));
        // FE3 prerequisites are Windows Update category GUIDs, not package
        // update IDs. Named framework requirements come from DCAT instead.
        for target in &instance.bundled_updates {
            graph.dependencies.push(DependencyEdge {
                source_update_id: instance.update_id.clone(),
                target_update_id: target.clone(),
                kind: DependencyKind::Bundled,
            });
        }

        let sha256 = normalize_sha256(&instance)?;
        graph.packages.push(ResolvedPackage {
            package_moniker: instance.package_moniker,
            package_type: package_type_name(&instance.package_type),
            package_uri: normalize_delivery_url(instance.package_uri)?,
            file_name: instance.file_name.or(instance.package_file_name),
            file_size,
            sha256,
            update_id: instance.update_id,
            identity_name,
            publisher,
            version: moniker.version,
            architecture: moniker.architecture,
            resource_id: moniker.resource_id,
            package_kind,
            minimum_os_version,
            language,
            is_neutral,
            content_id,
            format,
            prerequisites: instance.prerequisites,
            bundled_updates: instance.bundled_updates,
        });
    }
    Ok(graph)
}

fn parse_framework_version(value: &serde_json::Value) -> Result<PackageVersion, ResolverError> {
    let packed = match value {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(value) => value.trim().parse::<u64>().ok(),
        _ => None,
    };
    if let Some(packed) = packed {
        return Ok(PackageVersion::from_packed(packed));
    }
    value
        .as_str()
        .ok_or(ResolverError::InvalidFrameworkVersion)?
        .trim()
        .parse()
        .map_err(|_| ResolverError::InvalidFrameworkVersion)
}

pub fn normalize_delivery_url(value: Option<String>) -> Result<Option<String>, ResolverError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let url = reqwest::Url::parse(&value).map_err(|_| ResolverError::InvalidPackageUrl)?;
    let host = url
        .host_str()
        .ok_or(ResolverError::InvalidPackageUrl)?
        .to_ascii_lowercase();
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !crate::settings::MICROSOFT_PACKAGE_HOSTS.contains(&host.as_str())
    {
        return Err(ResolverError::InvalidPackageUrl);
    }
    match url.scheme() {
        "https" if url.port().is_none_or(|port| port == 443) => {}
        "http" if url.port().is_none_or(|port| port == 80) => {}
        _ => return Err(ResolverError::InvalidPackageUrl),
    }
    Ok(Some(url.to_string()))
}

fn normalize_sha256(
    instance: &storelib_rs::PackageInstance,
) -> Result<Option<String>, ResolverError> {
    let mut encoded = instance
        .additional_digests
        .iter()
        .filter(|entry| digest_algorithm_is_sha256(&entry.algorithm))
        .map(|entry| entry.value.as_str())
        .collect::<Vec<_>>();
    if instance
        .digest_algorithm
        .as_deref()
        .is_some_and(digest_algorithm_is_sha256)
    {
        if let Some(value) = instance.digest.as_deref() {
            encoded.push(value);
        }
    }

    let mut normalized = None;
    for value in encoded {
        let bytes = STANDARD
            .decode(value.trim())
            .map_err(|_| ResolverError::InvalidPackageDigest)?;
        if bytes.len() != 32 {
            return Err(ResolverError::InvalidPackageDigest);
        }
        let value = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if normalized
            .as_ref()
            .is_some_and(|existing| existing != &value)
        {
            return Err(ResolverError::ConflictingPackageDigest);
        }
        normalized = Some(value);
    }
    Ok(normalized)
}

fn digest_algorithm_is_sha256(value: &str) -> bool {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .eq("sha256".chars())
}

struct MonikerMetadata {
    identity_name: String,
    version: PackageVersion,
    architecture: Architecture,
    resource_id: Option<String>,
}

fn parse_package_moniker(value: &str) -> Result<MonikerMetadata, ResolverError> {
    let mut parts = value.rsplitn(5, '_');
    let _publisher_id = parts.next().ok_or(ResolverError::InvalidPackageMoniker)?;
    let resource_id = parts.next().ok_or(ResolverError::InvalidPackageMoniker)?;
    let architecture = match parts.next() {
        Some("x64" | "amd64") => Architecture::X64,
        Some("arm64") => Architecture::Arm64,
        Some("arm") => Architecture::Arm,
        Some("x86") => Architecture::X86,
        Some("neutral") => Architecture::Neutral,
        _ => return Err(ResolverError::InvalidPackageMoniker),
    };
    let version = parts
        .next()
        .ok_or(ResolverError::InvalidPackageMoniker)?
        .parse()
        .map_err(|_| ResolverError::InvalidPackageMoniker)?;
    let identity_name = parts
        .next()
        .filter(|identity| !identity.is_empty())
        .ok_or(ResolverError::InvalidPackageMoniker)?;

    Ok(MonikerMetadata {
        identity_name: identity_name.to_owned(),
        version,
        architecture,
        resource_id: (!resource_id.is_empty() && resource_id != "~")
            .then(|| resource_id.to_owned()),
    })
}

fn minimum_os_version(
    instance: &storelib_rs::PackageInstance,
) -> Result<Option<PackageVersion>, ResolverError> {
    let mut minimum = None;
    for value in instance
        .applicability_blob
        .as_ref()
        .and_then(|blob| blob.content_target_platforms.as_deref())
        .unwrap_or_default()
        .iter()
        .filter_map(|platform| platform.platform_min_version)
    {
        let packed = u64::try_from(value).map_err(|_| ResolverError::InvalidMinimumOsVersion)?;
        let version = PackageVersion::from_packed(packed);
        minimum = Some(minimum.map_or(version, |current: PackageVersion| current.max(version)));
    }
    Ok(minimum)
}

fn package_format(
    file_name: Option<&str>,
    package_type: &PackageType,
    is_bundle: Option<bool>,
) -> Result<PackageFormat, ResolverError> {
    let extension = file_name
        .and_then(|name| name.rsplit_once('.').map(|(_, extension)| extension))
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("msix") => Ok(PackageFormat::Msix),
        Some("appx") => Ok(PackageFormat::Appx),
        Some("msixbundle") => Ok(PackageFormat::MsixBundle),
        Some("appxbundle") => Ok(PackageFormat::AppxBundle),
        Some("eappx" | "emsix") => Ok(PackageFormat::Eappx),
        Some("eappxbundle" | "emsixbundle") => Ok(PackageFormat::EappxBundle),
        Some("msixvc") => Ok(PackageFormat::Msixvc),
        Some(_) => Err(ResolverError::UnsupportedPackageFormat),
        None if matches!(package_type, PackageType::AppX | PackageType::Uap) => {
            if is_bundle == Some(true) {
                Ok(PackageFormat::AppxBundle)
            } else {
                Ok(PackageFormat::Appx)
            }
        }
        None => Ok(PackageFormat::Unknown),
    }
}

fn package_type_name(package_type: &PackageType) -> String {
    match package_type {
        PackageType::AppX => "appx".to_owned(),
        PackageType::Uap => "uap".to_owned(),
        PackageType::Xap => "xap".to_owned(),
        PackageType::Unknown => "unknown".to_owned(),
    }
}

fn validate_appx_metadata(xml: &str) -> Result<(), ResolverError> {
    let document = roxmltree::Document::parse(xml)
        .map_err(|error| ResolverError::MalformedFixture(error.to_string()))?;
    let mut found = false;
    for node in document
        .descendants()
        .filter(|node| node.tag_name().name() == "AppxMetadata")
    {
        found = true;
        if node
            .attribute("PackageMoniker")
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(ResolverError::MissingField("packageMoniker"));
        }
    }
    if !found {
        return Err(ResolverError::MissingField("packageMoniker"));
    }
    Ok(())
}

fn attach_update_ids(
    xml: &str,
    instances: &mut [storelib_rs::PackageInstance],
) -> Result<(), ResolverError> {
    let document = roxmltree::Document::parse(xml)
        .map_err(|error| ResolverError::MalformedFixture(error.to_string()))?;

    for instance in instances {
        let metadata = document.descendants().find(|node| {
            node.tag_name().name() == "AppxMetadata"
                && node.attribute("PackageMoniker") == Some(instance.package_moniker.as_str())
        });
        let update_id = metadata
            .and_then(|node| {
                node.ancestors()
                    .find(|ancestor| ancestor.tag_name().name() == "Xml")
            })
            .and_then(|xml_node| {
                xml_node
                    .children()
                    .find(|child| child.tag_name().name() == "UpdateIdentity")
            })
            .and_then(|identity| identity.attribute("UpdateID"))
            .filter(|update_id| !update_id.trim().is_empty())
            .ok_or(ResolverError::MissingField("updateId"))?;
        instance.update_id = update_id.to_owned();
    }
    Ok(())
}
