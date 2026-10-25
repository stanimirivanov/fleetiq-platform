//! Protocol commissioning schemas and reviewed operations need no database.
use serde_json::{Value, json};
const SPEC: &str = include_str!("../../../contracts/http/dist/openapi.json");
const PATH: &str = "/api/v1/tenants/{tenant_id}/assets/{asset_id}/protocol-routes/{channel_id}";
fn resolve<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    value["$ref"].as_str().map_or(value, |reference| {
        spec.pointer(reference.strip_prefix('#').unwrap()).unwrap()
    })
}
fn validator(spec: &Value, name: &str) -> jsonschema::Validator {
    jsonschema::validator_for(&json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":format!("#/components/schemas/{name}")})).unwrap()
}
#[test]
fn route_operations_publish_protected_exact_methods_and_valid_examples() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    assert_eq!(spec["paths"][PATH].as_object().unwrap().len(), 2);
    for (method, id) in [
        ("get", "getDeviceProtocolRoute"),
        ("put", "changeDeviceProtocolRoute"),
    ] {
        let operation = &spec["paths"][PATH][method];
        assert_eq!(operation["operationId"], id);
        assert_eq!(
            operation["security"],
            json!([{"bearerAuth":[]},{"cookieAuth":[]}])
        );
        assert!(
            operation["description"]
                .as_str()
                .unwrap()
                .contains("protocol.manage")
        );
        let parameters: Vec<_> = operation["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| resolve(&spec, value))
            .collect();
        for name in ["tenant_id", "asset_id", "channel_id"] {
            assert!(parameters.iter().any(|value| value["name"] == name
                && value["in"] == "path"
                && value["required"] == true));
        }
        for name in ["Origin", "X-FleetIQ-CSRF"] {
            assert_eq!(
                parameters.iter().any(|value| value["name"] == name),
                method == "put"
            );
        }
        for status in [
            "200",
            "400",
            "401",
            "403",
            "500",
            if method == "get" { "404" } else { "409" },
        ] {
            let response = resolve(&spec, &operation["responses"][status]);
            assert!(!response.is_null());
            assert_eq!(
                resolve(&spec, &response["headers"]["Cache-Control"])["schema"]["const"],
                "no-store"
            );
        }
        let response = resolve(&spec, &operation["responses"]["200"]);
        assert!(
            validator(&spec, "ProtocolDeviceRoute")
                .is_valid(&response["content"]["application/json"]["example"])
        );
    }
}
#[test]
fn requests_require_explicit_selection_and_reject_forged_authority_or_overflow() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    let request = &spec["paths"][PATH]["put"]["requestBody"];
    assert_eq!(request["required"], true);
    let example = &request["content"]["application/json"]["example"];
    let check = validator(&spec, "ChangeProtocolRouteRequest");
    assert!(check.is_valid(example));
    assert!(check.is_valid(&json!({"expected_revision":1,"selection":null})));
    for invalid in [
        json!({"expected_revision":0}),
        json!({"expected_revision":-1,"selection":null}),
        json!({"expected_revision":4_294_967_296_u64,"selection":null}),
        json!({"expected_revision":0,"selection":null,"tenant_id":"forged"}),
        json!({"expected_revision":0,"selection":{"package_id":"custom","package_version":"1","world":"identity-v1","policy_version":1}}),
    ] {
        assert!(!check.is_valid(&invalid));
    }
    let channel = resolve(
        &spec,
        &spec["components"]["parameters"]["protocol-channel-id"],
    );
    let check = jsonschema::validator_for(&channel["schema"]).unwrap();
    for invalid in ["bad/channel".into(), "é".into(), "x".repeat(129)] {
        assert!(!check.is_valid(&json!(invalid)));
    }
}
#[test]
fn retired_response_cannot_claim_an_active_admission_policy() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    let response = resolve(&spec, &spec["paths"][PATH]["get"]["responses"]["200"]);
    let mut value = response["content"]["application/json"]["example"].clone();
    let check = validator(&spec, "ProtocolDeviceRoute");
    value["selection"] = Value::Null;
    assert!(!check.is_valid(&value));
    value["policy_version"] = Value::Null;
    assert!(check.is_valid(&value));
    value["forged"] = json!(true);
    assert!(!check.is_valid(&value));
    value.as_object_mut().unwrap().remove("forged");
    value["revision"] = json!(0);
    assert!(!check.is_valid(&value));
}
