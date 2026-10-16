//! Exact revision commands and their previously implemented reads are additive.
use serde_json::{Value, json};
const SPEC: &str = include_str!("../../../contracts/http/dist/openapi.json");
fn resolve<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    value["$ref"].as_str().map_or(value, |r| {
        spec.pointer(r.strip_prefix('#').unwrap()).unwrap()
    })
}
fn validator(spec: &Value, name: &str) -> jsonschema::Validator {
    jsonschema::validator_for(&json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":format!("#/components/schemas/{name}")})).unwrap()
}
#[test]
fn four_operations_publish_exact_security_temporal_retry_and_example_contracts() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    for (resource, id, get, post) in [
        (
            "relationships",
            "relationship_id",
            "getRelationshipRevision",
            "appendRelationshipRevision",
        ),
        (
            "signal-bindings",
            "binding_id",
            "getSignalBindingRevision",
            "appendSignalBindingRevision",
        ),
    ] {
        let path =
            format!("/api/v1/tenants/{{tenant_id}}/{resource}/{{{id}}}/revisions/{{revision}}");
        for (method, operation_id) in [("get", get), ("post", post)] {
            let operation = &spec["paths"][&path][method];
            assert_eq!(operation["operationId"], operation_id);
            assert_eq!(
                operation["security"],
                json!([{"bearerAuth":[]},{"cookieAuth":[]}])
            );
            let description = operation["description"].as_str().unwrap();
            for text in if method == "post" {
                &[
                    "metadata.manage",
                    "32 KiB",
                    "201",
                    "200",
                    "409",
                    "original",
                    "ambiguous",
                    "successor",
                    "recorded time",
                ][..]
            } else {
                &["metadata.read", "exact", "404", "half-open"][..]
            } {
                assert!(description.contains(text), "{operation_id}: {text}");
            }
            let parameters: Vec<_> = operation["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| resolve(&spec, p))
                .collect();
            for name in ["tenant_id", id, "revision"] {
                assert!(
                    parameters
                        .iter()
                        .any(|p| p["name"] == name && p["in"] == "path" && p["required"] == true)
                );
            }
            if method == "post" {
                for name in ["Origin", "X-FleetIQ-CSRF"] {
                    assert!(parameters.iter().any(|p| p["name"] == name));
                }
            }
            for response in operation["responses"].as_object().unwrap().values() {
                let response = resolve(&spec, response);
                assert_eq!(
                    resolve(&spec, &response["headers"]["Cache-Control"])["schema"]["const"],
                    "no-store",
                    "{operation_id} response must publish no-store"
                );
                for media in response["content"].as_object().unwrap().values() {
                    let schema = json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":media["schema"]["$ref"]});
                    assert!(
                        jsonschema::validator_for(&schema)
                            .unwrap()
                            .is_valid(&media["example"])
                    );
                }
            }
        }
    }
}
#[test]
fn closed_nested_commands_reject_forged_context_invalid_types_and_unbounded_references() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    for (name, payload, reference) in [
        (
            "AppendRelationshipRevisionRequest",
            json!({"relationship_type":{"id":"physical.contains","version":1},"source_asset_id":"asset-a","target_asset_id":"asset-b","effective_interval":{"start_ms":0,"end_ms":null}}),
            "relationship_type",
        ),
        (
            "AppendSignalBindingRevisionRequest",
            json!({"source":{"device_asset_id":"asset-a","endpoint_id":"can","signal_id":"bus.voltage"},"target":{"asset_id":"asset-b","property":{"id":"voltage","version":1}},"effective_interval":{"start_ms":0}}),
            "source",
        ),
    ] {
        let check = validator(&spec, name);
        assert!(check.is_valid(&payload));
        for field in [
            "actor_id",
            "tenant_id",
            "recorded_at_ms",
            "id",
            "revision",
            "unknown",
        ] {
            let mut bad = payload.clone();
            bad[field] = json!("forged");
            assert!(!check.is_valid(&bad));
        }
        let mut bad = payload.clone();
        bad[reference]["unknown"] = json!("forged");
        assert!(!check.is_valid(&bad));
        let mut bad = payload.clone();
        bad["effective_interval"]["unknown"] = json!(0);
        assert!(!check.is_valid(&bad));
        let mut bad = payload.clone();
        bad["effective_interval"]["start_ms"] = json!("0");
        assert!(!check.is_valid(&bad));
    }
    let revision = resolve(
        &spec,
        &spec["components"]["parameters"]["exact-record-revision"],
    );
    let check = jsonschema::validator_for(&revision["schema"]).unwrap();
    for valid in ["1", "0001", "18446744073709551615"] {
        assert!(check.is_valid(&json!(valid)));
    }
    for invalid in ["0", "-1", "1.0", " 1", ""] {
        assert!(!check.is_valid(&json!(invalid)));
    }
}
