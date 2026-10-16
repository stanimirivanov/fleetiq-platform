//! Additive create-only metadata wire contracts and negative schema boundaries.
use serde_json::{Value, json};
const SPEC: &str = include_str!("../../../contracts/http/dist/openapi.json");
fn resolve<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    value["$ref"].as_str().map_or(value, |r| {
        spec.pointer(r.strip_prefix('#').unwrap()).unwrap()
    })
}
fn validator(spec: &Value, name: &str) -> jsonschema::Validator {
    jsonschema::validator_for(
        &json!({"components":{"schemas":spec["components"]["schemas"]},
        "$ref":format!("#/components/schemas/{name}")}),
    )
    .unwrap()
}
#[test]
fn operations_publish_exact_authority_retry_security_and_bounded_request_contracts() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    for (path, id) in [
        ("/api/v1/tenants/{tenant_id}/assets", "registerAsset"),
        (
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}/device",
            "registerCommunicationDevice",
        ),
    ] {
        let operation = &spec["paths"][path]["post"];
        assert_eq!(operation["operationId"], id);
        assert_eq!(
            operation["security"],
            json!([{"bearerAuth":[]},{"cookieAuth":[]}])
        );
        let description = operation["description"].as_str().unwrap();
        for text in [
            "metadata.manage",
            "32 KiB",
            "201",
            "200",
            "409",
            "original",
            "ambiguous",
            "100",
        ] {
            assert!(description.contains(text), "{id}: {text}");
        }
        let parameters: Vec<_> = operation["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| resolve(&spec, p))
            .collect();
        for name in ["tenant_id", "Origin", "X-FleetIQ-CSRF"] {
            assert!(parameters.iter().any(|p| p["name"] == name));
        }
        for status in [
            "200", "201", "400", "401", "403", "404", "405", "409", "500",
        ] {
            let response = resolve(&spec, &operation["responses"][status]);
            assert_eq!(
                resolve(&spec, &response["headers"]["Cache-Control"])["schema"]["const"],
                "no-store"
            );
            for media in response["content"].as_object().unwrap().values() {
                let schema = json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":media["schema"]["$ref"]});
                assert!(
                    jsonschema::validator_for(&schema)
                        .unwrap()
                        .is_valid(&media["example"]),
                    "{id}: {status}"
                );
            }
        }
    }
    let get = &spec["paths"]["/api/v1/tenants/{tenant_id}/assets/{asset_id}/device"]["get"];
    assert_eq!(get["operationId"], "getCommunicationDevice");
    assert!(
        get["description"]
            .as_str()
            .unwrap()
            .contains("metadata.read")
    );
    let response = resolve(&spec, &get["responses"]["200"]);
    assert!(
        validator(&spec, "CommunicationDevice")
            .is_valid(&response["content"]["application/json"]["example"])
    );
}
#[test]
fn closed_requests_reject_forged_context_invalid_identifiers_and_collection_overflow() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    let asset = json!({"id":"asset-a","name":"Machine","asset_type":{"id":"equipment","version":1},
        "external_identifiers":[{"kind":"imei","authority":null,"value":"350000000000001"}]});
    let check = validator(&spec, "RegisterAssetRequest");
    assert!(check.is_valid(&asset));
    for (field, value) in [
        ("tenant_id", json!("forged")),
        ("actor_id", json!("forged")),
        ("recorded_at_ms", json!(0)),
        ("id", json!(" bad")),
        ("name", json!("x".repeat(201))),
        (
            "external_identifiers",
            json!(vec![asset["external_identifiers"][0].clone(); 101]),
        ),
    ] {
        let mut bad = asset.clone();
        bad[field] = value;
        assert!(!check.is_valid(&bad), "{field}");
    }
    let mut bad = asset.clone();
    bad["asset_type"]["version"] = json!(0);
    assert!(!check.is_valid(&bad));
    bad["asset_type"]["version"] = json!(4_294_967_296_u64);
    assert!(!check.is_valid(&bad));
    bad = asset.clone();
    bad["external_identifiers"][0]["authority"] = json!("");
    assert!(!check.is_valid(&bad));
    let mut omitted = asset.clone();
    omitted["external_identifiers"][0]
        .as_object_mut()
        .unwrap()
        .remove("authority");
    assert!(check.is_valid(&omitted));
    let device = json!({"endpoints":[{"id":"can","kind":"bus.can","name":"CAN"}]});
    let check = validator(&spec, "RegisterCommunicationDeviceRequest");
    assert!(check.is_valid(&device));
    for bad in [
        json!({"endpoints":[],"asset_id":"forged"}),
        json!({"endpoints":vec![device["endpoints"][0].clone();101]}),
        json!({"endpoints":[{"id":"can","kind":"bus.can","name":"CAN","protocol":"forged"}]}),
    ] {
        assert!(!check.is_valid(&bad));
    }
}
