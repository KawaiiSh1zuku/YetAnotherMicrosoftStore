use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{
    domain::{Architecture, PackageVersion},
    inventory::PackageInventoryRecord,
    resolver::ResolvedPackage,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssociationConfidence {
    VerifiedDeployment,
    ExactPackageFamilyName,
    ExactIdentityPublisher,
    Ambiguous,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageAssociation {
    pub package_family_name: String,
    pub product_id: Option<String>,
    pub content_id: Option<String>,
    pub identity_name: String,
    pub publisher: String,
    pub confidence: AssociationConfidence,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogIdentity {
    pub product_id: String,
    pub package_family_name: Option<String>,
    pub content_id: Option<String>,
    pub identity_name: String,
    pub publisher: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogVersionState {
    UnresolvedAssociation,
    SourceIdentityMismatch,
    UpToDate,
    UpdateAvailable,
    VersionAheadOfCatalog,
}

pub fn correlate_package(
    installed: &PackageInventoryRecord,
    catalog: &[CatalogIdentity],
    cached: Option<&PackageAssociation>,
    observed_at: i64,
) -> PackageAssociation {
    let pfn_matches = catalog
        .iter()
        .filter(|candidate| {
            candidate
                .package_family_name
                .as_deref()
                .is_some_and(|value| value.eq_ignore_ascii_case(&installed.package_family_name))
        })
        .collect::<Vec<_>>();
    if let Some(association) = unique_association(
        installed,
        &pfn_matches,
        AssociationConfidence::ExactPackageFamilyName,
        observed_at,
    ) {
        return association;
    }
    if multiple_products(&pfn_matches) {
        return unresolved(installed, AssociationConfidence::Ambiguous, observed_at);
    }

    if let Some(cached) = cached.filter(|cached| cache_matches(installed, cached)) {
        let mut cached = cached.clone();
        cached.observed_at = observed_at;
        return cached;
    }

    let identity_matches = catalog
        .iter()
        .filter(|candidate| {
            candidate
                .identity_name
                .eq_ignore_ascii_case(&installed.identity_name)
                && candidate.publisher == installed.publisher
        })
        .collect::<Vec<_>>();
    if let Some(association) = unique_association(
        installed,
        &identity_matches,
        AssociationConfidence::ExactIdentityPublisher,
        observed_at,
    ) {
        return association;
    }
    if multiple_products(&identity_matches) {
        return unresolved(installed, AssociationConfidence::Ambiguous, observed_at);
    }
    unresolved(installed, AssociationConfidence::Unresolved, observed_at)
}

pub fn compare_catalog_version(
    installed: &PackageInventoryRecord,
    catalog_main: &ResolvedPackage,
    association: &PackageAssociation,
) -> CatalogVersionState {
    if association.product_id.is_none() {
        return CatalogVersionState::UnresolvedAssociation;
    }
    let Some(identity_name) = catalog_main.identity_name.as_deref() else {
        return CatalogVersionState::SourceIdentityMismatch;
    };
    let Some(publisher) = catalog_main.publisher.as_deref() else {
        return CatalogVersionState::SourceIdentityMismatch;
    };
    if !identity_name.eq_ignore_ascii_case(&installed.identity_name)
        || publisher != installed.publisher
        || !architecture_matches(&installed.architecture, catalog_main.architecture)
    {
        return CatalogVersionState::SourceIdentityMismatch;
    }

    let installed_version = PackageVersion::new(
        installed.version[0],
        installed.version[1],
        installed.version[2],
        installed.version[3],
    );
    match installed_version.cmp(&catalog_main.version) {
        std::cmp::Ordering::Less => CatalogVersionState::UpdateAvailable,
        std::cmp::Ordering::Equal => CatalogVersionState::UpToDate,
        std::cmp::Ordering::Greater => CatalogVersionState::VersionAheadOfCatalog,
    }
}

fn unique_association(
    installed: &PackageInventoryRecord,
    matches: &[&CatalogIdentity],
    confidence: AssociationConfidence,
    observed_at: i64,
) -> Option<PackageAssociation> {
    let product_ids = matches
        .iter()
        .map(|candidate| candidate.product_id.as_str())
        .collect::<HashSet<_>>();
    if product_ids.len() != 1 {
        return None;
    }
    let candidate = matches.first()?;
    Some(PackageAssociation {
        package_family_name: installed.package_family_name.clone(),
        product_id: Some(candidate.product_id.clone()),
        content_id: candidate.content_id.clone(),
        identity_name: installed.identity_name.clone(),
        publisher: installed.publisher.clone(),
        confidence,
        observed_at,
    })
}

fn multiple_products(matches: &[&CatalogIdentity]) -> bool {
    matches
        .iter()
        .map(|candidate| candidate.product_id.as_str())
        .collect::<HashSet<_>>()
        .len()
        > 1
}

fn cache_matches(installed: &PackageInventoryRecord, cached: &PackageAssociation) -> bool {
    cached
        .package_family_name
        .eq_ignore_ascii_case(&installed.package_family_name)
        && cached
            .identity_name
            .eq_ignore_ascii_case(&installed.identity_name)
        && cached.publisher == installed.publisher
        && cached.confidence == AssociationConfidence::VerifiedDeployment
        && cached.product_id.is_some()
}

fn unresolved(
    installed: &PackageInventoryRecord,
    confidence: AssociationConfidence,
    observed_at: i64,
) -> PackageAssociation {
    PackageAssociation {
        package_family_name: installed.package_family_name.clone(),
        product_id: None,
        content_id: None,
        identity_name: installed.identity_name.clone(),
        publisher: installed.publisher.clone(),
        confidence,
        observed_at,
    }
}

fn architecture_matches(installed: &str, catalog: Architecture) -> bool {
    catalog == Architecture::Neutral
        || installed.eq_ignore_ascii_case(match catalog {
            Architecture::X64 => "x64",
            Architecture::Arm64 => "arm64",
            Architecture::Arm => "arm",
            Architecture::X86 => "x86",
            Architecture::Neutral => "neutral",
        })
}
