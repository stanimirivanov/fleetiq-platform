//! Bounded cookie parsing, cryptographic credentials, and host-only cookie policy.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, header};
use cookie::{Cookie, SameSite};
use fleetiq_domain::UnixTimeMillis;
use sha2::{Digest, Sha256};

use crate::protected_request::ProtectedRequestError;

pub(super) const SESSION_COOKIE: &str = "__Host-fleetiq-session";
pub(super) const LOGIN_COOKIE: &str = "__Host-fleetiq-login";
const MAX_COOKIE_HEADER_BYTES: usize = 8_192;

/// Rejects malformed or duplicate credentials instead of choosing one value.
///
/// The maintained cookie parser handles cookie syntax; this boundary additionally
/// bounds the complete headers and requires our fixed-size opaque hexadecimal
/// credential. Unrelated well-formed cookies may coexist with these cookies.
pub(super) fn credential(
    headers: &HeaderMap,
    name: &str,
) -> Result<Option<String>, ProtectedRequestError> {
    let mut result = None;
    let mut total = 0_usize;
    for value in headers.get_all(header::COOKIE) {
        total = total
            .checked_add(value.as_bytes().len())
            .filter(|total| *total <= MAX_COOKIE_HEADER_BYTES)
            .ok_or(ProtectedRequestError::BrowserUnauthorized)?;
        let text = value
            .to_str()
            .map_err(|_| ProtectedRequestError::BrowserUnauthorized)?;
        for part in text.split(';') {
            let cookie = Cookie::parse(part.trim())
                .map_err(|_| ProtectedRequestError::BrowserUnauthorized)?;
            if cookie.name() == name {
                if result.is_some()
                    || cookie.value().len() != 64
                    || !cookie.value().bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(ProtectedRequestError::BrowserUnauthorized);
                }
                result = Some(cookie.value().to_owned());
            }
        }
    }
    Ok(result)
}

/// Produces 256 bits of OS randomness without using a predictable fallback.
pub(super) fn random_credential() -> Result<String, ProtectedRequestError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| ProtectedRequestError::Internal)?;
    Ok(hex(&bytes))
}

/// Stores an irreversible lookup key rather than the browser's bearer secret.
pub(super) fn digest(value: &str) -> String {
    hex(&Sha256::digest(value.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    output
}

pub(super) fn cookie(name: &'static str, value: String, lifetime_seconds: i64) -> String {
    Cookie::build((name, value))
        .path("/")
        .secure(true)
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(cookie::time::Duration::seconds(lifetime_seconds))
        .build()
        .to_string()
}

pub(super) fn clear_cookie(name: &'static str) -> String {
    cookie(name, String::new(), 0)
}

pub(crate) fn now() -> Result<UnixTimeMillis, ProtectedRequestError> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ProtectedRequestError::Internal)?
        .as_millis();
    let milliseconds = i64::try_from(milliseconds).map_err(|_| ProtectedRequestError::Internal)?;
    Ok(UnixTimeMillis::new(milliseconds))
}

/// Binds sessions to one deployed provider trust domain without storing a secret.
///
/// URL and client validators reject control characters, so newline separators
/// are unambiguous. Rotating the confidential client secret preserves existing
/// sessions; changing issuer, client ID, or authority invalidates them.
pub(super) fn provider_digest(config: &super::BrowserIdentityConfig) -> String {
    digest(&format!(
        "{}\n{}\n{}",
        config.issuer(),
        config.client_id(),
        config.identity_authority().as_str()
    ))
}
