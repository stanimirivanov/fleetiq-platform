//! Published configuration methods and strict command vocabulary.
use serde_json::{Value, json};
#[test]
fn configuration_contract_publishes_protection_and_strict_commands() {
    let spec: Value =
        serde_json::from_str(include_str!("../../../contracts/http/dist/openapi.json")).unwrap();
    for suffix in ["revisions/{revision}", "activation"] {
        let path = format!(
            "/api/v1/tenants/{{tenant_id}}/calculator-configurations/{{configuration_id}}/{suffix}"
        );
        for method in ["get", "put"] {
            let operation = &spec["paths"][&path][method];
            assert_eq!(
                operation["security"],
                json!([{"bearerAuth":[]},{"cookieAuth":[]}])
            );
            assert!(
                operation["description"]
                    .as_str()
                    .unwrap()
                    .contains(if method == "put" {
                        "analytics.manage"
                    } else {
                        "metadata.read"
                    })
            );
            for status in [
                "200",
                "400",
                "401",
                "403",
                "405",
                "500",
                if method == "put" { "409" } else { "404" },
            ] {
                assert!(!operation["responses"][status].is_null());
            }
        }
    }
    let schema = json!({"components":spec["components"],"$ref":"#/components/schemas/CalculatorActivationRequest"});
    let check = jsonschema::validator_for(&schema).unwrap();
    assert!(check.is_valid(&json!({"expected_generation":0,"revision":1})));
    assert!(check.is_valid(&json!({"expected_generation":1,"revision":null})));
    for invalid in [
        json!({"expected_generation":0}),
        json!({"expected_generation":0,"revision":0}),
        json!({"expected_generation":4_294_967_296_u64,"revision":1}),
        json!({"expected_generation":0,"revision":1,"actor_id":"forged"}),
    ] {
        assert!(!check.is_valid(&invalid));
    }
    assert!(
        spec["components"]["schemas"]["TenantAction"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("analytics.manage"))
    );
}
