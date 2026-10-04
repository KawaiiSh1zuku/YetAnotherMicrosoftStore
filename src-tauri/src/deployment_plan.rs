use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use crate::{
    applicability::SelectionResult,
    cache::CacheManager,
    domain::{Architecture, CacheEntry, CacheState, PackageFormat, PackageKind, PackageVersion},
    job_events::{DeploymentCheckpoint, DeploymentCheckpointPackage, JobTargetRole},
    package::{PackageFileRequest, PackageIdentity},
    package_validation::{ValidationError, VerifiedPackageSet},
    resolver::{PackageGraph, ResolvedPackage},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentPlan {
    pub product_id: String,
    pub main_update_id: String,
    pub main_version: PackageVersion,
    pub content_id: Option<String>,
    pub package_set: VerifiedPackageSet,
    pub checkpoint_packages: Vec<DeploymentCheckpointPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeploymentPlanError {
    MissingProductId,
    MissingMainPackage,
    AmbiguousMainPackage,
    MissingPackageIdentity {
        update_id: String,
    },
    UnsupportedPackageFormat {
        update_id: String,
    },
    MissingVerifiedCache {
        update_id: String,
    },
    CacheNotVerified {
        update_id: String,
    },
    CacheMetadataMismatch {
        update_id: String,
    },
    UnsafeCachePath {
        update_id: String,
    },
    PackageValidation {
        update_id: String,
        error: ValidationError,
    },
    CheckpointInvalid,
    DependencyCycle {
        update_id: String,
    },
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

    let (main_request, main_checkpoint) = package_request(main, cache, JobTargetRole::Main, 0)?;
    let dependency_packages = ordered
        .into_iter()
        .filter(|update_id| *update_id != main.update_id)
        .enumerate()
        .map(|(index, update_id)| {
            package_request(
                selected[update_id],
                cache,
                if selected[update_id].package_kind == PackageKind::Resource {
                    JobTargetRole::Resource
                } else {
                    JobTargetRole::Dependency
                },
                u16::try_from(index + 1).map_err(|_| DeploymentPlanError::CheckpointInvalid)?,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let dependencies = dependency_packages
        .iter()
        .map(|(request, _)| request.clone())
        .collect();
    let checkpoint_packages = std::iter::once(main_checkpoint)
        .chain(
            dependency_packages
                .into_iter()
                .map(|(_, checkpoint)| checkpoint),
        )
        .collect();

    Ok(DeploymentPlan {
        product_id: product_id.clone(),
        main_update_id: main.update_id.clone(),
        main_version: main.version,
        content_id: main.content_id.clone(),
        package_set: VerifiedPackageSet {
            main: main_request,
            dependencies,
        },
        checkpoint_packages,
    })
}

impl DeploymentCheckpoint {
    pub fn from_plan(plan: &DeploymentPlan) -> Self {
        Self {
            product_id: plan.product_id.clone(),
            main_update_id: plan.main_update_id.clone(),
            main_version: plan.main_version,
            content_id: plan.content_id.clone(),
            packages: plan.checkpoint_packages.clone(),
        }
    }

    pub fn rebuild_verified_plan<F>(
        &self,
        cache_root: &Path,
        cache: &[CacheEntry],
        mut verify: F,
    ) -> Result<DeploymentPlan, DeploymentPlanError>
    where
        F: FnMut(&PackageFileRequest) -> Result<(), ValidationError>,
    {
        if !self.is_safe() {
            return Err(DeploymentPlanError::CheckpointInvalid);
        }
        let manager =
            CacheManager::new(cache_root).map_err(|_| DeploymentPlanError::CheckpointInvalid)?;
        let mut packages = self.packages.clone();
        packages.sort_by_key(|package| package.order);
        let mut main = None;
        let mut dependencies = Vec::new();
        for package in &packages {
            let entry = cache
                .iter()
                .find(|entry| entry.cache_key == package.cache_key)
                .ok_or_else(|| DeploymentPlanError::MissingVerifiedCache {
                    update_id: package.update_id.clone(),
                })?;
            if entry.state != CacheState::Verified {
                return Err(DeploymentPlanError::CacheNotVerified {
                    update_id: package.update_id.clone(),
                });
            }
            if entry.update_id != package.update_id
                || entry.size != package.expected_size
                || !entry.sha256.eq_ignore_ascii_case(&package.sha256)
            {
                return Err(DeploymentPlanError::CacheMetadataMismatch {
                    update_id: package.update_id.clone(),
                });
            }
            let path = PathBuf::from(&entry.path);
            if !path.is_absolute() || manager.validate_verified_path(&path).is_err() {
                return Err(DeploymentPlanError::UnsafeCachePath {
                    update_id: package.update_id.clone(),
                });
            }
            if !matches!(fs::metadata(&path), Ok(metadata) if metadata.len() == package.expected_size)
            {
                return Err(DeploymentPlanError::CacheMetadataMismatch {
                    update_id: package.update_id.clone(),
                });
            }
            if !supported_format(package.format) {
                return Err(DeploymentPlanError::UnsupportedPackageFormat {
                    update_id: package.update_id.clone(),
                });
            }
            let request = PackageFileRequest {
                path,
                sha256_hex: package.sha256.clone(),
                expected_identity: Some(PackageIdentity {
                    name: package.identity_name.clone(),
                    publisher: package.publisher.clone(),
                    version: package
                        .version
                        .parse::<PackageVersion>()
                        .map_err(|_| DeploymentPlanError::CheckpointInvalid)?
                        .components(),
                    architecture: architecture_name(package.architecture).to_owned(),
                    resource_id: package.resource_id.clone().unwrap_or_default(),
                }),
            };
            verify(&request).map_err(|error| DeploymentPlanError::PackageValidation {
                update_id: package.update_id.clone(),
                error,
            })?;
            if package.role == JobTargetRole::Main {
                main = Some(request);
            } else {
                dependencies.push(request);
            }
        }
        Ok(DeploymentPlan {
            product_id: self.product_id.clone(),
            main_update_id: self.main_update_id.clone(),
            main_version: self.main_version,
            content_id: self.content_id.clone(),
            package_set: VerifiedPackageSet {
                main: main.ok_or(DeploymentPlanError::CheckpointInvalid)?,
                dependencies,
            },
            checkpoint_packages: packages,
        })
    }
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
    role: JobTargetRole,
    order: u16,
) -> Result<(PackageFileRequest, DeploymentCheckpointPackage), DeploymentPlanError> {
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

    let request = PackageFileRequest {
        path: PathBuf::from(&entry.path),
        sha256_hex: entry.sha256.clone(),
        expected_identity: Some(PackageIdentity {
            name: identity_name.clone(),
            publisher: publisher.clone(),
            version: package.version.components(),
            architecture: architecture_name(package.architecture).to_owned(),
            resource_id: package.resource_id.clone().unwrap_or_default(),
        }),
    };
    Ok((
        request,
        DeploymentCheckpointPackage {
            role,
            order,
            cache_key: entry.cache_key.clone(),
            update_id: package.update_id.clone(),
            identity_name: identity_name.clone(),
            publisher: publisher.clone(),
            version: package.version.to_string(),
            architecture: package.architecture,
            resource_id: package.resource_id.clone(),
            package_kind: package.package_kind,
            format: package.format,
            expected_size,
            sha256: expected_hash.to_owned(),
        },
    ))
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
