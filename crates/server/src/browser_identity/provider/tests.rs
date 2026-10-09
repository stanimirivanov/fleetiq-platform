use std::collections::BTreeMap;

use openidconnect::{
    AccessToken, JsonWebKeyId, PrivateSigningKey,
    core::{
        CoreEdDsaPrivateSigningKey, CoreIdToken, CoreIdTokenClaims, CoreJsonWebKeySet,
        CoreJwsSigningAlgorithm,
    },
};
use serde_json::{Value, json};

use super::*;

// Deterministic fixture key used solely to produce signed, adversarial test
// responses. This is public test material and never a deployment credential.
const TEST_SIGNING_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMC4CAQAwBQYDK2VwBCIEICWeYPLxoZKHZlQ6rkBi11E9JwchynXtljATLqym/XS9\n-----END PRIVATE KEY-----\n";

fn configuration() -> BrowserIdentityConfig {
    BrowserIdentityConfig::from_values(
        "https://fleet.example",
        "https://id.example/realm",
        "fleet-client",
        "secret",
        "workforce",
    )
    .expect("valid trusted deployment")
}

fn signing_key() -> CoreEdDsaPrivateSigningKey {
    CoreEdDsaPrivateSigningKey::from_ed25519_pem(
        TEST_SIGNING_KEY,
        Some(JsonWebKeyId::new("test-key".to_owned())),
    )
    .expect("valid test-only signing key")
}

fn metadata() -> CoreProviderMetadata {
    let metadata: CoreProviderMetadata = serde_json::from_value(json!({
        "issuer": "https://id.example/realm",
        "authorization_endpoint": "https://id.example/authorize",
        "token_endpoint": "https://id.example/token",
        "jwks_uri": "https://id.example/keys",
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["EdDSA"]
    }))
    .expect("valid provider metadata");
    metadata.set_jwks(CoreJsonWebKeySet::new(vec![
        signing_key().as_verification_key(),
    ]))
}

fn claims() -> Value {
    json!({"iss":"https://id.example/realm", "aud":["fleet-client"],
        "exp":4_102_444_800_i64, "iat":1, "sub":"operator-a", "nonce":"expected-nonce"})
}

fn token_response(claims: Value) -> CoreTokenResponse {
    let claims: CoreIdTokenClaims = serde_json::from_value(claims).expect("valid test claims");
    let access = AccessToken::new("provider-access-token".to_owned());
    let token = CoreIdToken::new(
        claims,
        &signing_key(),
        CoreJwsSigningAlgorithm::EdDsa,
        Some(&access),
        None,
    )
    .expect("sign test response");
    response_from_token(&token.to_string(), "provider-access-token")
}

fn response_from_token(token: &str, access: &str) -> CoreTokenResponse {
    serde_json::from_value(json!({"id_token":token, "access_token":access, "token_type":"Bearer"}))
        .expect("valid token envelope")
}

fn verify(tokens: &CoreTokenResponse, nonce: &str) -> Result<OidcIdentity, OidcError> {
    let configuration = configuration();
    let client = client_from_metadata(&configuration, metadata()).expect("valid discovered client");
    verify_identity(&configuration, &client, tokens, nonce)
}

#[test]
fn verifies_real_signature_and_returns_only_registered_subject_and_expiry() {
    let identity =
        verify(&token_response(claims()), "expected-nonce").expect("valid signature and claims");
    assert_eq!(identity.identity.subject().as_str(), "operator-a");
    assert_eq!(identity.identity.authority_id().as_str(), "workforce");
    assert_eq!(identity.expires_at.get(), 4_102_444_800_000);
}

#[test]
fn rejects_wrong_issuer_audience_expired_token_and_wrong_authorized_party() {
    for (name, value) in [
        ("iss", json!("https://other.example/realm")),
        ("aud", json!(["other-client"])),
        ("exp", json!(1)),
        ("azp", json!("other-client")),
        ("sub", json!("")),
    ] {
        let mut candidate = claims();
        candidate[name] = value;
        assert_eq!(
            verify(&token_response(candidate), "expected-nonce").err(),
            Some(OidcError::Rejected),
            "reject {name}"
        );
    }
    assert_eq!(
        verify(&token_response(claims()), "wrong-nonce").err(),
        Some(OidcError::Rejected)
    );
}

#[test]
fn rejects_a_tampered_signature_and_substituted_access_token() {
    let response = token_response(claims());
    let compact = response.id_token().expect("signed token").to_string();
    let mut forged = compact.clone();
    let start = forged.rfind('.').expect("compact token has signature") + 1;
    let replacement = if forged.as_bytes()[start] == b'A' {
        "B"
    } else {
        "A"
    };
    forged.replace_range(start..=start, replacement);
    assert_eq!(
        verify(
            &response_from_token(&forged, "provider-access-token"),
            "expected-nonce"
        )
        .err(),
        Some(OidcError::Rejected)
    );
    assert_eq!(
        verify(
            &response_from_token(&compact, "substituted-access-token"),
            "expected-nonce"
        )
        .err(),
        Some(OidcError::Rejected)
    );
}

#[test]
fn rejects_missing_id_token_and_unsafe_discovered_endpoints() {
    let response: CoreTokenResponse =
        serde_json::from_value(json!({"access_token":"secret", "token_type":"Bearer"}))
            .expect("valid OAuth envelope");
    assert_eq!(
        verify(&response, "expected-nonce").err(),
        Some(OidcError::Rejected)
    );
    let unsafe_metadata = metadata().set_token_endpoint(Some(
        openidconnect::TokenUrl::new("http://id.example/token".to_owned())
            .expect("parse insecure URL"),
    ));
    assert_eq!(
        client_from_metadata(&configuration(), unsafe_metadata).err(),
        Some(OidcError::Operational)
    );
    for endpoint in [
        "http://id.example/token",
        "https://user:secret@id.example/token",
        "https://id.example/token#fragment",
    ] {
        assert!(!secure_endpoint(endpoint));
    }
}

#[test]
fn authorization_request_uses_unique_state_nonce_s256_and_fixed_callback() {
    let configuration = configuration();
    let client = OidcClient {
        client: client_from_metadata(&configuration, metadata()).expect("valid client"),
        configuration,
        http: reqwest::Client::new(),
    };
    let first = client.begin_login().expect("authorization request");
    let second = client.begin_login().expect("independent request");
    let url = url::Url::parse(&first.authorization_url).expect("authorization URL");
    let query: BTreeMap<_, _> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    assert_eq!(query["response_type"], "code");
    assert_eq!(query["scope"], "openid");
    assert_eq!(query["code_challenge_method"], "S256");
    assert_eq!(
        query["redirect_uri"],
        "https://fleet.example/api/v1/auth/callback"
    );
    assert_eq!(query["state"], first.state);
    assert_eq!(query["nonce"], first.nonce);
    assert_ne!(first.state, second.state);
    assert_ne!(first.nonce, second.nonce);
    assert_ne!(first.pkce_verifier, second.pkce_verifier);
    assert!(!first.authorization_url.contains("secret"));
}

#[tokio::test]
async fn refuses_insecure_network_destination_before_contacting_it() {
    let request = openidconnect::http::Request::builder()
        .uri("http://localhost:1/token")
        .body(Vec::new())
        .expect("valid request");
    assert_eq!(
        bounded_request(&reqwest::Client::new(), request)
            .await
            .err(),
        Some(OidcError::Operational)
    );
}

#[test]
fn accepts_discovered_confidential_client_methods_and_rejects_unsupported_methods() {
    let post = metadata().set_token_endpoint_auth_methods_supported(Some(vec![
        CoreClientAuthMethod::ClientSecretPost,
    ]));
    let client =
        client_from_metadata(&configuration(), post).expect("provider supports secret post");
    assert!(matches!(client.auth_type(), AuthType::RequestBody));
    let unsupported = metadata()
        .set_token_endpoint_auth_methods_supported(Some(vec![CoreClientAuthMethod::None]));
    assert_eq!(
        client_from_metadata(&configuration(), unsupported).err(),
        Some(OidcError::Operational)
    );
}
