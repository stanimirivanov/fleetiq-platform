//! Evidence operations retain explicit authorization, limits and no-store contracts.
use serde_json::{Value, json};
#[test]
fn evidence_contract_is_bounded_read_only_and_exact() {
    let spec: Value =
        serde_json::from_str(include_str!("../../../contracts/http/dist/openapi.json")).unwrap();
    let base = "/api/v1/tenants/{tenant_id}/assets/{asset_id}/calculator-evaluations";
    for suffix in ["", "/{evaluation_id}", "/{evaluation_id}/outputs"] {
        let path = &spec["paths"][format!("{base}{suffix}")];
        assert_eq!(path.as_object().unwrap().len(), 1);
        let op = &path["get"];
        assert_eq!(op["security"], json!([{"bearerAuth":[]},{"cookieAuth":[]}]));
        assert!(
            op["description"]
                .as_str()
                .unwrap()
                .contains("telemetry.read")
        );
        for status in ["200", "400", "401", "403", "405", "500"] {
            assert!(!op["responses"][status].is_null());
        }
        if !suffix.is_empty() {
            assert!(!op["responses"]["404"].is_null());
        }
    }
    for (name, field, cap) in [
        ("CalculatorEvaluationPage", "evaluations", 100),
        ("CalculatorOutputPage", "outputs", 20),
    ] {
        assert_eq!(
            spec["components"]["schemas"][name]["properties"][field]["maxItems"],
            cap
        );
    }
    assert_eq!(
        spec["components"]["schemas"]["CalculatorEvaluationEvidence"]["properties"]["inputs"]["maxItems"],
        1024
    );
    let schema = json!({"components":spec["components"],"$ref":"#/components/schemas/CalculatorInputReference"});
    let check = jsonschema::validator_for(&schema).unwrap();
    assert!(
        check.is_valid(&json!({"evidence_id":"a".repeat(64),"event_at_ms":"9223372036854775807"}))
    );
    assert!(!check.is_valid(
        &json!({"evidence_id":"a".repeat(64),"event_at_ms":9_223_372_036_854_775_807_i64})
    ));
}
