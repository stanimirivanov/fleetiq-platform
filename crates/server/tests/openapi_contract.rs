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
    protected: bool,
    can_be_missing: bool,
    snapshot: bool,
}

const OPERATIONS: &[PublishedOperation] = &[
    PublishedOperation {
        path: "/api/v1",
        id: "getApiVersion",
        protected: false,
        can_be_missing: false,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets",
        id: "listAssets",
        protected: true,
        can_be_missing: false,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}",
        id: "getAsset",
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/relationships",
        id: "listAssetRelationships",
        protected: true,
        can_be_missing: true,
        snapshot: true,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/assets/{asset_id}/signal-bindings",
        id: "listAssetSignalBindings",
        protected: true,
        can_be_missing: true,
        snapshot: true,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/signal-bindings",
        id: "listSourceSignalBindings",
        protected: true,
        can_be_missing: false,
        snapshot: true,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/asset-types/{type_id}/versions/{version}",
        id: "getAssetTypeDefinition",
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/properties/{property_id}/versions/{version}",
        id: "getPropertyDefinition",
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
    PublishedOperation {
        path: "/api/v1/tenants/{tenant_id}/relationship-types/{type_id}/versions/{version}",
        id: "getRelationshipTypeDefinition",
        protected: true,
        can_be_missing: true,
        snapshot: false,
    },
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
        OPERATIONS.len() + 4,
        "review every published path"
    );
    let mut ids = HashSet::new();
    for expected in OPERATIONS {
        let path_item = paths
            .get(expected.path)
            .expect("every reviewed path must exist");
        assert!(
            path_item
                .as_object()
                .is_some_and(|item| item.len() == 1 && item.contains_key("get")),
            "{} must publish only GET",
            expected.path
        );
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
            for status in ["400", "401", "403", "500"] {
                assert!(responses.contains_key(status), "{} {status}", expected.path);
            }
            let success = resolve_component(&contract, &operation["responses"]["200"]);
            let cache_control = resolve_component(&contract, &success["headers"]["Cache-Control"]);
            assert_eq!(
                cache_control["schema"]["const"], "no-store",
                "{}",
                expected.path
            );
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
            let media = if status == "200" {
                "application/json"
            } else {
                "application/problem+json"
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
