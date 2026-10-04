//! Published HTTP contract checks that do not require PostgreSQL.

use std::error::Error;

use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use fleetiq_server::api_router;
use jsonschema::validator_for;
use serde_json::{Value, json};
use tower::ServiceExt;

const SPEC: &str = include_str!("../../../contracts/http/fleetiq-v1-ui-baseline.openapi.json");

fn contract() -> Result<Value, serde_json::Error> {
    serde_json::from_str(SPEC)
}

fn resolve_response<'a>(contract: &'a Value, response: &'a Value) -> &'a Value {
    response["$ref"].as_str().map_or(response, |reference| {
        contract
            .pointer(reference.trim_start_matches('#'))
            .expect("response reference must resolve")
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

#[test]
fn published_examples_satisfy_their_response_schemas() -> Result<(), Box<dyn Error>> {
    let contract = contract()?;
    assert_eq!(contract["openapi"], "3.1.1");
    assert_references_resolve(&contract, &contract);

    for (path, operation) in [
        ("/api/v1", "get"),
        ("/api/v1/tenants/{tenant_id}/assets", "get"),
    ] {
        let responses = contract["paths"][path][operation]["responses"]
            .as_object()
            .expect("documented operation must have responses");
        for (status, response) in responses {
            let response = resolve_response(&contract, response);
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
                "{path} {status} example violates {schema_ref}"
            );
        }
    }

    let page_schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "components": { "schemas": contract["components"]["schemas"] },
        "$ref": "#/components/schemas/AssetPage"
    });
    let page_validator = validator_for(&page_schema)?;
    assert!(!page_validator.is_valid(&json!({ "assets": [], "next_after": 42 })));
    assert!(!page_validator.is_valid(&json!({ "assets": [{}], "next_after": null })));
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
    assert_eq!(
        actual,
        contract["paths"]["/api/v1"]["get"]["responses"]["200"]["content"]["application/json"]["example"]
    );
    Ok(())
}
