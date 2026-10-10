//! Verifies which workspace package owns every direct external dependency.
//!
//! Cargo supplies canonical names and all declarations, including renamed,
//! optional, and inactive target dependencies. Local helpers must become reviewed
//! workspace members; a registry approval never permits a nonmember path source.

mod policy;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use cargo_metadata::Metadata;

use super::model::{DependencyClass, DependencyEdge};
use policy::{ExternalPackagePolicy, PACKAGE_POLICIES};

const GUIDE: &str = "see docs/architecture/dependency-rules.md";

/// Returns the number of distinct approved direct external package/kind edges.
///
/// Source, version, feature, license, and advisory review remain separate supply
/// chain controls. This check verifies ownership and fails closed for local
/// dependencies that the workspace architecture policy cannot classify.
pub(super) fn check(metadata: &Metadata) -> Result<usize, Vec<String>> {
    check_with_policy(metadata, PACKAGE_POLICIES)
}

fn check_with_policy(
    metadata: &Metadata,
    policies: &[ExternalPackagePolicy],
) -> Result<usize, Vec<String>> {
    let packages: BTreeSet<_> = metadata
        .workspace_packages()
        .into_iter()
        .map(|package| package.name.as_str())
        .collect();
    let (approved, mut violations) = validate_policy(policies, &packages);
    let (declared, declaration_violations) = external_edges(metadata);
    violations.extend(declaration_violations);

    for edge in declared.difference(&approved) {
        violations.insert(format!("unapproved external dependency: {edge}; {GUIDE}"));
    }
    for edge in approved.difference(&declared) {
        violations.insert(format!("stale external dependency rule: {edge}; {GUIDE}"));
    }

    if violations.is_empty() {
        Ok(declared.len())
    } else {
        Err(violations.into_iter().collect())
    }
}

/// Identity comes from a member's manifest directory, never its spelling alone.
/// A registry package named like a workspace member is still an external edge.
fn external_edges(metadata: &Metadata) -> (BTreeSet<DependencyEdge>, BTreeSet<String>) {
    let packages = metadata.workspace_packages();
    let member_roots: BTreeSet<_> = packages
        .iter()
        .filter_map(|package| package.manifest_path.parent())
        .collect();
    let mut edges = BTreeSet::new();
    let mut violations = BTreeSet::new();

    for package in packages {
        for dependency in &package.dependencies {
            if dependency
                .path
                .as_deref()
                .is_some_and(|path| member_roots.contains(path))
            {
                continue;
            }
            let class = match super::dependency_class(dependency.kind) {
                Ok(class) => class,
                Err(reason) => {
                    violations.insert(format!(
                        "{reason} on external dependency {} -> {}; {GUIDE}",
                        package.name, dependency.name
                    ));
                    continue;
                }
            };
            let edge = DependencyEdge::new(package.name.as_str(), &dependency.name, class);
            if let Some(path) = &dependency.path {
                violations.insert(format!(
                    "nonmember path dependency is not permitted: {edge} at `{path}`; \
                     add the helper as a reviewed workspace member; {GUIDE}"
                ));
            }
            edges.insert(edge);
        }
    }

    (edges, violations)
}

fn validate_policy(
    policies: &[ExternalPackagePolicy],
    workspace_packages: &BTreeSet<&str>,
) -> (BTreeSet<DependencyEdge>, BTreeSet<String>) {
    let mut package_names = BTreeSet::new();
    let mut edges = BTreeSet::new();
    let mut violations = BTreeSet::new();

    for policy in policies {
        if !valid_name(policy.name) {
            violations.insert(format!(
                "external dependency policy has an empty or padded package name; {GUIDE}"
            ));
        }
        if !workspace_packages.contains(policy.name) {
            violations.insert(format!(
                "external dependency policy names unknown workspace package `{}`; {GUIDE}",
                policy.name
            ));
        }
        if !package_names.insert(policy.name) {
            violations.insert(format!(
                "duplicate external dependency policy for `{}`; {GUIDE}",
                policy.name
            ));
        }
        for edge in policy.dependencies() {
            if !valid_name(&edge.dependency) {
                violations.insert(format!(
                    "external dependency rule has an empty or padded dependency name: {edge}; {GUIDE}"
                ));
            }
            if !edges.insert(edge.clone()) {
                violations.insert(format!(
                    "duplicate external dependency rule: {edge}; {GUIDE}"
                ));
            }
        }
    }
    for package in workspace_packages.difference(&package_names) {
        violations.insert(format!(
            "workspace package `{package}` has no external dependency policy; {GUIDE}"
        ));
    }

    (edges, violations)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.trim() == name
}
