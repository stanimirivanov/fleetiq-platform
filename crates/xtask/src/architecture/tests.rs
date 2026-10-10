use super::*;

#[test]
fn current_workspace_matches_the_declared_policy() {
    let metadata = load_metadata().expect("workspace metadata should load");
    let graph = graph_from_metadata(&metadata).expect("workspace dependency kinds should be known");
    assert!(protocol_fixture_version_violation(&metadata).is_none());
    source_overrides::check(metadata.workspace_root.as_std_path())
        .expect("repository source overrides should not bypass declared ownership");

    validate_graph(&graph).expect("workspace dependencies should match the architecture policy");
    external_dependencies::check(&metadata)
        .expect("external dependencies should have approved owners");
    lint_inheritance::check(&metadata).expect("every workspace member should inherit shared lints");
    assert!(
        graph
            .dependencies
            .iter()
            .all(|edge| graph.packages.contains(&edge.dependency)),
        "only internal workspace dependencies belong in the architecture graph"
    );
}

#[test]
fn unapproved_production_dependency_is_rejected() {
    let mut graph = declared_graph();
    graph.dependencies.insert(DependencyEdge::new(
        "fleetiq-analytics",
        "fleetiq-telemetry",
        DependencyClass::Normal,
    ));

    let violations = policy_violations(validate_graph(&graph));

    assert_eq!(
        violations,
        vec![
            "unapproved dependency from policy package: \
             fleetiq-analytics -[normal]-> fleetiq-telemetry"
                .to_owned()
        ]
    );
}

#[test]
fn development_exception_does_not_authorize_a_production_edge() {
    let mut graph = declared_graph();
    graph.dependencies.insert(DependencyEdge::new(
        "fleetiq-postgres",
        "fleetiq-protocol-host",
        DependencyClass::Normal,
    ));

    let violations = policy_violations(validate_graph(&graph));

    assert_eq!(
        violations,
        vec![
            "unapproved dependency from adapter package: \
             fleetiq-postgres -[normal]-> fleetiq-protocol-host"
                .to_owned()
        ]
    );
}

#[test]
fn unapproved_development_dependency_is_rejected() {
    let mut graph = declared_graph();
    graph.dependencies.insert(DependencyEdge::new(
        "fleetiq-semantics",
        "fleetiq-protocol-sdk",
        DependencyClass::Development,
    ));

    let violations = policy_violations(validate_graph(&graph));

    assert_eq!(
        violations,
        vec![
            "unapproved dependency from policy package: \
             fleetiq-semantics -[development]-> fleetiq-protocol-sdk"
                .to_owned()
        ]
    );
}

#[test]
fn semantic_policy_cannot_depend_on_application_capability() {
    let mut graph = declared_graph();
    graph.dependencies.insert(DependencyEdge::new(
        "fleetiq-semantics",
        "fleetiq-identity",
        DependencyClass::Normal,
    ));

    let violations = policy_violations(validate_graph(&graph));

    assert_eq!(
        violations,
        vec![
            "unapproved dependency from policy package: \
             fleetiq-semantics -[normal]-> fleetiq-identity"
                .to_owned()
        ]
    );
}

#[test]
fn unclassified_workspace_package_is_rejected() {
    let mut graph = declared_graph();
    graph.packages.insert("fleetiq-new-capability".to_owned());

    let violations = policy_violations(validate_graph(&graph));

    assert_eq!(
        violations,
        vec!["workspace package `fleetiq-new-capability` has no architecture policy".to_owned()]
    );
}

#[test]
fn removed_dependency_makes_its_permanent_rule_stale() {
    let mut graph = declared_graph();
    graph.dependencies.remove(&DependencyEdge::new(
        "fleetiq-server",
        "fleetiq-postgres",
        DependencyClass::Normal,
    ));

    let violations = policy_violations(validate_graph(&graph));

    assert_eq!(
        violations,
        vec!["stale dependency rule: fleetiq-server -[normal]-> fleetiq-postgres".to_owned()]
    );
}

#[test]
fn declared_policy_is_consistent_and_acyclic() {
    assert_eq!(validate_policy(), Vec::<String>::new());
}

#[test]
fn production_cycle_is_detected_independently_of_edge_direction() {
    let packages = BTreeSet::from(["a", "b", "c"]);
    let edges = BTreeSet::from([
        DependencyEdge::new("a", "b", DependencyClass::Normal),
        DependencyEdge::new("b", "c", DependencyClass::Build),
        DependencyEdge::new("c", "a", DependencyClass::Normal),
    ]);

    assert!(has_dependency_cycle(&packages, &edges));
}

fn declared_graph() -> WorkspaceGraph {
    let packages = PACKAGE_POLICIES
        .iter()
        .map(|package| package.name.to_owned())
        .collect();
    let dependencies = PACKAGE_POLICIES
        .iter()
        .flat_map(PackagePolicy::dependencies)
        .chain(
            TEMPORARY_EXCEPTIONS
                .iter()
                .map(super::model::TemporaryException::edge),
        )
        .collect();

    WorkspaceGraph {
        packages,
        dependencies,
    }
}

fn policy_violations(result: Result<ArchitectureReport, ArchitectureCheckError>) -> Vec<String> {
    match result.expect_err("the graph should violate policy") {
        ArchitectureCheckError::Policy(violations) => violations,
        ArchitectureCheckError::Metadata(error) => {
            panic!("unexpected metadata failure: {error}")
        }
    }
}

#[test]
fn protocol_fixture_tools_must_request_the_same_version() {
    assert!(fixture_version_violation(Some("=0.258.0"), Some("=0.258.0")).is_none());
    assert!(fixture_version_violation(Some("^0.258.0"), Some("^0.258.0")).is_some());
    assert!(fixture_version_violation(Some("=0.258"), Some("=0.258")).is_some());

    let violation =
        fixture_version_violation(Some("^0.258.0"), Some("^0.254.0")).expect("versions differ");
    assert!(violation.contains("wit-parser` (^0.258.0)"));
    assert!(violation.contains("wit-component` (^0.254.0)"));
    assert!(fixture_version_violation(None, Some("^0.258.0")).is_some());
}
