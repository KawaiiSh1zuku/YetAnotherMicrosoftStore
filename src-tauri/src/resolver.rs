use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use storelib_rs::{DisplayCatalogHandler, FE3Handler, IdentifierType, PackageType};

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
pub struct ResolvedPackage {
    pub package_moniker: String,
    pub package_type: String,
    pub package_uri: Option<String>,
    pub file_name: Option<String>,
    pub file_size: Option<u64>,
    pub digest: Option<String>,
    pub update_id: String,
    pub prerequisites: Vec<String>,
    pub bundled_updates: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PackageGraph {
    pub packages: Vec<ResolvedPackage>,
    pub dependencies: Vec<DependencyEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolverError {
    MalformedFixture(String),
    MissingField(&'static str),
    InvalidPackageSize,
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
        normalize_instances(instances)
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
            let instances = self
                .handler
                .get_packages_for_product(None)
                .await
                .map_err(|_| ResolverError::StoreLib)?;
            normalize_instances(instances)
        })
    }
}

fn normalize_instances(
    instances: Vec<storelib_rs::PackageInstance>,
) -> Result<PackageGraph, ResolverError> {
    let mut graph = PackageGraph::default();
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
        for target in &instance.prerequisites {
            graph.dependencies.push(DependencyEdge {
                source_update_id: instance.update_id.clone(),
                target_update_id: target.clone(),
                kind: DependencyKind::Prerequisite,
            });
        }
        for target in &instance.bundled_updates {
            graph.dependencies.push(DependencyEdge {
                source_update_id: instance.update_id.clone(),
                target_update_id: target.clone(),
                kind: DependencyKind::Bundled,
            });
        }

        graph.packages.push(ResolvedPackage {
            package_moniker: instance.package_moniker,
            package_type: package_type_name(instance.package_type),
            package_uri: instance.package_uri,
            file_name: instance.file_name.or(instance.package_file_name),
            file_size,
            digest: instance.digest,
            update_id: instance.update_id,
            prerequisites: instance.prerequisites,
            bundled_updates: instance.bundled_updates,
        });
    }
    Ok(graph)
}

fn package_type_name(package_type: PackageType) -> String {
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
