//! Versioned rule configuration publishes independent permission/grammar bounds.
use serde_json::{Value, json};
#[test]
fn rule_contract_requires_exact_actions_and_a_closed_bounded_grammar() {
    let spec: Value =
        serde_json::from_str(include_str!("../../../contracts/http/dist/openapi.json")).unwrap();
    let base = "/api/v1/tenants/{tenant_id}/alert-rules/{rule_id}";
    for (suffix, methods) in [
        ("", vec!["get"]),
        ("/revisions/{revision}", vec!["get", "put"]),
        ("/activation", vec!["put"]),
    ] {
        let path = &spec["paths"][format!("{base}{suffix}")];
        assert_eq!(path.as_object().unwrap().len(), methods.len());
        for method in methods {
            let op = &path[method];
            assert_eq!(op["security"], json!([{"bearerAuth":[]},{"cookieAuth":[]}]));
            assert!(
                op["description"]
                    .as_str()
                    .unwrap()
                    .contains(if method == "get" {
                        "alerts.read"
                    } else {
                        "alerts.manage"
                    })
            );
            for status in [
                "200",
                "400",
                "401",
                "403",
                "405",
                "500",
                if method == "get" { "404" } else { "409" },
            ] {
                assert!(!op["responses"][status].is_null());
            }
        }
    }
    let schema =
        json!({"components":spec["components"],"$ref":"#/components/schemas/AlertRuleDefinition"});
    let check = jsonschema::validator_for(&schema).unwrap();
    let example = &spec["components"]["responses"]["AlertRuleRevisionSucceeded"]["content"]["application/json"]
        ["example"]["definition"];
    assert!(check.is_valid(example));
    for (key, value) in [
        ("kind", json!("promql")),
        ("comparison", json!("eval")),
        ("threshold", json!(50)),
        ("resolution", json!("manual")),
        ("hold_ms", json!(604_800_001)),
        ("actor_id", json!("forged")),
    ] {
        let mut bad = example.clone();
        bad[key] = value;
        assert!(!check.is_valid(&bad), "{key}");
    }
    let mut missing = example.clone();
    missing.as_object_mut().unwrap().remove("threshold_unit");
    assert!(!check.is_valid(&missing));
}
