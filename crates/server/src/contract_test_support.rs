//! Shared test-only assertions for the backend-owned UI contract.

use serde_json::Value;

const SPEC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contracts/http/dist/openapi.json"
));

/// Compares a deterministic HTTP handler response with the published 200 example.
pub(crate) fn assert_published_example(actual: &[u8], path: &str) {
    let spec: Value = serde_json::from_str(SPEC).expect("valid published OpenAPI");
    let actual: Value = serde_json::from_slice(actual).expect("valid HTTP JSON response");
    let response = &spec["paths"][path]["get"]["responses"]["200"];
    let response = response["$ref"].as_str().map_or(response, |reference| {
        let pointer = reference
            .strip_prefix('#')
            .expect("bundled references must be local");
        spec.pointer(pointer)
            .expect("published response reference must resolve")
    });
    assert_eq!(
        actual, response["content"]["application/json"]["example"],
        "{path}"
    );
}
