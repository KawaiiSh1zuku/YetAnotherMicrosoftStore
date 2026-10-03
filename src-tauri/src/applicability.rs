use std::{
    cmp::Reverse,
    collections::{HashMap, HashSet},
    fmt,
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{Architecture, PackageFormat, PackageKind, PackageVersion},
    resolver::{DependencyKind, FrameworkRequirement, PackageGraph, ResolvedPackage},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCapabilities {
    pub os_version: PackageVersion,
    pub native_architecture: Architecture,
    pub compatible_architectures: Vec<Architecture>,
    pub supported_formats: Vec<PackageFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    Install,
    Update,
    Repair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPreferences {
    pub market: String,
    pub preferred_architectures: Vec<Architecture>,
    pub preferred_languages: Vec<String>,
    pub mode: SelectionMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPackage {
    pub identity_name: String,
    pub publisher: Option<String>,
    pub version: PackageVersion,
    pub architecture: Architecture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionReason {
    SelectedMain,
    SelectedDependency,
    SelectedResource,
    SatisfiedByInstalled,
    MinimumOsNotMet,
    ArchitectureIncompatible,
    UnsupportedFormat,
    VersionNotNewer,
    VersionAheadOfCatalog,
    MissingApplicabilityMetadata,
    PackageNotInstalled,
    LowerPriority,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDecision {
    pub update_id: String,
    pub selected: bool,
    pub reason: DecisionReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionResult {
    pub packages: Vec<ResolvedPackage>,
    pub decisions: Vec<PackageDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionRejectionReason {
    Market,
    OperatingSystem,
    Format,
    Architecture,
    Dependency,
    PackageNotInstalled,
    Version,
    NoCompatiblePackage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedMainPackage {
    pub version: PackageVersion,
    pub architecture: Architecture,
    pub format: PackageFormat,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectionPreview {
    pub installable: bool,
    pub main: Option<SelectedMainPackage>,
    pub dependency_count: usize,
    pub rejection_reason: Option<SelectionRejectionReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicabilityError {
    MarketMismatch,
    NoCompatiblePackage,
    DependencyUnresolved {
        update_id: String,
    },
    DependencyCycle {
        update_id: String,
    },
    PackageNotInstalled {
        identity_name: String,
    },
    VersionAheadOfCatalog {
        identity_name: String,
        installed: PackageVersion,
        catalog: PackageVersion,
    },
}

impl fmt::Display for ApplicabilityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MarketMismatch => formatter.write_str("package graph market does not match"),
            Self::NoCompatiblePackage => formatter.write_str("no compatible package is available"),
            Self::DependencyUnresolved { .. } => {
                formatter.write_str("a required package dependency could not be resolved")
            }
            Self::DependencyCycle { .. } => {
                formatter.write_str("the package dependency graph contains a cycle")
            }
            Self::PackageNotInstalled { .. } => {
                formatter.write_str("the package to update is not installed")
            }
            Self::VersionAheadOfCatalog { .. } => {
                formatter.write_str("installed package version is ahead of the catalog")
            }
        }
    }
}

impl std::error::Error for ApplicabilityError {}

pub fn preview_packages(
    graph: &PackageGraph,
    host: &HostCapabilities,
    preferences: &SelectionPreferences,
    installed: &[InstalledPackage],
) -> SelectionPreview {
    match select_packages(graph, host, preferences, installed) {
        Ok(selection) => {
            let main = selection
                .packages
                .iter()
                .find(|package| package.package_kind == PackageKind::Main)
                .map(|package| SelectedMainPackage {
                    version: package.version,
                    architecture: package.architecture,
                    format: package.format,
                    language: package.language.clone(),
                });
            let dependency_count = selection
                .packages
                .iter()
                .filter(|package| package.package_kind == PackageKind::Framework)
                .count();
            SelectionPreview {
                installable: main.is_some(),
                main,
                dependency_count,
                rejection_reason: None,
            }
        }
        Err(error) => SelectionPreview {
            installable: false,
            main: None,
            dependency_count: 0,
            rejection_reason: Some(closed_rejection_reason(&error, graph, host)),
        },
    }
}

fn closed_rejection_reason(
    error: &ApplicabilityError,
    graph: &PackageGraph,
    host: &HostCapabilities,
) -> SelectionRejectionReason {
    match error {
        ApplicabilityError::MarketMismatch => SelectionRejectionReason::Market,
        ApplicabilityError::PackageNotInstalled { .. } => {
            SelectionRejectionReason::PackageNotInstalled
        }
        ApplicabilityError::VersionAheadOfCatalog { .. } => SelectionRejectionReason::Version,
        ApplicabilityError::DependencyCycle { .. } => SelectionRejectionReason::Dependency,
        ApplicabilityError::DependencyUnresolved { update_id } => graph
            .packages
            .iter()
            .find(|package| package.update_id == *update_id)
            .and_then(|package| package_incompatibility_reason(package, host))
            .map(selection_rejection_from_decision)
            .unwrap_or(SelectionRejectionReason::Dependency),
        ApplicabilityError::NoCompatiblePackage => graph
            .packages
            .iter()
            .filter(|package| package.package_kind == PackageKind::Main)
            .filter_map(|package| package_incompatibility_reason(package, host))
            .map(selection_rejection_from_decision)
            .next()
            .unwrap_or(SelectionRejectionReason::NoCompatiblePackage),
    }
}

fn selection_rejection_from_decision(reason: DecisionReason) -> SelectionRejectionReason {
    match reason {
        DecisionReason::MinimumOsNotMet => SelectionRejectionReason::OperatingSystem,
        DecisionReason::UnsupportedFormat => SelectionRejectionReason::Format,
        DecisionReason::ArchitectureIncompatible => SelectionRejectionReason::Architecture,
        DecisionReason::PackageNotInstalled => SelectionRejectionReason::PackageNotInstalled,
        DecisionReason::VersionNotNewer | DecisionReason::VersionAheadOfCatalog => {
            SelectionRejectionReason::Version
        }
        _ => SelectionRejectionReason::NoCompatiblePackage,
    }
}

pub fn select_packages(
    graph: &PackageGraph,
    host: &HostCapabilities,
    preferences: &SelectionPreferences,
    installed: &[InstalledPackage],
) -> Result<SelectionResult, ApplicabilityError> {
    if graph
        .market
        .as_deref()
        .is_some_and(|market| !market.eq_ignore_ascii_case(&preferences.market))
    {
        return Err(ApplicabilityError::MarketMismatch);
    }

    let mut decisions = Vec::new();
    let mut applicable_roots = Vec::new();
    let mut version_ahead = None;
    let mut package_not_installed = None;

    for package in graph
        .packages
        .iter()
        .filter(|package| package.package_kind == PackageKind::Main)
    {
        if let Some(reason) = package_incompatibility_reason(package, host) {
            decisions.push(decision(package, false, reason));
            continue;
        }

        if let Some(installed_package) = matching_installed(package, installed) {
            match preferences.mode {
                SelectionMode::Update if package.version <= installed_package.version => {
                    let reason = if package.version < installed_package.version {
                        version_ahead = Some(version_ahead_error(package, installed_package));
                        DecisionReason::VersionAheadOfCatalog
                    } else {
                        DecisionReason::VersionNotNewer
                    };
                    decisions.push(decision(package, false, reason));
                    continue;
                }
                SelectionMode::Install | SelectionMode::Repair
                    if package.version < installed_package.version =>
                {
                    version_ahead = Some(version_ahead_error(package, installed_package));
                    decisions.push(decision(
                        package,
                        false,
                        DecisionReason::VersionAheadOfCatalog,
                    ));
                    continue;
                }
                _ => {}
            }
        } else if preferences.mode == SelectionMode::Update {
            let identity_name = package
                .identity_name
                .clone()
                .unwrap_or_else(|| package.update_id.clone());
            package_not_installed = Some(ApplicabilityError::PackageNotInstalled { identity_name });
            decisions.push(decision(
                package,
                false,
                DecisionReason::PackageNotInstalled,
            ));
            continue;
        }
        applicable_roots.push(package);
    }

    let root = applicable_roots
        .into_iter()
        .max_by_key(|package| root_rank(package, preferences));
    let Some(root) = root else {
        return Err(version_ahead
            .or(package_not_installed)
            .unwrap_or(ApplicabilityError::NoCompatiblePackage));
    };

    for package in graph
        .packages
        .iter()
        .filter(|package| package.package_kind == PackageKind::Main)
    {
        if package.update_id == root.update_id {
            decisions.push(decision(package, true, DecisionReason::SelectedMain));
        } else if !decisions
            .iter()
            .any(|decision| decision.update_id == package.update_id)
        {
            decisions.push(decision(package, false, DecisionReason::LowerPriority));
        }
    }

    let context = SelectionContext {
        graph,
        host,
        preferences,
        installed,
    };
    let mut selection = SelectionAccumulator {
        packages: vec![root.clone()],
        selected_ids: HashSet::from([root.update_id.as_str()]),
        visiting_ids: HashSet::new(),
        completed_ids: HashSet::new(),
        decisions,
    };
    select_framework_requirements(&context, &mut selection)?;
    select_dependencies(&context, root, &mut selection)?;

    Ok(SelectionResult {
        packages: selection.packages,
        decisions: selection.decisions,
    })
}

fn select_framework_requirements<'a>(
    context: &SelectionContext<'a>,
    selection: &mut SelectionAccumulator<'a>,
) -> Result<(), ApplicabilityError> {
    for requirement in &context.graph.framework_requirements {
        let dependency = best_framework_candidate(requirement, context).ok_or_else(|| {
            ApplicabilityError::DependencyUnresolved {
                update_id: requirement.identity_name.clone(),
            }
        })?;
        let minimum_version = requirement
            .minimum_version
            .unwrap_or_else(|| PackageVersion::new(0, 0, 0, 0));
        if matching_installed(dependency, context.installed)
            .is_some_and(|installed| installed.version >= minimum_version)
        {
            selection.decisions.push(decision(
                dependency,
                false,
                DecisionReason::SatisfiedByInstalled,
            ));
            continue;
        }
        add_selected(dependency, DecisionReason::SelectedDependency, selection);
        select_dependencies(context, dependency, selection)?;
    }
    Ok(())
}

fn best_framework_candidate<'a>(
    requirement: &FrameworkRequirement,
    context: &SelectionContext<'a>,
) -> Option<&'a ResolvedPackage> {
    context
        .graph
        .packages
        .iter()
        .filter(|package| {
            package.package_kind == PackageKind::Framework
                && package.identity_name.as_deref().is_some_and(|identity| {
                    identity.eq_ignore_ascii_case(&requirement.identity_name)
                })
                && requirement
                    .minimum_version
                    .is_none_or(|minimum| package.version >= minimum)
                && package_incompatibility_reason(package, context.host).is_none()
        })
        .max_by_key(|package| root_rank(package, context.preferences))
}

struct SelectionContext<'a> {
    graph: &'a PackageGraph,
    host: &'a HostCapabilities,
    preferences: &'a SelectionPreferences,
    installed: &'a [InstalledPackage],
}

struct SelectionAccumulator<'a> {
    packages: Vec<ResolvedPackage>,
    selected_ids: HashSet<&'a str>,
    visiting_ids: HashSet<&'a str>,
    completed_ids: HashSet<&'a str>,
    decisions: Vec<PackageDecision>,
}

fn select_dependencies<'a>(
    context: &SelectionContext<'a>,
    source: &'a ResolvedPackage,
    selection: &mut SelectionAccumulator<'a>,
) -> Result<(), ApplicabilityError> {
    if selection.completed_ids.contains(source.update_id.as_str()) {
        return Ok(());
    }
    if !selection.visiting_ids.insert(source.update_id.as_str()) {
        return Err(ApplicabilityError::DependencyCycle {
            update_id: source.update_id.clone(),
        });
    }

    let prerequisites = context.graph.dependencies.iter().filter(|edge| {
        edge.source_update_id == source.update_id && edge.kind == DependencyKind::Prerequisite
    });
    for edge in prerequisites {
        let dependency = context
            .graph
            .packages
            .iter()
            .find(|package| package.update_id == edge.target_update_id)
            .ok_or_else(|| ApplicabilityError::DependencyUnresolved {
                update_id: edge.target_update_id.clone(),
            })?;
        if matching_installed(dependency, context.installed)
            .is_some_and(|installed| installed.version >= dependency.version)
        {
            selection.decisions.push(decision(
                dependency,
                false,
                DecisionReason::SatisfiedByInstalled,
            ));
            continue;
        }
        if let Some(reason) = package_incompatibility_reason(dependency, context.host) {
            selection
                .decisions
                .push(decision(dependency, false, reason));
            return Err(ApplicabilityError::DependencyUnresolved {
                update_id: dependency.update_id.clone(),
            });
        }
        add_selected(dependency, DecisionReason::SelectedDependency, selection);
        select_dependencies(context, dependency, selection)?;
    }

    let bundled = context
        .graph
        .dependencies
        .iter()
        .filter(|edge| {
            edge.source_update_id == source.update_id && edge.kind == DependencyKind::Bundled
        })
        .map(|edge| {
            context
                .graph
                .packages
                .iter()
                .find(|package| package.update_id == edge.target_update_id)
                .ok_or_else(|| ApplicabilityError::DependencyUnresolved {
                    update_id: edge.target_update_id.clone(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut best_language_resources = HashMap::new();
    for package in bundled.iter().filter(|package| {
        package.package_kind == PackageKind::Resource
            && package.is_neutral != Some(true)
            && package.language.is_some()
            && package_incompatibility_reason(package, context.host).is_none()
    }) {
        let Some(rank) = language_rank(package.language.as_deref(), context.preferences) else {
            continue;
        };
        let group = resource_group(package);
        let candidate = (rank, package.update_id.as_str());
        best_language_resources
            .entry(group)
            .and_modify(|current| {
                if candidate < *current {
                    *current = candidate;
                }
            })
            .or_insert(candidate);
    }

    for package in bundled {
        if let Some(reason) = package_incompatibility_reason(package, context.host) {
            selection.decisions.push(decision(package, false, reason));
            continue;
        }
        if package.package_kind == PackageKind::Resource
            && package.is_neutral != Some(true)
            && package.language.is_some()
        {
            let selected = best_language_resources
                .get(&resource_group(package))
                .is_some_and(|(_, update_id)| *update_id == package.update_id);
            if selected {
                add_selected(package, DecisionReason::SelectedResource, selection);
                select_dependencies(context, package, selection)?;
            } else {
                selection
                    .decisions
                    .push(decision(package, false, DecisionReason::LowerPriority));
            }
            continue;
        }
        let reason = if package.package_kind == PackageKind::Resource {
            DecisionReason::SelectedResource
        } else {
            DecisionReason::SelectedDependency
        };
        add_selected(package, reason, selection);
        select_dependencies(context, package, selection)?;
    }
    selection.visiting_ids.remove(source.update_id.as_str());
    selection.completed_ids.insert(source.update_id.as_str());
    Ok(())
}

pub fn package_incompatibility_reason(
    package: &ResolvedPackage,
    host: &HostCapabilities,
) -> Option<DecisionReason> {
    if package.package_kind == PackageKind::Unknown {
        return Some(DecisionReason::MissingApplicabilityMetadata);
    }
    if package
        .minimum_os_version
        .is_some_and(|minimum| minimum > host.os_version)
    {
        return Some(DecisionReason::MinimumOsNotMet);
    }
    if !host.supported_formats.contains(&package.format) {
        return Some(DecisionReason::UnsupportedFormat);
    }
    if package.architecture != Architecture::Neutral
        && !host
            .compatible_architectures
            .contains(&package.architecture)
    {
        return Some(DecisionReason::ArchitectureIncompatible);
    }
    None
}

fn root_rank(
    package: &ResolvedPackage,
    preferences: &SelectionPreferences,
) -> (PackageVersion, bool, Reverse<usize>) {
    (
        package.version,
        matches!(
            package.format,
            PackageFormat::MsixBundle | PackageFormat::AppxBundle | PackageFormat::EappxBundle
        ),
        Reverse(architecture_rank(package.architecture, preferences)),
    )
}

fn architecture_rank(architecture: Architecture, preferences: &SelectionPreferences) -> usize {
    preferences
        .preferred_architectures
        .iter()
        .position(|candidate| *candidate == architecture)
        .unwrap_or(preferences.preferred_architectures.len())
}

fn language_rank(
    language: Option<&str>,
    preferences: &SelectionPreferences,
) -> Option<(usize, u8, usize)> {
    let language = language?.to_ascii_lowercase();
    let preferred = preferences
        .preferred_languages
        .iter()
        .enumerate()
        .filter_map(|(preference_index, preferred)| {
            language_match_rank(&language, &preferred.to_ascii_lowercase())
                .map(|(specificity, distance)| (preference_index, specificity, distance))
        })
        .min();
    if preferred.is_some() {
        return preferred;
    }

    language_match_rank(&language, "en-us")
        .map(|(specificity, distance)| {
            (preferences.preferred_languages.len(), specificity, distance)
        })
        .or(Some((preferences.preferred_languages.len() + 1, 0, 0)))
}

fn language_match_rank(language: &str, preferred: &str) -> Option<(u8, usize)> {
    if preferred == language {
        return Some((0, 0));
    }
    if preferred.starts_with(&format!("{language}-")) {
        return Some((
            1,
            tag_component_count(preferred) - tag_component_count(language),
        ));
    }
    if language.starts_with(&format!("{preferred}-")) {
        return Some((
            2,
            tag_component_count(language) - tag_component_count(preferred),
        ));
    }
    (primary_language(preferred) == primary_language(language)).then_some((3, 0))
}

fn primary_language(language: &str) -> Option<&str> {
    language.split('-').next()
}

fn tag_component_count(language: &str) -> usize {
    language.split('-').count()
}

fn resource_group(package: &ResolvedPackage) -> String {
    package
        .identity_name
        .as_deref()
        .unwrap_or(&package.update_id)
        .to_ascii_lowercase()
}

fn matching_installed<'a>(
    package: &ResolvedPackage,
    installed: &'a [InstalledPackage],
) -> Option<&'a InstalledPackage> {
    let identity_name = package.identity_name.as_deref()?;
    installed
        .iter()
        .filter(|candidate| {
            candidate.identity_name.eq_ignore_ascii_case(identity_name)
                && (package.architecture == Architecture::Neutral
                    || candidate.architecture == package.architecture)
                && package.publisher.as_deref().is_none_or(|publisher| {
                    candidate
                        .publisher
                        .as_deref()
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(publisher))
                })
        })
        .max_by_key(|candidate| candidate.version)
}

fn version_ahead_error(
    package: &ResolvedPackage,
    installed: &InstalledPackage,
) -> ApplicabilityError {
    ApplicabilityError::VersionAheadOfCatalog {
        identity_name: installed.identity_name.clone(),
        installed: installed.version,
        catalog: package.version,
    }
}

fn add_selected<'a>(
    package: &'a ResolvedPackage,
    reason: DecisionReason,
    selection: &mut SelectionAccumulator<'a>,
) {
    if selection.selected_ids.insert(&package.update_id) {
        selection.packages.push(package.clone());
        selection.decisions.push(decision(package, true, reason));
    }
}

fn decision(package: &ResolvedPackage, selected: bool, reason: DecisionReason) -> PackageDecision {
    PackageDecision {
        update_id: package.update_id.clone(),
        selected,
        reason,
    }
}
