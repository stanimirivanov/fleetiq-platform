//! Review the full administration contract, including mutation schemas and failures.
use jsonschema::validator_for;
use serde_json::{Value, json};
use std::collections::HashSet;
const SPEC: &str = include_str!("../../../contracts/http/dist/openapi.json");
const OPERATIONS: &[(&str, &str, &str)] = &[
    ("/api/v1/admin/actors", "get", "listActors"),
    ("/api/v1/admin/actors", "post", "provisionActor"),
    (
        "/api/v1/admin/actors/{actor_id}/lifecycle",
        "post",
        "changeActorLifecycle",
    ),
    (
        "/api/v1/admin/actor-lifecycle-changes",
        "get",
        "listActorLifecycleChanges",
    ),
];
fn resolve<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    value["$ref"].as_str().map_or(value, |r| {
        spec.pointer(r.strip_prefix('#').unwrap()).unwrap()
    })
}
#[test]
fn administration_operations_have_exact_authority_security_bounds_and_valid_examples() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    let mut ids = HashSet::new();
    for (path, method, id) in OPERATIONS {
        let operation = &spec["paths"][path][method];
        assert_eq!(operation["operationId"], *id);
        assert!(ids.insert(*id));
        assert_eq!(
            operation["security"],
            json!([{"bearerAuth":[]},{"cookieAuth":[]}])
        );
        assert!(
            operation["description"]
                .as_str()
                .unwrap()
                .contains("identity.manage")
        );
        for status in ["400", "401", "403", "405", "500"] {
            assert!(
                !operation["responses"][status].is_null(),
                "{path} {method} {status}"
            );
        }
        for response in operation["responses"].as_object().unwrap().values() {
            let response = resolve(&spec, response);
            assert_eq!(
                resolve(&spec, &response["headers"]["Cache-Control"])["schema"]["const"],
                "no-store"
            );
            for media in response["content"].as_object().unwrap().values() {
                let schema = json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":media["schema"]["$ref"]});
                assert!(
                    validator_for(&schema).unwrap().is_valid(&media["example"]),
                    "{path} {method}: {}",
                    media["example"]
                );
            }
        }
        if *method == "post" {
            assert!(operation["requestBody"]["required"].as_bool().unwrap());
            assert!(!operation["responses"]["409"].is_null());
            let parameters: Vec<_> = operation["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| resolve(&spec, p))
                .collect();
            for name in ["Origin", "X-FleetIQ-CSRF"] {
                assert!(parameters.iter().any(|p| p["name"] == name));
            }
        }
    }
}
#[test]
fn mutation_payloads_reject_forged_audit_context_and_invalid_identifiers() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    let schema = json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":"#/components/schemas/ProvisionActorRequest"});
    let validator = validator_for(&schema).unwrap();
    let valid = json!({"provisioning_id":"command-a","actor_id":"actor-a","external_identity":{"authority_id":"workforce","subject":" User 1 "}});
    assert!(validator.is_valid(&valid));
    for (field, value) in [
        ("recorded_at_ms", json!(1000)),
        ("provisioner_actor_id", json!("admin")),
        ("actor_id", json!(" actor-a")),
        ("provisioning_id", json!("")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert!(!validator.is_valid(&invalid));
    }
}
