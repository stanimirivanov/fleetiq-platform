//! Published HTTP contract checks that do not require PostgreSQL.

use std::{collections::HashSet, error::Error};

use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use fleetiq_server::api_router;
use jsonschema::validator_for;
use serde_json::{Value, json};
use tower::ServiceExt;

const SPEC: &str = include_str!("../../../contracts/http/dist/openapi.json");

struct PublishedOperation {
    path: &'static str,
    id: &'static str,
    post_id: Option<&'static str>,
    protected: bool,
    can_be_missing: bool,
    snapshot: bool,
}

const OPERATIONS: &[PublishedOperation] = &[
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/fleet",
        id: "getFleetSnapshot",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/properties/{property_id}/versions/{version}/live",
        id: "streamCurrentProperty",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/properties/{property_id}/versions/{version}/current",
        id: "getCurrentPropertySnapshot",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/semantic-graph",
        id: "exploreSemanticGraph",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/properties/{property_id}/versions/{version}/history",
        id: "listPropertyHistory",
        post_id: None,
        protected: true,
        can_be_missing: false,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/properties/{property_id}/versions/{version}/interpretations/{interpretation}",
        id: "getPropertyInterpretation",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/ingestion-diagnostics",
        id: "getDeviceIngestionDiagnostics",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/me/tenants",
        id: "listOperatorTenants",
        post_id: None,
        protected: true,
        can_be_missing: false,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1",
        id: "getApiVersion",
        post_id: None,
        protected: false,
        can_be_missing: false,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets",
        id: "listAssets",
        post_id: Some("registerAsset"),
        protected: true,
        can_be_missing: false,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}",
        id: "getAsset",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/device",
        id: "getCommunicationDevice",
        post_id: Some("registerCommunicationDevice"),
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/relationships",
        id: "listAssetRelationships",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: true,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/signal-bindings",
        id: "listAssetSignalBindings",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: true,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/signal-bindings",
        id: "listSourceSignalBindings",
        post_id: None,
        protected: true,
        can_be_missing: false,
        snapshot: true,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/asset-types/{type_id}/versions/{version}",
        id: "getAssetTypeDefinition",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/properties/{property_id}/versions/{version}",
        id: "getPropertyDefinition",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/relationship-types/{type_id}/versions/{version}",
        id: "getRelationshipTypeDefinition",
        post_id: None,
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
];

// Explicitly reviewed outside the metadata GET table. Feature-owned contract
// suites validate their payloads; this table prevents unreviewed paths/methods
// or duplicate operation IDs from slipping past the global inventory guard.
const ADDITIONAL_PATHS: &[(&str, &[(&str, &str)])] = &[
    (
        "/api/v1/tenants/{tenant_id}/alerts/{rule_id}/revisions/{revision}/acknowledgement",
        &[("put", "acknowledgeAlert")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/alerts",
        &[("get", "listAlerts")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/alerts/{rule_id}/revisions/{revision}",
        &[("get", "getAlert")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/alerts/{rule_id}/revisions/{revision}/transitions",
        &[("get", "listAlertTransitions")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/alert-rules/{rule_id}",
        &[("get", "getAlertRuleHead")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/alert-rules/{rule_id}/revisions/{revision}",
        &[
            ("get", "getAlertRuleRevision"),
            ("put", "createAlertRuleRevision"),
        ],
    ),
    (
        "/api/v1/tenants/{tenant_id}/alert-rules/{rule_id}/activation",
        &[("put", "changeAlertRuleActivation")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/assets/{asset_id}/calculator-evaluations",
        &[("get", "listCalculatorEvaluations")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/assets/{asset_id}/calculator-evaluations/{evaluation_id}",
        &[("get", "getCalculatorEvaluationEvidence")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/assets/{asset_id}/calculator-evaluations/{evaluation_id}/outputs",
        &[("get", "listCalculatorEvaluationOutputs")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/calculator-configurations/{configuration_id}/revisions/{revision}",
        &[
            ("get", "getCalculatorConfigurationRevision"),
            ("put", "createCalculatorConfigurationRevision"),
        ],
    ),
    (
        "/api/v1/tenants/{tenant_id}/calculator-configurations/{configuration_id}/activation",
        &[
            ("get", "getCalculatorActivation"),
            ("put", "changeCalculatorActivation"),
        ],
    ),
    (
        "/api/v1/tenants/{tenant_id}/assets/{asset_id}/protocol-routes/{channel_id}",
        &[
            ("get", "getDeviceProtocolRoute"),
            ("put", "changeDeviceProtocolRoute"),
        ],
    ),
    (
        "/api/v1/tenants/{tenant_id}/protocol-packages/{package_id}/versions/{package_version}",
        &[
            ("get", "getProtocolPackageRelease"),
            ("put", "registerProtocolPackageArtifact"),
        ],
    ),
    (
        "/api/v1/tenants/{tenant_id}/protocol-packages/{package_id}/versions/{package_version}/admissions",
        &[("post", "admitProtocolPackage")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/relationships/{relationship_id}/revisions/{revision}",
        &[
            ("get", "getRelationshipRevision"),
            ("post", "appendRelationshipRevision"),
        ],
    ),
    (
        "/api/v1/tenants/{tenant_id}/signal-bindings/{binding_id}/revisions/{revision}",
        &[
            ("get", "getSignalBindingRevision"),
            ("post", "appendSignalBindingRevision"),
        ],
    ),
    (
        "/api/v1/tenants/{tenant_id}/asset-types",
        &[
            ("get", "listAssetTypeDefinitions"),
            ("post", "publishAssetTypeDefinition"),
        ],
    ),
    (
        "/api/v1/tenants/{tenant_id}/asset-types/{type_id}/versions",
        &[("get", "listAssetTypeDefinitionVersions")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/properties",
        &[
            ("get", "listPropertyDefinitions"),
            ("post", "publishPropertyDefinition"),
        ],
    ),
    (
        "/api/v1/tenants/{tenant_id}/properties/{property_id}/versions",
        &[("get", "listPropertyDefinitionVersions")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/relationship-types",
        &[
            ("get", "listRelationshipTypeDefinitions"),
            ("post", "publishRelationshipTypeDefinition"),
        ],
    ),
    (
        "/api/v1/tenants/{tenant_id}/relationship-types/{type_id}/versions",
        &[("get", "listRelationshipTypeDefinitionVersions")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/actors/{actor_id}/grants",
        &[("get", "listActorTenantGrants")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/actors/{actor_id}/access-changes",
        &[("post", "changeTenantAccess")],
    ),
    (
        "/api/v1/tenants/{tenant_id}/access-changes",
        &[("get", "listTenantAccessChanges")],
    ),
    (
        "/api/v1/admin/tenants",
        &[("get", "listTenants"), ("post", "provisionTenant")],
    ),
    (
        "/api/v1/admin/tenants/{tenant_id}/lifecycle",
        &[("post", "changeTenantLifecycle")],
    ),
    (
        "/api/v1/admin/tenant-lifecycle-changes",
        &[("get", "listTenantLifecycleChanges")],
    ),
    (
        "/api/v1/admin/actors/{actor_id}/platform-grants",
        &[("get", "listActorPlatformGrants")],
    ),
    (
        "/api/v1/admin/actors/{actor_id}/platform-access-changes",
        &[("post", "changePlatformAccess")],
    ),
    (
        "/api/v1/admin/platform-access-changes",
        &[("get", "listPlatformAccessChanges")],
    ),
    ("/api/v1/auth/login", &[("get", "beginOperatorLogin")]),
    ("/api/v1/auth/callback", &[("get", "completeOperatorLogin")]),
    ("/api/v1/auth/session", &[("get", "getOperatorSession")]),
    ("/api/v1/auth/logout", &[("post", "endOperatorSession")]),
    (
        "/api/v1/admin/actors",
        &[("get", "listActors"), ("post", "provisionActor")],
    ),
    (
        "/api/v1/admin/actors/{actor_id}/lifecycle",
        &[("post", "changeActorLifecycle")],
    ),
    (
        "/api/v1/admin/actor-lifecycle-changes",
        &[("get", "listActorLifecycleChanges")],
    ),
    (
        "/api/v1/admin/actors/{actor_id}/external-identities",
        &[("get", "listActorExternalIdentities")],
    ),
    (
        "/api/v1/admin/actors/{actor_id}/external-identity-changes",
        &[("post", "changeExternalIdentity")],
    ),
    (
        "/api/v1/admin/external-identity-changes",
        &[("get", "listExternalIdentityChanges")],
    ),
];

fn contract() -> Result<Value, serde_json::Error> {
    serde_json::from_str(SPEC)
}

fn resolve_component<'a>(contract: &'a Value, component: &'a Value) -> &'a Value {
    component["$ref"].as_str().map_or(component, |reference| {
        let pointer = reference
            .strip_prefix('#')
            .expect("bundled references must be local");
        contract
            .pointer(pointer)
            .expect("component reference must resolve")
    })
}

fn assert_references_resolve(contract: &Value, value: &Value) {
    match value {
        Value::Object(fields) => {
            if let Some(reference) = fields.get("$ref").and_then(Value::as_str) {
                assert!(
                    contract
                        .pointer(reference.trim_start_matches('#'))
                        .is_some(),
                    "unresolved contract reference: {reference}"
                );
            }
            for child in fields.values() {
                assert_references_resolve(contract, child);
            }
        }
        Value::Array(values) => {
            for child in values {
                assert_references_resolve(contract, child);
            }
        }
        _ => {}
    }
}

fn success_example<'a>(contract: &'a Value, path: &str) -> &'a Value {
    let response = resolve_component(
        contract,
        &contract["paths"][path]["get"]["responses"]["200"],
    );
    &response["content"]["application/json"]["example"]
}

fn validate_component_example(
    contract: &Value,
    component: &str,
    example: &Value,
) -> Result<bool, Box<dyn Error>> {
    let reference = format!("#/components/schemas/{component}");
    let schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "components": { "schemas": contract["components"]["schemas"] },
        "$ref": reference,
    });
    Ok(validator_for(&schema)?.is_valid(example))
}

fn assert_protected_responses(
    contract: &Value,
    responses: &serde_json::Map<String, Value>,
    expected: &PublishedOperation,
) {
    for status in ["400", "401", "403", "500"] {
        if status == "403" && expected.id == "listOperatorTenants" {
            assert!(
                !responses.contains_key(status),
                "self discovery has no tenant authorization denial"
            );
            continue;
        }
        assert!(responses.contains_key(status), "{} {status}", expected.path);
    }
    let success = resolve_component(contract, &responses["200"]);
    let cache_control = resolve_component(contract, &success["headers"]["Cache-Control"]);
    assert_eq!(
        cache_control["schema"]["const"], "no-store",
        "{}",
        expected.path
    );
}

fn assert_additional_paths(
    paths: &serde_json::Map<String, Value>,
    ids: &mut HashSet<&'static str>,
) {
    for (path, operations) in ADDITIONAL_PATHS {
        let path_item = paths.get(*path).expect("every reviewed path must exist");
        assert_eq!(
            path_item.as_object().unwrap().len(),
            operations.len(),
            "{path} has unreviewed path fields or methods"
        );
        for (method, id) in *operations {
            assert_eq!(path_item[method]["operationId"], *id, "{path} {method}");
            assert!(ids.insert(*id), "duplicate operation ID: {id}");
        }
    }
}

fn assert_reviewed_methods(
    path_item: &Value,
    expected: &PublishedOperation,
    ids: &mut HashSet<&'static str>,
) {
    assert!(
        path_item.as_object().is_some_and(|item| {
            item.len() == 1 + usize::from(expected.post_id.is_some())
                && item.contains_key("get")
                && (expected.post_id.is_none() || item.contains_key("post"))
        }),
        "{} must publish exactly its reviewed methods",
        expected.path
    );
    if let Some(post_id) = expected.post_id {
        assert_eq!(
            path_item["post"]["operationId"], post_id,
            "{}",
            expected.path
        );
        assert!(ids.insert(post_id), "duplicate operation ID");
    }
}

#[test]
fn published_operations_have_stable_security_and_request_contracts() -> Result<(), Box<dyn Error>> {
    let contract = contract()?;
    assert_eq!(contract["openapi"], "3.1.1");
    assert_references_resolve(&contract, &contract);

    let paths = contract["paths"]
        .as_object()
        .expect("published paths must be an object");
    assert_eq!(
        paths.len(),
        OPERATIONS.len() + ADDITIONAL_PATHS.len(),
        "review every published path"
    );
    let mut ids = HashSet::new();
    for expected in OPERATIONS {
        let path_item = paths
            .get(expected.path)
            .expect("every reviewed path must exist");
        assert_reviewed_methods(path_item, expected, &mut ids);
        let operation = &path_item["get"];
        assert_eq!(operation["operationId"], expected.id, "{}", expected.path);
        assert!(ids.insert(expected.id), "duplicate operation ID");
        assert_eq!(
            operation["security"],
            if expected.protected {
                json!([{ "bearerAuth": [] }, { "cookieAuth": [] }])
            } else {
                json!([])
            },
            "{}",
            expected.path
        );

        let responses = operation["responses"]
            .as_object()
            .expect("each GET must declare responses");
        assert!(responses.contains_key("200"), "{}", expected.path);
        if expected.protected {
            assert_protected_responses(&contract, responses, expected);
        }
        if expected.can_be_missing {
            assert!(responses.contains_key("404"), "{}", expected.path);
        }
        if expected.snapshot {
            let parameters: Vec<_> = operation["parameters"]
                .as_array()
                .expect("snapshot parameters")
                .iter()
                .map(|parameter| resolve_component(&contract, parameter))
                .collect();
            for name in ["effective_at_ms", "known_at_ms"] {
                assert!(
                    parameters.iter().any(|parameter| parameter["name"] == name
                        && parameter["in"] == "query"
                        && parameter["required"] == true),
                    "{} requires {name}",
                    expected.path
                );
            }
            if expected.id == "listSourceSignalBindings" {
                for name in ["device_asset_id", "endpoint_id", "signal_id"] {
                    assert!(
                        parameters.iter().any(|parameter| parameter["name"] == name
                            && parameter["in"] == "query"
                            && parameter["required"] == true),
                        "{} requires {name}",
                        expected.path
                    );
                }
            }
            let limit = parameters
                .iter()
                .find(|parameter| parameter["name"] == "limit")
                .expect("bounded page size");
            assert_eq!(limit["schema"]["minimum"], 1, "{}", expected.path);
            assert_eq!(limit["schema"]["maximum"], 100, "{}", expected.path);
            assert!(
                parameters
                    .iter()
                    .any(|parameter| parameter["name"] == "after")
            );
        }
    }

    assert_additional_paths(paths, &mut ids);
    assert_eq!(
        contract["components"]["securitySchemes"]["bearerAuth"]["scheme"],
        "bearer"
    );
    Ok(())
}

#[test]
fn published_examples_satisfy_their_response_schemas() -> Result<(), Box<dyn Error>> {
    let contract = contract()?;
    for expected in OPERATIONS {
        let responses = contract["paths"][expected.path]["get"]["responses"]
            .as_object()
            .expect("documented operation must have responses");
        for (status, response) in responses {
            let response = resolve_component(&contract, response);
            let media = match (status.as_str(), expected.id) {
                ("200", "streamCurrentProperty") => "text/event-stream",
                ("200", _) => "application/json",
                _ => "application/problem+json",
            };
            let body = &response["content"][media];
            let schema_ref = body["schema"]["$ref"]
                .as_str()
                .expect("each example must reference a component schema");
            let schema = json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "components": { "schemas": contract["components"]["schemas"] },
                "$ref": schema_ref
            });
            let validator = validator_for(&schema)?;
            assert!(
                validator.is_valid(&body["example"]),
                "{} {status} example violates {schema_ref}",
                expected.path
            );
        }
    }
    Ok(())
}

#[test]
fn published_schemas_reject_semantically_invalid_metadata() -> Result<(), Box<dyn Error>> {
    let contract = contract()?;
    let mut page = success_example(&contract, "/api/v1/tenants/{tenant_id}/assets").clone();
    page["next_after"] = json!(42);
    assert!(!validate_component_example(&contract, "AssetPage", &page)?);

    let mut asset =
        success_example(&contract, "/api/v1/tenants/{tenant_id}/assets/{asset_id}").clone();
    asset["external_identifiers"][0]
        .as_object_mut()
        .expect("sample external identifier")
        .remove("kind");
    assert!(!validate_component_example(&contract, "Asset", &asset)?);

    let mut relationship = success_example(
        &contract,
        "/api/v1/tenants/{tenant_id}/assets/{asset_id}/relationships",
    )
    .clone();
    relationship["relationships"][0]["revision"] = json!(1);
    assert!(!validate_component_example(
        &contract,
        "RelationshipSnapshot",
        &relationship
    )?);

    let mut target_bindings = success_example(
        &contract,
        "/api/v1/tenants/{tenant_id}/assets/{asset_id}/signal-bindings",
    )
    .clone();
    target_bindings["bindings"][0]["source"]
        .as_object_mut()
        .expect("sample source")
        .remove("endpoint_id");
    assert!(!validate_component_example(
        &contract,
        "AssetBindingSnapshot",
        &target_bindings
    )?);

    let mut source_bindings =
        success_example(&contract, "/api/v1/tenants/{tenant_id}/signal-bindings").clone();
    source_bindings["next_after"] = json!(7);
    assert!(!validate_component_example(
        &contract,
        "SourceBindingSnapshot",
        &source_bindings
    )?);

    let mut property = success_example(
        &contract,
        "/api/v1/tenants/{tenant_id}/properties/{property_id}/versions/{version}",
    )
    .clone();
    property["value_kind"] = json!("floating");
    assert!(!validate_component_example(
        &contract,
        "PropertyDefinition",
        &property
    )?);
    Ok(())
}

#[tokio::test]
async fn discovery_response_matches_published_example() -> Result<(), Box<dyn Error>> {
    let response = api_router()
        .oneshot(Request::get("/api/v1").body(Body::empty())?)
        .await?;
    assert_eq!(response.status().as_u16(), 200);
    let actual: Value = serde_json::from_slice(&to_bytes(response.into_body(), 1024).await?)?;
    let contract = contract()?;
    assert_eq!(&actual, success_example(&contract, "/api/v1"));
    Ok(())
}

#[test]
fn operator_identity_operations_publish_the_complete_browser_boundary() -> Result<(), Box<dyn Error>>
{
    let contract = contract()?;
    for (name, method, id, success) in [
        ("login", "get", "beginOperatorLogin", "303"),
        ("callback", "get", "completeOperatorLogin", "303"),
        ("session", "get", "getOperatorSession", "200"),
        ("logout", "post", "endOperatorSession", "204"),
    ] {
        let operation = &contract["paths"][format!("/api/v1/auth/{name}")][method];
        assert_eq!(operation["operationId"], id);
        assert!(operation["responses"].get(success).is_some());
        let response = resolve_component(&contract, &operation["responses"][success]);
        let cache = resolve_component(&contract, &response["headers"]["Cache-Control"]);
        assert_eq!(cache["schema"]["const"], "no-store");
        let policy = resolve_component(&contract, &response["headers"]["Referrer-Policy"]);
        assert_eq!(policy["schema"]["const"], "no-referrer");
        assert_eq!(
            operation["security"],
            if name == "session" {
                json!([{ "cookieAuth": [] }])
            } else {
                json!([])
            }
        );
    }
    let cookie = &contract["components"]["securitySchemes"]["cookieAuth"];
    assert_eq!(cookie["in"], "cookie");
    assert_eq!(cookie["name"], "__Host-fleetiq-session");
    let session = resolve_component(
        &contract,
        &contract["paths"]["/api/v1/auth/session"]["get"]["responses"]["200"],
    );
    let schema = &session["content"]["application/json"]["schema"];
    assert_eq!(schema["$ref"], "#/components/schemas/OperatorSession");
    assert!(validate_component_example(
        &contract,
        "OperatorSession",
        &session["content"]["application/json"]["example"]
    )?);
    let logout = &contract["paths"]["/api/v1/auth/logout"]["post"];
    let parameters: Vec<_> = logout["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| resolve_component(&contract, p))
        .collect();
    assert!(
        parameters
            .iter()
            .any(|p| p["name"] == "Origin" && p["required"] == true)
    );
    assert!(parameters.iter().any(|p| p["name"] == "X-FleetIQ-CSRF"
        && p["schema"]["const"] == "1"
        && p["required"] == true));
    Ok(())
}

#[test]
fn discovery_contract_rejects_invented_or_empty_capabilities() -> Result<(), Box<dyn Error>> {
    let contract = contract()?;
    let valid =
        json!({"tenants":[{"tenant_id":"tenant-a","actions":["metadata.read"]}],"next_after":null});
    assert!(validate_component_example(
        &contract,
        "OperatorTenantPage",
        &valid
    )?);
    assert!(validate_component_example(
        &contract,
        "OperatorTenantPage",
        &json!({"tenants":[],"next_after":null})
    )?);
    for actions in [
        json!([]),
        json!(["platform.access.manage"]),
        json!(["metadata.read", "metadata.read"]),
    ] {
        let mut invalid = valid.clone();
        invalid["tenants"][0]["actions"] = actions;
        assert!(!validate_component_example(
            &contract,
            "OperatorTenantPage",
            &invalid
        )?);
    }
    let operation = &contract["paths"]["/api/v1/me/tenants"]["get"];
    let parameters = operation["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|parameter| resolve_component(&contract, parameter))
        .collect::<Vec<_>>();
    assert_eq!(parameters.len(), 2);
    let limit = parameters
        .iter()
        .find(|parameter| parameter["name"] == "limit")
        .unwrap();
    assert_eq!(limit["schema"]["minimum"], 1);
    assert_eq!(limit["schema"]["maximum"], 100);
    assert_eq!(limit["schema"]["default"], 50);
    assert!(
        parameters
            .iter()
            .any(|parameter| parameter["name"] == "after" && parameter["in"] == "query")
    );
    Ok(())
}

#[test]
fn live_contract_publishes_cursor_limits_and_terminal_event_schemas() -> Result<(), Box<dyn Error>>
{
    let spec = contract()?;
    let path = "/api/v1/tenants/{tenant_id}/assets/{asset_id}/properties/{property_id}/versions/{version}/live";
    let operation = &spec["paths"][path]["get"];
    let parameters = operation["parameters"].as_array().unwrap();
    let cursor = parameters
        .iter()
        .map(|p| resolve_component(&spec, p))
        .find(|p| p["name"] == "Last-Event-ID")
        .unwrap();
    assert_eq!(cursor["in"], "header");
    assert_eq!(cursor["schema"]["maxLength"], 110);
    let response = resolve_component(&spec, &operation["responses"]["200"]);
    let content = &response["content"]["text/event-stream"];
    assert!(content["example"].as_str().unwrap().ends_with("\n\n"));
    let events = &content["x-fleetiq-event-data"];
    assert_eq!(events.as_object().unwrap().len(), 5);
    for name in [
        "snapshot",
        "replace",
        "heartbeat",
        "resnapshot-required",
        "stream-ended",
    ] {
        assert!(resolve_component(&spec, &events[name]).is_object());
    }
    assert!(operation["responses"]["429"].is_object());
    // Unlike the wire string schema, event-data validation checks actual DTOs.
    for event in content["example"]
        .as_str()
        .unwrap()
        .split("\n\n")
        .filter(|e| !e.is_empty())
    {
        let name = event
            .lines()
            .find_map(|line| line.strip_prefix("event: "))
            .unwrap();
        let data = event
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap();
        let reference = events[name]["$ref"].as_str().unwrap();
        let component = reference.strip_prefix("#/components/schemas/").unwrap();
        let data: Value = serde_json::from_str(data)?;
        assert!(validate_component_example(&spec, component, &data)?);
    }
    Ok(())
}

/// Discovery may advertise every exact grant; an old three-action cap hid valid tenants.
#[test]
fn tenant_discovery_accepts_complete_grants_and_rejects_invalid_arrays()
-> Result<(), Box<dyn Error>> {
    let contract = contract()?;
    let actions = contract["components"]["schemas"]["TenantAction"]["enum"]
        .as_array()
        .expect("closed tenant action vocabulary");
    assert_eq!(actions.len(), 9, "review changes to the action vocabulary");
    assert_eq!(
        contract["components"]["schemas"]["OperatorTenant"]["properties"]["actions"]["maxItems"],
        json!(actions.len()),
        "discovery must support the complete existing vocabulary"
    );
    for count in 1..=actions.len() {
        let page = json!({"tenants": [{"tenant_id": "tenant-a", "actions": &actions[..count]}], "next_after": null});
        assert!(
            validate_component_example(&contract, "OperatorTenantPage", &page)?,
            "valid {count}-grant tenant rejected"
        );
    }
    let mut excessive = actions.clone();
    excessive.push(actions[0].clone());
    for invalid in [
        json!([]),
        json!(["metadata.read", "metadata.read"]),
        json!(["unknown.action"]),
        json!(excessive),
    ] {
        let page =
            json!({"tenants": [{"tenant_id": "tenant-a", "actions": invalid}], "next_after": null});
        assert!(
            !validate_component_example(&contract, "OperatorTenantPage", &page)?,
            "invalid action array accepted"
        );
    }
    Ok(())
}
