//! Credential-free session inspection and exact-origin CSRF-protected logout.

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Serialize;

use super::super::{
    credential::{LOGIN_COOKIE, SESSION_COOKIE, clear_cookie, credential, digest},
    provider::OidcProvider,
};
use super::{
    BrowserIdentityState, BrowserResponseError, internal, reject_authorization, secure_response,
    set_cookie,
};

#[derive(Serialize)]
struct SessionResponse {
    actor_id: String,
    expires_at_ms: i64,
}

pub(super) async fn session<P: OidcProvider>(
    State(state): State<Arc<BrowserIdentityState<P>>>,
    headers: HeaderMap,
) -> Result<Response, BrowserResponseError> {
    let (actor, expires_at_ms) = state.authentication.authenticate_session(&headers).await?;
    Ok(secure_response(
        Json(SessionResponse {
            actor_id: actor.into_actor_id().into_inner(),
            expires_at_ms,
        })
        .into_response(),
    ))
}

pub(super) async fn logout<P: OidcProvider>(
    State(state): State<Arc<BrowserIdentityState<P>>>,
    headers: HeaderMap,
) -> Result<Response, BrowserResponseError> {
    reject_authorization(&headers)?;
    crate::mutation_guard::MutationGuard::Browser {
        public_origin: state.config.public_origin().to_owned(),
    }
    .check(&headers)?;
    if let Some(secret) = credential(&headers, SESSION_COOKIE)? {
        state
            .repository
            .delete_session(&digest(&secret))
            .await
            .map_err(internal)?;
    }
    if let Some(secret) = credential(&headers, LOGIN_COOKIE)? {
        state
            .repository
            .delete_login_attempts(&digest(&secret))
            .await
            .map_err(internal)?;
    }
    let mut response = secure_response(StatusCode::NO_CONTENT.into_response());
    set_cookie(&mut response, clear_cookie(SESSION_COOKIE))?;
    set_cookie(&mut response, clear_cookie(LOGIN_COOKIE))?;
    Ok(response)
}
