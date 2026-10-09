//! Browser-bound authorization requests and single-use verified callbacks.

use std::sync::Arc;

use axum::{
    extract::{Query, State, rejection::QueryRejection},
    http::HeaderMap,
    response::Response,
};
use fleetiq_access::ActorIdentityResolver;
use fleetiq_domain::UnixTimeMillis;
use fleetiq_postgres::{BrowserLoginAttempt, BrowserSession};
use serde::Deserialize;

use super::super::{
    credential::{
        LOGIN_COOKIE, SESSION_COOKIE, clear_cookie, cookie, credential, digest, now,
        provider_digest, random_credential,
    },
    provider::OidcProvider,
};
use super::{
    BrowserIdentityState, BrowserResponseError, internal, provider_error, redirect,
    reject_authorization, set_cookie,
};

const LOGIN_LIFETIME_MS: i64 = 5 * 60 * 1_000;
const SESSION_LIFETIME_MS: i64 = 8 * 60 * 60 * 1_000;

pub(super) async fn login<P: OidcProvider>(
    State(state): State<Arc<BrowserIdentityState<P>>>,
    headers: HeaderMap,
) -> Result<Response, BrowserResponseError> {
    reject_authorization(&headers)?;
    let _ = credential(&headers, LOGIN_COOKIE)?;
    let login = state.provider.begin_login().map_err(provider_error)?;
    let browser_secret = random_credential()?;
    let current = now()?;
    state
        .repository
        .insert_login_attempt(&BrowserLoginAttempt {
            state_digest: digest(&login.state),
            provider_digest: provider_digest(&state.config),
            browser_digest: digest(&browser_secret),
            pkce_verifier: login.pkce_verifier,
            nonce: login.nonce,
            expires_at: UnixTimeMillis::new(current.get() + LOGIN_LIFETIME_MS),
        })
        .await
        .map_err(internal)?;
    let mut response = redirect(&login.authorization_url)?;
    set_cookie(
        &mut response,
        cookie(LOGIN_COOKIE, browser_secret, LOGIN_LIFETIME_MS / 1_000),
    )?;
    Ok(response)
}

#[derive(Deserialize)]
pub(super) struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub(super) async fn callback<P: OidcProvider>(
    State(state): State<Arc<BrowserIdentityState<P>>>,
    headers: HeaderMap,
    query: Result<Query<CallbackQuery>, QueryRejection>,
) -> Result<Response, BrowserResponseError> {
    reject_authorization(&headers)?;
    let Query(query) = query.map_err(|_| BrowserResponseError::Invalid)?;
    let browser_secret =
        credential(&headers, LOGIN_COOKIE)?.ok_or(BrowserResponseError::Unauthorized)?;
    let oidc_state = query
        .state
        .filter(|value| !value.is_empty() && value.len() <= 512)
        .ok_or(BrowserResponseError::Invalid)?;
    // DELETE RETURNING gives the callback a single-use claim before exchanging
    // the code. Replays and browsers without the matching secret fail closed.
    let attempt = state
        .repository
        .consume_login_attempt(&digest(&oidc_state), &digest(&browser_secret), now()?)
        .await
        .map_err(internal)?
        .ok_or(BrowserResponseError::Unauthorized)?;
    if attempt.provider_digest != provider_digest(&state.config) {
        return Err(BrowserResponseError::Unauthorized);
    }
    if query.error.is_some() {
        return Err(BrowserResponseError::Unauthorized);
    }
    let code = query
        .code
        .filter(|value| !value.is_empty() && value.len() <= 4_096)
        .ok_or(BrowserResponseError::Invalid)?;
    let verified = state
        .provider
        .finish_login(&code, &attempt.pkce_verifier, &attempt.nonce)
        .await
        .map_err(provider_error)?;
    let actor_id = state
        .resolver
        .resolve(&verified.identity)
        .await
        .map_err(internal)?
        .ok_or(BrowserResponseError::Unauthorized)?;
    let current = now()?;
    let expires_at = verified
        .expires_at
        .min(UnixTimeMillis::new(current.get() + SESSION_LIFETIME_MS));
    if expires_at <= current {
        return Err(BrowserResponseError::Unauthorized);
    }
    // Rotate the credential, including when the browser was already signed in.
    // Never adopt a caller-selected cookie as a new session identifier.
    if let Some(previous) = credential(&headers, SESSION_COOKIE)? {
        state
            .repository
            .delete_session(&digest(&previous))
            .await
            .map_err(internal)?;
    }
    let secret = random_credential()?;
    state
        .repository
        .insert_session(&BrowserSession {
            credential_digest: digest(&secret),
            provider_digest: provider_digest(&state.config),
            identity: verified.identity,
            actor_id,
            expires_at,
        })
        .await
        .map_err(internal)?;
    let mut response = redirect("/")?;
    set_cookie(
        &mut response,
        cookie(
            SESSION_COOKIE,
            secret,
            (expires_at.get() - current.get()) / 1_000,
        ),
    )?;
    set_cookie(&mut response, clear_cookie(LOGIN_COOKIE))?;
    Ok(response)
}
