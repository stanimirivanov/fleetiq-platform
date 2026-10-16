//! Additive immutable publication and bounded discovery wire contracts.
use jsonschema::validator_for;
use serde_json::{Value, json};
const SPEC: &str = include_str!("../../../contracts/http/dist/openapi.json");
fn resolve<'a>(spec: &'a Value, value: &'a Value) -> &'a Value {
    value["$ref"].as_str().map_or(value, |r| {
        spec.pointer(r.strip_prefix('#').unwrap()).unwrap()
    })
}
fn validator(spec: &Value, name: &str) -> jsonschema::Validator {
    validator_for(&json!({"components":{"schemas":spec["components"]["schemas"]},"$ref":format!("#/components/schemas/{name}")})).unwrap()
}
#[test]
fn all_definition_administration_operations_publish_exact_permissions_bounds_and_valid_examples() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    for (resource, kind, path_id) in [
        ("asset-types", "AssetType", "type_id"),
        ("properties", "Property", "property_id"),
        ("relationship-types", "RelationshipType", "type_id"),
    ] {
        let collection = format!("/api/v1/tenants/{{tenant_id}}/{resource}");
        let versions = format!("{collection}/{{{path_id}}}/versions");
        for (path, method, id) in [
            (collection.as_str(), "get", format!("list{kind}Definitions")),
            (
                collection.as_str(),
                "post",
                format!("publish{kind}Definition"),
            ),
            (
                versions.as_str(),
                "get",
                format!("list{kind}DefinitionVersions"),
            ),
        ] {
            let op = &spec["paths"][path][method];
            assert_eq!(op["operationId"], id);
            assert_eq!(op["security"], json!([{"bearerAuth":[]},{"cookieAuth":[]}]));
            assert!(
                op["description"]
                    .as_str()
                    .unwrap()
                    .contains(if method == "get" {
                        "metadata.read"
                    } else {
                        "metadata.manage"
                    })
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
                    assert!(
                        validator_for(&schema).unwrap().is_valid(&media["example"]),
                        "{id}"
                    );
                }
            }
            let parameters: Vec<_> = op["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| resolve(&spec, p))
                .collect();
            if method == "post" {
                for name in ["Origin", "X-FleetIQ-CSRF"] {
                    assert!(parameters.iter().any(|p| p["name"] == name));
                }
                for status in ["200", "201", "404", "409"] {
                    assert!(!op["responses"][status].is_null());
                }
                assert!(op["description"].as_str().unwrap().contains("16 KiB"));
            } else {
                let limit = parameters.iter().find(|p| p["name"] == "limit").unwrap();
                assert_eq!(limit["schema"]["minimum"], 1);
                assert_eq!(limit["schema"]["maximum"], 100);
                assert_eq!(limit["schema"]["default"], 50);
                for name in ["after_id", "after_version"] {
                    assert!(parameters.iter().any(|p| p["name"] == name));
                }
            }
        }
    }
}
#[test]
fn publication_requests_reject_forged_context_and_invalid_domain_shapes() {
    let spec: Value = serde_json::from_str(SPEC).unwrap();
    for (name, valid) in [
        (
            "PublishAssetTypeDefinitionRequest",
            json!({"id":"equipment","version":1,"name":"Equipment","supported_properties":[]}),
        ),
        (
            "PublishPropertyDefinitionRequest",
            json!({"id":"temperature","version":1,"name":"Temperature","value_kind":"decimal","canonical_unit":"celsius"}),
        ),
        (
            "PublishRelationshipTypeDefinitionRequest",
            json!({"id":"contains","version":1,"name":"Contains"}),
        ),
    ] {
        let check = validator(&spec, name);
        assert!(check.is_valid(&valid));
        for (field, value) in [
            ("tenant_id", json!("forged")),
            ("actor_id", json!("forged")),
            ("recorded_at_ms", json!(0)),
            ("version", json!(0)),
            ("version", json!(4_294_967_296_u64)),
            ("id", json!(" invalid")),
            ("name", json!("x".repeat(201))),
        ] {
            let mut bad = valid.clone();
            bad[field] = value;
            assert!(!check.is_valid(&bad), "{name} {field}");
        }
    }
    let property = validator(&spec, "PublishPropertyDefinitionRequest");
    assert!(
        property
            .is_valid(&json!({"id":"enabled","version":1,"name":"Enabled","value_kind":"boolean"}))
    );
    assert!(!property.is_valid(&json!({"id":"enabled","version":1,"name":"Enabled","value_kind":"boolean","canonical_unit":"volt"})));
    assert!(
        !property
            .is_valid(&json!({"id":"enabled","version":1,"name":"Enabled","value_kind":"Boolean"}))
    );
    let asset = validator(&spec, "PublishAssetTypeDefinitionRequest");
    assert!(!asset.is_valid(&json!({"id":"equipment","version":1,"name":"Equipment","supported_properties":vec![json!({"id":"p","version":1});101]})));
    assert!(
        !validator(&spec, "DefinitionPublicationResult").is_valid(&json!({"outcome":"updated"}))
    );
}
