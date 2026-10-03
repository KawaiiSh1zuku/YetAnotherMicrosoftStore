use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use crate::{
    applicability::SelectionResult,
    broker_protocol::{PackageFileRequest, PackageIdentity},
    domain::{Architecture, CacheEntry, CacheState, PackageFormat, PackageKind, PackageVersion},
    package_validation::VerifiedPackageSet,
    resolver::{PackageGraph, ResolvedPackage},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentPlan {
    pub product_id: String,
    pub main_update_id: String,
    pub main_version: PackageVersion,
    pub content_id: Option<String>,
    pub package_set: VerifiedPackageSet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentPlanError {
    MissingProductId,
    MissingMainPackage,
    AmbiguousMainPackage,
    MissingPackageIdentity { update_id: String },
    UnsupportedPackageFormat { update_id: String },
    MissingVerifiedCache { update_id: String },
    CacheNotVerified { update_id: String },
    CacheMetadataMismatch { update_id: String },
    DependencyCycle { update_id: String },
}

pub fn build_deployment_plan(
    graph: &PackageGraph,
    selection: &SelectionResult,
    cache: &[CacheEntry],
) -> Result<DeploymentPlan, DeploymentPlanError> {
    let product_id = graph
        .product_id
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or(DeploymentPlanError::MissingProductId)?;
    let main_packages = selection
        .packages
        .iter()
        .filter(|package| package.package_kind == PackageKind::Main)
        .collect::<Vec<_>>();
    let main = match main_packages.as_slice() {
        [] => return Err(DeploymentPlanError::MissingMainPackage),
        [main] => *main,
        _ => return Err(DeploymentPlanError::AmbiguousMainPackage),
    };

    let selected = selection
        .packages
        .iter()
        .map(|package| (package.update_id.as_str(), package))
        .collect::<HashMap<_, _>>();
    for package in selected.values() {
        if !supported_format(package.format) {
            return Err(DeploymentPlanError::UnsupportedPackageFormat {
                update_id: package.update_id.clone(),
            });
        }
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut ordered = Vec::new();
    visit_dependencies(
        main.update_id.as_str(),
        graph,
        &selected,
        &mut visiting,
        &mut visited,
        &mut ordered,
    )?;
    let mut remaining = selected.keys().copied().collect::<Vec<_>>();
    remaining.sort_unstable();
    for update_id in remaining {
        visit_dependencies(
            update_id,
            graph,
            &selected,
            &mut visiting,
            &mut visited,
            &mut ordered,
        )?;
    }

    let main_request = package_request(main, cache)?;
    let dependencies = ordered
        .into_iter()
        .filter(|update_id| *update_id != main.update_id)
        .map(|update_id| package_request(selected[update_id], cache))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(DeploymentPlan {
        product_id: product_id.clone(),
        main_update_id: main.update_id.clone(),
        main_version: main.version,
        content_id: main.content_id.clone(),
        package_set: VerifiedPackageSet {
            main: main_request,
            dependencies,
        },
    })
}

fn visit_dependencies<'a>(
    update_id: &'a str,
    graph: &'a PackageGraph,
    selected: &HashMap<&'a str, &'a ResolvedPackage>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
    ordered: &mut Vec<&'a str>,
) -> Result<(), DeploymentPlanError> {
    if visited.contains(update_id) {
        return Ok(());
    }
    if !visiting.insert(update_id) {
        return Err(DeploymentPlanError::DependencyCycle {
            update_id: update_id.to_owned(),
        });
    }
    let mut dependencies = graph
        .dependencies
        .iter()
        .filter(|edge| edge.source_update_id == update_id)
        .map(|edge| edge.target_update_id.as_str())
        .filter(|target| selected.contains_key(target))
        .collect::<Vec<_>>();
    dependencies.sort_unstable();
    dependencies.dedup();
    for dependency in dependencies {
        visit_dependencies(dependency, graph, selected, visiting, visited, ordered)?;
    }
    visiting.remove(update_id);
    visited.insert(update_id);
    ordered.push(update_id);
    Ok(())
}

fn package_request(
    package: &ResolvedPackage,
    cache: &[CacheEntry],
) -> Result<PackageFileRequest, DeploymentPlanError> {
    let entries = cache
        .iter()
        .filter(|entry| entry.update_id == package.update_id)
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Err(DeploymentPlanError::MissingVerifiedCache {
            update_id: package.update_id.clone(),
        });
    }
    let verified = entries
        .iter()
        .copied()
        .filter(|entry| entry.state == CacheState::Verified)
        .collect::<Vec<_>>();
    if verified.is_empty() {
        return Err(DeploymentPlanError::CacheNotVerified {
            update_id: package.update_id.clone(),
        });
    }
    let Some(expected_size) = package.file_size else {
        return Err(DeploymentPlanError::CacheMetadataMismatch {
            update_id: package.update_id.clone(),
        });
    };
    let Some(expected_hash) = package.sha256.as_deref() else {
        return Err(DeploymentPlanError::CacheMetadataMismatch {
            update_id: package.update_id.clone(),
        });
    };
    let entry = verified
        .into_iter()
        .find(|entry| {
            entry.size == expected_size
                && entry.sha256.eq_ignore_ascii_case(expected_hash)
                && PathBuf::from(&entry.path).is_absolute()
        })
        .ok_or_else(|| DeploymentPlanError::CacheMetadataMismatch {
            update_id: package.update_id.clone(),
        })?;
    let identity_name = package
        .identity_name
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| DeploymentPlanError::MissingPackageIdentity {
            update_id: package.update_id.clone(),
        })?;
    let publisher = package
        .publisher
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| DeploymentPlanError::MissingPackageIdentity {
            update_id: package.update_id.clone(),
        })?;

    Ok(PackageFileRequest {
        path: PathBuf::from(&entry.path),
        sha256_hex: entry.sha256.clone(),
        expected_identity: Some(PackageIdentity {
            name: identity_name.clone(),
            publisher: publisher.clone(),
            version: package.version.components(),
            architecture: architecture_name(package.architecture).to_owned(),
            resource_id: package.resource_id.clone().unwrap_or_default(),
        }),
    })
}

fn supported_format(format: PackageFormat) -> bool {
    matches!(
        format,
        PackageFormat::Msix
            | PackageFormat::Appx
            | PackageFormat::MsixBundle
            | PackageFormat::AppxBundle
            | PackageFormat::Eappx
            | PackageFormat::EappxBundle
    )
}

fn architecture_name(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::X64 => "x64",
        Architecture::Arm64 => "arm64",
        Architecture::Arm => "arm",
        Architecture::X86 => "x86",
        Architecture::Neutral => "neutral",
    }
}
