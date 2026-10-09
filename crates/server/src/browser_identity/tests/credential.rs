//! Bounded and unambiguous credential parsing.

use super::*;

#[test]
fn cookie_credentials_reject_ambiguity_malformed_values_and_oversized_headers() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        header::COOKIE,
        secret_cookie(SESSION_COOKIE, SESSION_SECRET)
            .parse()
            .unwrap(),
    );
    assert_eq!(
        credential(&headers, SESSION_COOKIE)
            .ok()
            .flatten()
            .as_deref(),
        Some(SESSION_SECRET)
    );
    headers.append(
        header::COOKIE,
        secret_cookie(SESSION_COOKIE, SESSION_SECRET)
            .parse()
            .unwrap(),
    );
    assert!(credential(&headers, SESSION_COOKIE).is_err());
    for value in [
        secret_cookie(SESSION_COOKIE, "wrong"),
        "broken-cookie".to_owned(),
        format!("unrelated={}", "x".repeat(8_193)),
    ] {
        headers.clear();
        headers.insert(header::COOKIE, value.parse().unwrap());
        assert!(credential(&headers, SESSION_COOKIE).is_err());
    }
}
