//! Acknowledgement is an exact-version operator annotation, never condition state.
use serde_json::{Value, json};
#[test]
fn acknowledgement_contract_preserves_independent_authority_and_exact_versions() {
    let spec: Value =
        serde_json::from_str(include_str!("../../../contracts/http/dist/openapi.json")).unwrap();
    let path = &spec["paths"]["/api/v1/tenants/{tenant_id}/alerts/{rule_id}/revisions/{revision}/acknowledgement"];
    assert_eq!(path.as_object().unwrap().len(), 1);
    let operation = &path["put"];
    assert_eq!(operation["operationId"], "acknowledgeAlert");
    assert_eq!(
        operation["security"],
        json!([{"bearerAuth":[]},{"cookieAuth":[]}])
    );
    assert!(
        operation["description"]
            .as_str()
            .unwrap()
            .contains("alerts.acknowledge")
    );
    for code in ["200", "400", "401", "403", "405", "409", "500"] {
        assert!(!operation["responses"][code].is_null());
    }
    assert!(operation["responses"]["404"].is_null());
    let schema = json!({"components":spec["components"],"$ref":"#/components/schemas/AlertAcknowledgementRequest"});
    let validate = jsonschema::validator_for(&schema).unwrap();
    assert!(validate.is_valid(&json!({"expected_version":"9007199254740993"})));
    for invalid in [
        json!({}),
        json!({"expected_version":1}),
        json!({"expected_version":"0"}),
        json!({"expected_version":"01"}),
        json!({"expected_version":"1","actor_id":"forged"}),
    ] {
        assert!(!validate.is_valid(&invalid));
    }
    let schema =
        json!({"components":spec["components"],"$ref":"#/components/schemas/AlertAcknowledgement"});
    let example = &spec["components"]["responses"]["AlertAcknowledgementSucceeded"]["content"]["application/json"]
        ["example"];
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(example)
    );
    let transition = &spec["components"]["responses"]["AlertTransitionPageSucceeded"]["content"]["application/json"]
        ["example"]["transitions"][0];
    let schema =
        json!({"components":spec["components"],"$ref":"#/components/schemas/AlertTransition"});
    let validate = jsonschema::validator_for(&schema).unwrap();
    assert!(validate.is_valid(transition));
    let mut annotated = transition.clone();
    annotated["acknowledgement"] = example.clone();
    assert!(validate.is_valid(&annotated));
    annotated["phase"] = json!("acknowledged");
    assert!(!validate.is_valid(&annotated));
    assert!(
        spec["components"]["schemas"]["TenantAction"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("alerts.acknowledge"))
    );
    assert_eq!(
        spec["components"]["schemas"]["OperatorTenant"]["properties"]["actions"]["maxItems"],
        9
    );
}
