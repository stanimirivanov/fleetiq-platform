use std::sync::OnceLock;

use cargo_metadata::{Dependency, DependencyKind, Package};

use super::*;

// An explicit reviewed baseline, independent of the metadata being mutated.
const REVIEWED_EXTERNAL_EDGES: usize = 46;

#[test]
fn current_workspace_matches_exact_external_ownership() {
    assert_eq!(
        check(&metadata()).expect("external ownership matches"),
        REVIEWED_EXTERNAL_EDGES
    );
}

#[test]
fn renamed_approved_dependency_keeps_canonical_identity() {
    let mut metadata = metadata();
    dependency_mut(&mut metadata, "fleetiq-domain", "bigdecimal").rename =
        Some("precise_decimal".to_owned());

    assert_eq!(
        check(&metadata).expect("aliases do not change ownership"),
        REVIEWED_EXTERNAL_EDGES
    );
}

#[test]
fn renamed_internal_dependency_is_not_external() {
    let mut metadata = metadata();
    dependency_mut(&mut metadata, "fleetiq-history", "fleetiq-domain").rename =
        Some("domain_values".to_owned());

    assert_eq!(
        check(&metadata).expect("member paths establish identity"),
        REVIEWED_EXTERNAL_EDGES
    );
}

#[test]
fn alias_cannot_hide_an_unapproved_external_dependency() {
    let mut metadata = metadata();
    let mut dependency = dependency_mut(&mut metadata, "fleetiq-postgres", "sqlx").clone();
    dependency.rename = Some("decimal_helpers".to_owned());
    package_mut(&mut metadata, "fleetiq-domain")
        .dependencies
        .push(dependency);

    assert_violation(
        check(&metadata),
        "unapproved external dependency: fleetiq-domain -[normal]-> sqlx",
    );
}

#[test]
fn inactive_optional_target_dependency_is_still_checked() {
    let mut metadata = metadata();
    let mut dependency = dependency_mut(&mut metadata, "fleetiq-protocol-wasm", "wasmtime").clone();
    dependency.optional = true;
    dependency.target = Some(
        "cfg(target_os = \"none\")"
            .parse()
            .expect("valid inactive target"),
    );
    package_mut(&mut metadata, "fleetiq-domain")
        .dependencies
        .push(dependency);

    assert_violation(
        check(&metadata),
        "unapproved external dependency: fleetiq-domain -[normal]-> wasmtime",
    );
}

#[test]
fn development_approval_does_not_permit_normal_or_build_dependencies() {
    for kind in [DependencyKind::Normal, DependencyKind::Build] {
        let mut metadata = metadata();
        dependency_mut(&mut metadata, "fleetiq-access", "tokio").kind = kind;
        let class = super::super::dependency_class(kind).expect("known kind");
        let violations = check(&metadata).expect_err("test ownership does not permit production");

        assert!(violations.iter().any(|violation| {
            violation.contains(&format!(
                "unapproved external dependency: fleetiq-access -[{class}]-> tokio"
            ))
        }));
        assert!(violations.iter().any(|violation| {
            violation
                .contains("stale external dependency rule: fleetiq-access -[development]-> tokio")
        }));
    }
}

#[test]
fn normal_approval_does_not_permit_development_dependency() {
    let mut metadata = metadata();
    dependency_mut(&mut metadata, "fleetiq-domain", "bigdecimal").kind =
        DependencyKind::Development;

    assert_violation(
        check(&metadata),
        "unapproved external dependency: fleetiq-domain -[development]-> bigdecimal",
    );
}

#[test]
fn approved_registry_name_cannot_authorize_nonmember_path_source() {
    let mut metadata = metadata();
    let path = metadata.workspace_root.join("helpers/database");
    let dependency = dependency_mut(&mut metadata, "fleetiq-postgres", "sqlx");
    dependency.path = Some(path);
    dependency.source = None;
    dependency.rename = Some("local_storage".to_owned());

    let violations = assert_violation(
        check(&metadata),
        "nonmember path dependency is not permitted: fleetiq-postgres -[normal]-> sqlx",
    );
    assert_eq!(
        violations.len(),
        1,
        "the source violates an otherwise approved edge"
    );
}

#[test]
fn nonmember_path_cannot_hide_behind_workspace_package_name() {
    let mut metadata = metadata();
    let path = metadata.workspace_root.join("helpers/domain");
    dependency_mut(&mut metadata, "fleetiq-history", "fleetiq-domain").path = Some(path);

    assert_violation(
        check(&metadata),
        "nonmember path dependency is not permitted: fleetiq-history -[normal]-> fleetiq-domain",
    );
}

#[test]
fn registry_homonym_of_workspace_member_is_external() {
    let mut metadata = metadata();
    let mut dependency = dependency_mut(&mut metadata, "fleetiq-domain", "bigdecimal").clone();
    dependency.name = "fleetiq-domain".to_owned();
    package_mut(&mut metadata, "fleetiq-history")
        .dependencies
        .push(dependency);

    assert_violation(
        check(&metadata),
        "unapproved external dependency: fleetiq-history -[normal]-> fleetiq-domain",
    );
}

#[test]
fn removed_dependency_makes_its_ownership_rule_stale() {
    let mut metadata = metadata();
    package_mut(&mut metadata, "fleetiq-domain")
        .dependencies
        .retain(|dependency| dependency.name != "bigdecimal");

    assert_violation(
        check(&metadata),
        "stale external dependency rule: fleetiq-domain -[normal]-> bigdecimal",
    );
}

#[test]
fn unknown_dependency_kind_fails_closed() {
    let mut metadata = metadata();
    dependency_mut(&mut metadata, "fleetiq-domain", "bigdecimal").kind = DependencyKind::Unknown;

    assert_violation(
        check(&metadata),
        "unknown Cargo dependency kind on external dependency fleetiq-domain -> bigdecimal",
    );
}

#[test]
fn unknown_policy_package_is_rejected() {
    let mut policies = PACKAGE_POLICIES.to_vec();
    policies[0].name = "fleetiq-unregistered";

    assert_violation(
        check_with_policy(&metadata(), &policies),
        "external dependency policy names unknown workspace package `fleetiq-unregistered`",
    );
}

#[test]
fn package_with_no_external_dependencies_still_requires_policy() {
    let policies: Vec<_> = PACKAGE_POLICIES
        .iter()
        .copied()
        .filter(|policy| policy.name != "fleetiq-analytics")
        .collect();

    assert_violation(
        check_with_policy(&metadata(), &policies),
        "workspace package `fleetiq-analytics` has no external dependency policy",
    );
}

#[test]
fn duplicate_policy_and_dependency_rules_are_rejected() {
    let mut policies = PACKAGE_POLICIES.to_vec();
    policies.push(
        *policies
            .iter()
            .find(|policy| policy.name == "fleetiq-access")
            .unwrap(),
    );
    policy_mut(&mut policies, "fleetiq-domain").normal = &["bigdecimal", "bigdecimal"];
    let violations = assert_violation(
        check_with_policy(&metadata(), &policies),
        "duplicate external dependency policy for `fleetiq-access`",
    );

    assert!(violations.iter().any(|violation| {
        violation
            .contains("duplicate external dependency rule: fleetiq-domain -[normal]-> bigdecimal")
    }));
}

#[test]
fn empty_and_padded_policy_names_are_rejected() {
    for invalid_dependencies in [
        &[""][..],
        &[" "][..],
        &[" bigdecimal"][..],
        &["bigdecimal "][..],
    ] {
        let mut policies = PACKAGE_POLICIES.to_vec();
        policy_mut(&mut policies, "fleetiq-domain").normal = invalid_dependencies;
        assert_violation(
            check_with_policy(&metadata(), &policies),
            "external dependency rule has an empty or padded dependency name",
        );
    }
    let mut policies = PACKAGE_POLICIES.to_vec();
    policies[0].name = "";
    assert_violation(
        check_with_policy(&metadata(), &policies),
        "external dependency policy has an empty or padded package name",
    );
}

#[test]
fn diagnostics_are_stable_across_metadata_declaration_order() {
    let mut metadata = metadata();
    dependency_mut(&mut metadata, "fleetiq-access", "tokio").kind = DependencyKind::Normal;
    let before = check(&metadata).expect_err("kind change violates policy");
    metadata.packages.reverse();
    for package in &mut metadata.packages {
        package.dependencies.reverse();
    }

    assert_eq!(check(&metadata).expect_err("same violations"), before);
    assert!(before.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(before.iter().all(|violation| violation.contains(GUIDE)));
}

/// Clone Cargo's normalized declarations instead of inventing a second manifest
/// parser. One metadata invocation supplies all mutations without database or
/// dependency resolution; no fixture relies on the active host target.
fn metadata() -> Metadata {
    static METADATA: OnceLock<Metadata> = OnceLock::new();
    METADATA
        .get_or_init(|| super::super::load_metadata().expect("workspace metadata should load"))
        .clone()
}

fn package_mut<'a>(metadata: &'a mut Metadata, name: &str) -> &'a mut Package {
    metadata
        .packages
        .iter_mut()
        .find(|package| package.name == name)
        .expect("fixture package exists")
}

fn dependency_mut<'a>(
    metadata: &'a mut Metadata,
    package: &str,
    dependency: &str,
) -> &'a mut Dependency {
    package_mut(metadata, package)
        .dependencies
        .iter_mut()
        .find(|candidate| candidate.name == dependency)
        .expect("fixture dependency exists")
}

fn policy_mut<'a>(
    policies: &'a mut [ExternalPackagePolicy],
    package: &str,
) -> &'a mut ExternalPackagePolicy {
    policies
        .iter_mut()
        .find(|policy| policy.name == package)
        .expect("policy package exists")
}

fn assert_violation(result: Result<usize, Vec<String>>, expected: &str) -> Vec<String> {
    let violations = result.expect_err("metadata should violate ownership policy");
    assert!(
        violations
            .iter()
            .any(|violation| violation.contains(expected)),
        "expected {expected:?} in {violations:?}"
    );
    assert!(violations.iter().all(|violation| violation.contains(GUIDE)));
    violations
}

#[test]
fn removing_the_stream_dependency_leaves_a_stale_reviewed_edge() {
    let mut metadata = metadata();
    package_mut(&mut metadata, "fleetiq-server")
        .dependencies
        .retain(|dependency| dependency.name != "futures-util");
    assert_violation(
        check(&metadata),
        "stale external dependency rule: fleetiq-server -[normal]-> futures-util",
    );
}

#[test]
fn stream_dependency_approval_does_not_extend_to_projection_policy() {
    let mut metadata = metadata();
    let mut dependency = dependency_mut(&mut metadata, "fleetiq-server", "futures-util").clone();
    dependency.rename = Some("application_events".to_owned());
    package_mut(&mut metadata, "fleetiq-projection")
        .dependencies
        .push(dependency);
    assert_violation(
        check(&metadata),
        "unapproved external dependency: fleetiq-projection -[normal]-> futures-util",
    );
}
