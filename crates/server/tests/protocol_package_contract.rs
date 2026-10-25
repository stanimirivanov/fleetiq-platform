//! Protocol package authority and wire contracts, independent of PostgreSQL.
use serde_json::{Value, json};

const SPEC: &str = include_str!("../../../contracts/http/dist/openapi.json");
const RELEASE: &str =
    "/api/v1/tenants/{tenant_id}/protocol-packages/{package_id}/versions/{package_version}";
const ADMISSION: &str = "/api/v1/tenants/{tenant_id}/protocol-packages/{package_id}/versions/{package_version}/admissions";

fn resolve<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    value["$ref"].as_str().map_or(value, |reference| {
        spec.pointer(reference.strip_prefix('#').unwrap()).unwrap()
    })
}

#[test]
fn protocol_operations_preserve_security_identity_and_failure_contracts() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    for (path, method, id, statuses) in [
        (
            RELEASE,
            "get",
            "getProtocolPackageRelease",
            &["200", "400", "401", "403", "404", "500"][..],
        ),
        (
            RELEASE,
            "put",
            "registerProtocolPackageArtifact",
            &[
                "200", "201", "400", "401", "403", "404", "409", "413", "415", "500",
            ][..],
        ),
        (
            ADMISSION,
            "post",
            "admitProtocolPackage",
            &[
                "200", "400", "401", "403", "404", "422", "429", "500", "503",
            ][..],
        ),
    ] {
        let operation = &spec["paths"][path][method];
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
        let responses = operation["responses"].as_object().unwrap();
        assert_eq!(responses.len(), statuses.len(), "{id}");
        for status in statuses {
            assert!(responses.contains_key(*status), "{id}: {status}");
        }
        let response = resolve(&spec, &responses["200"]);
        assert_eq!(
            resolve(&spec, &response["headers"]["Cache-Control"])["schema"]["const"],
            "no-store"
        );
        let parameters: Vec<_> = operation["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .map(|parameter| resolve(&spec, parameter))
            .collect();
        for name in ["tenant_id", "package_id", "package_version"] {
            assert!(
                parameters.iter().any(|parameter| parameter["name"] == name
                    && parameter["in"] == "path"
                    && parameter["required"] == true),
                "{id}: {name}"
            );
        }
        for name in ["Origin", "X-FleetIQ-CSRF"] {
            assert_eq!(
                parameters.iter().any(|parameter| parameter["name"] == name),
                method != "get",
                "{id}: {name}"
            );
        }
    }
}

#[test]
fn upload_requires_binary_content_and_a_canonical_digest() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    let operation = &spec["paths"][RELEASE]["put"];
    assert_eq!(operation["requestBody"]["required"], true);
    let content = operation["requestBody"]["content"].as_object().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(
        content["application/wasm"]["schema"],
        json!({"type":"string","format":"binary"})
    );
    let digest = operation["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|parameter| resolve(&spec, parameter))
        .find(|parameter| parameter["name"] == "X-Artifact-Sha256")
        .unwrap();
    assert_eq!(digest["in"], "header");
    assert_eq!(digest["required"], true);
    let validator = jsonschema::validator_for(&digest["schema"]).unwrap();
    assert!(validator.is_valid(&json!("a0".repeat(32))));
    for invalid in [
        json!("A0".repeat(32)),
        json!("a".repeat(63)),
        json!("z".repeat(64)),
        Value::Null,
    ] {
        assert!(!validator.is_valid(&invalid));
    }
}

#[test]
fn admission_requires_an_explicit_world_and_rejects_forged_authority() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    let body = &spec["paths"][ADMISSION]["post"]["requestBody"];
    assert_eq!(body["required"], true);
    assert_eq!(body["content"].as_object().unwrap().len(), 1);
    let schema = resolve(&spec, &body["content"]["application/json"]["schema"]);
    let validator = jsonschema::validator_for(schema).unwrap();
    for world in ["runtime-v1", "identity-v1"] {
        assert!(validator.is_valid(&json!({"world":world})));
    }
    for invalid in [
        json!({}),
        json!({"world":"unknown"}),
        json!({"world":"runtime-v1","activate":true}),
        json!({"world":"runtime-v1","actor_id":"forged"}),
        json!({"world":"runtime-v1","tenant_id":"foreign"}),
    ] {
        assert!(!validator.is_valid(&invalid));
    }
}
