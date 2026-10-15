//! Review the full administration contract, including mutation schemas and failures.
use jsonschema::validator_for;
use serde_json::{Value, json};
use std::collections::HashSet;
const SPEC: &str = include_str!("../../../contracts/http/dist/openapi.json");
const OPERATIONS: &[(&str, &str, &str)] = &[
    ("/api/v1/admin/tenants", "get", "listTenants"),
    ("/api/v1/admin/tenants", "post", "provisionTenant"),
    (
        "/api/v1/admin/tenants/{tenant_id}/lifecycle",
        "post",
        "changeTenantLifecycle",
    ),
    (
        "/api/v1/admin/tenant-lifecycle-changes",
        "get",
        "listTenantLifecycleChanges",
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
                .contains(if *id == "provisionTenant" {
                    "tenant.provision"
                } else {
                    "tenant.lifecycle.manage"
                })
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
    let schema = json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":"#/components/schemas/ProvisionTenantRequest"});
    let validator = validator_for(&schema).unwrap();
    let valid = json!({"provisioning_id":"command-a","tenant_id":"tenant-a","initial_administrator_actor_id":"admin-a"});
    assert!(validator.is_valid(&valid));
    for (field, value) in [
        ("recorded_at_ms", json!(1000)),
        ("provisioner_actor_id", json!("admin")),
        ("tenant_id", json!(" tenant-a")),
        ("provisioning_id", json!("")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert!(!validator.is_valid(&invalid));
    }
}

#[test]
fn lifecycle_states_are_closed_and_audit_context_is_server_owned() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    let schema = json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":"#/components/schemas/TenantLifecycleRequest"});
    let validator = validator_for(&schema).unwrap();
    for state in ["active", "suspended"] {
        let valid = json!({"change_id":"change-a","state":state});
        assert!(validator.is_valid(&valid));
        for (field, value) in [
            ("state", json!("deactivated")),
            ("tenant_id", json!("forged")),
            ("administrator_actor_id", json!("forged")),
            ("recorded_at_ms", json!(1000)),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert!(!validator.is_valid(&invalid));
        }
    }
}
