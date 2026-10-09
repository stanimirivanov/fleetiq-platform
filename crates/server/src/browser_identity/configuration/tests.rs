use super::{BrowserIdentityConfig, BrowserIdentityConfigError};

fn configuration(
    origin: &str,
    issuer: &str,
) -> Result<BrowserIdentityConfig, BrowserIdentityConfigError> {
    BrowserIdentityConfig::from_values(
        origin,
        issuer,
        "fleet-client",
        "private-secret",
        "workforce",
    )
}

#[test]
fn fixes_callback_and_preserves_exact_issuer_identity() {
    let configuration = configuration(
        "https://fleet.example:443/",
        "https://id.example/realms/fleet/",
    )
    .expect("valid secure deployment");
    assert_eq!(configuration.public_origin(), "https://fleet.example");
    assert_eq!(
        configuration.callback_url(),
        "https://fleet.example/api/v1/auth/callback"
    );
    assert_eq!(configuration.issuer(), "https://id.example/realms/fleet/");
    assert_eq!(configuration.identity_authority().as_str(), "workforce");
}

#[test]
fn rejects_insecure_or_ambiguous_origins() {
    for origin in [
        "http://localhost:5173",
        "https://fleet.example/app",
        "https://user@fleet.example",
        "https://@fleet.example",
        "https://fleet.example/a b",
        "https://fleet.example?redirect=evil",
        "https://fleet.example#fragment",
        " https://fleet.example",
        "https://fleet.example\n",
    ] {
        assert_eq!(
            configuration(origin, "https://id.example").err(),
            Some(BrowserIdentityConfigError::InvalidPublicOrigin)
        );
    }
}

#[test]
fn rejects_insecure_or_ambiguous_issuers() {
    for issuer in [
        "http://id.example",
        "https://user:secret@id.example",
        "https://@id.example",
        "https://id.example/a b",
        "https://id.example?tenant=a",
        "https://id.example#fragment",
        " https://id.example",
        "https://id.example\n",
    ] {
        assert_eq!(
            configuration("https://fleet.example", issuer).err(),
            Some(BrowserIdentityConfigError::InvalidIssuer)
        );
    }
}

#[test]
fn validates_client_credentials_and_registered_authority_without_leaking_secrets() {
    for secret in ["", "\nprivate-secret"] {
        assert_eq!(
            BrowserIdentityConfig::from_values(
                "https://fleet.example",
                "https://id.example",
                "client",
                secret,
                "workforce"
            )
            .err(),
            Some(BrowserIdentityConfigError::InvalidClientCredentials)
        );
    }
    assert_eq!(
        BrowserIdentityConfig::from_values(
            "https://fleet.example",
            "https://id.example",
            "client",
            "secret",
            ""
        )
        .err(),
        Some(BrowserIdentityConfigError::InvalidIdentityAuthority)
    );
    let valid =
        configuration("https://fleet.example", "https://id.example").expect("valid configuration");
    let diagnostic = format!("{valid:?}");
    assert!(!diagnostic.contains("private-secret"));
    assert!(!diagnostic.contains("fleet-client"));
}

#[test]
fn does_not_add_a_slash_to_an_issuer_without_a_path() {
    let without_slash =
        configuration("https://fleet.example", "https://id.example").expect("valid issuer");
    let with_slash =
        configuration("https://fleet.example", "https://id.example/").expect("valid issuer");
    assert_eq!(without_slash.issuer(), "https://id.example");
    assert_eq!(with_slash.issuer(), "https://id.example/");
}
