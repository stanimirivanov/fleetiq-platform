//! Public read bounds, exact identifiers/times and independently authorized evidence.
use serde_json::{Value, json};
#[test]
fn alert_reads_publish_bounded_closed_read_only_contracts() {
    let spec: Value =
        serde_json::from_str(include_str!("../../../contracts/http/dist/openapi.json")).unwrap();
    for (suffix, operation, exact) in [
        ("", "listAlerts", false),
        ("/{rule_id}/revisions/{revision}", "getAlert", true),
        (
            "/{rule_id}/revisions/{revision}/transitions",
            "listAlertTransitions",
            true,
        ),
    ] {
        let path = &spec["paths"][format!("/api/v1/tenants/{{tenant_id}}/alerts{suffix}")];
        assert_eq!(path.as_object().unwrap().len(), 1);
        let get = &path["get"];
        assert_eq!(get["operationId"], operation);
        assert_eq!(
            get["security"],
            json!([{"bearerAuth":[]},{"cookieAuth":[]}])
        );
        assert!(get["description"].as_str().unwrap().contains("alerts.read"));
        assert!(
            get["description"]
                .as_str()
                .unwrap()
                .contains("telemetry.read")
        );
        for status in ["200", "400", "401", "403", "405", "500"] {
            assert!(!get["responses"][status].is_null());
        }
        assert_eq!(!get["responses"]["404"].is_null(), exact);
    }
    for (response, schema) in [
        ("AlertPageSucceeded", "AlertPage"),
        ("AlertDetailSucceeded", "AlertDetail"),
        ("AlertTransitionPageSucceeded", "AlertTransitionPage"),
    ] {
        let value =
            &spec["components"]["responses"][response]["content"]["application/json"]["example"];
        let schema = json!({"components":spec["components"],"$ref":format!("#/components/schemas/{schema}")});
        assert!(jsonschema::validator_for(&schema).unwrap().is_valid(value));
    }
    let summary =
        json!({"components":spec["components"],"$ref":"#/components/schemas/AlertSummary"});
    let validate = jsonschema::validator_for(&summary).unwrap();
    let sample = &spec["components"]["responses"]["AlertPageSucceeded"]["content"]["application/json"]
        ["example"]["alerts"][0];
    for (key, value) in [
        ("version", json!(2)),
        ("version", json!("0")),
        ("last_event_at_ms", json!(2000)),
        ("phase", json!("acknowledged")),
        ("rule_revision", json!(0)),
        ("raw_outputs", json!([])),
    ] {
        let mut invalid = sample.clone();
        invalid[key] = value;
        assert!(!validate.is_valid(&invalid), "{key}");
    }
    let evidence =
        json!({"components":spec["components"],"$ref":"#/components/schemas/AlertEvaluationRef"});
    let validate = jsonschema::validator_for(&evidence).unwrap();
    assert!(
        validate.is_valid(&json!({"evaluation_id":"e","scope_asset_id":"a","output_index":null}))
    );
    assert!(
        !validate.is_valid(&json!({"evaluation_id":"e","scope_asset_id":"a","output_index":1024}))
    );
    assert_eq!(
        spec["components"]["schemas"]["AlertPage"]["properties"]["alerts"]["maxItems"],
        100
    );
    assert_eq!(
        spec["components"]["schemas"]["AlertTransitionPage"]["properties"]["transitions"]["maxItems"],
        100
    );
}
