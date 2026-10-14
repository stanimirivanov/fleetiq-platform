//! Published identity mutation and paging contracts retain security and exact subjects.
use jsonschema::validator_for;
use serde_json::{Value, json};
const SPEC: &str = include_str!("../../../contracts/http/dist/openapi.json");
fn resolve<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    value["$ref"].as_str().map_or(value, |r| {
        spec.pointer(r.strip_prefix('#').unwrap()).unwrap()
    })
}
#[test]
fn all_identity_operations_require_exact_authority_and_publish_bounded_noncached_outcomes() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    for (path, method, id) in [
        (
            "/api/v1/admin/actors/{actor_id}/external-identities",
            "get",
            "listActorExternalIdentities",
        ),
        (
            "/api/v1/admin/actors/{actor_id}/external-identity-changes",
            "post",
            "changeExternalIdentity",
        ),
        (
            "/api/v1/admin/external-identity-changes",
            "get",
            "listExternalIdentityChanges",
        ),
    ] {
        let op = &spec["paths"][path][method];
        assert_eq!(op["operationId"], id);
        assert_eq!(op["security"], json!([{"bearerAuth":[]},{"cookieAuth":[]}]));
        assert!(
            op["description"]
                .as_str()
                .unwrap()
                .contains("identity.manage")
        );
        for status in ["400", "401", "403", "405", "500"] {
            assert!(!op["responses"][status].is_null());
        }
        for response in op["responses"].as_object().unwrap().values() {
            let response = resolve(&spec, response);
            assert_eq!(
                resolve(&spec, &response["headers"]["Cache-Control"])["schema"]["const"],
                "no-store"
            );
            for media in response["content"].as_object().unwrap().values() {
                let schema = json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":media["schema"]["$ref"]});
                assert!(validator_for(&schema).unwrap().is_valid(&media["example"]));
            }
        }
        if method == "post" {
            assert!(!op["responses"]["404"].is_null());
            assert!(!op["responses"]["409"].is_null());
            let parameters: Vec<_> = op["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| resolve(&spec, p))
                .collect();
            for name in ["Origin", "X-FleetIQ-CSRF"] {
                assert!(parameters.iter().any(|p| p["name"] == name));
            }
        } else {
            let parameters: Vec<_> = op["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| resolve(&spec, p))
                .collect();
            let limit = parameters.iter().find(|p| p["name"] == "limit").unwrap();
            assert_eq!(limit["schema"]["minimum"], 1);
            assert_eq!(limit["schema"]["maximum"], 100);
            assert_eq!(limit["schema"]["default"], 50);
        }
    }
}
#[test]
fn mutation_schema_rejects_forged_context_and_preserves_case_spaces_and_unicode() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    let schema = json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":"#/components/schemas/ExternalIdentityChangeRequest"});
    let validator = validator_for(&schema).unwrap();
    let valid = json!({"change_id":"attach-a","kind":"attach","external_identity":{"authority_id":"Recovery","subject":" User+é "}});
    assert!(validator.is_valid(&valid));
    for (field, value) in [
        ("recorded_at_ms", json!(1000)),
        ("administrator_actor_id", json!("forged")),
        ("target_actor_id", json!("other")),
        ("kind", json!("Attach")),
        ("change_id", json!("")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert!(!validator.is_valid(&invalid));
    }
    for subject in [
        String::new(),
        " ".to_owned(),
        "x\u{0000}".to_owned(),
        "é".repeat(513),
    ] {
        let mut invalid = valid.clone();
        invalid["external_identity"]["subject"] = json!(subject);
        assert!(!validator.is_valid(&invalid));
    }
}
