//! Browser authentication routing and bounded transport response policy.

mod login;
mod session;

use std::sync::Arc;

use axum::{
    Router,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use fleetiq_postgres::{PostgresActorIdentityResolver, PostgresBrowserIdentityRepository};
use sqlx::PgPool;

use crate::protected_request::{BAD_REQUEST, INTERNAL, ProtectedRequestError, problem};

use super::{
    BrowserIdentityConfig, BrowserRequestAuthenticator,
    provider::{OidcError, OidcProvider},
};

struct BrowserIdentityState<P> {
    config: BrowserIdentityConfig,
    provider: P,
    repository: PostgresBrowserIdentityRepository,
    resolver: PostgresActorIdentityResolver,
    authentication: BrowserRequestAuthenticator,
}

/// Mounts the fixed same-origin browser authentication endpoints.
pub(crate) fn postgres_browser_router<P>(
    pool: PgPool,
    config: BrowserIdentityConfig,
    provider: P,
) -> Router
where
    P: OidcProvider + 'static,
{
    let authentication = BrowserRequestAuthenticator::new(pool.clone(), &config);
    let state = Arc::new(BrowserIdentityState {
        config,
        provider,
        repository: PostgresBrowserIdentityRepository::new(pool.clone()),
        resolver: PostgresActorIdentityResolver::new(pool),
        authentication,
    });
    Router::new()
        .route(
            "/api/v1/auth/login",
            get(login::login::<P>).fallback(browser_method_not_allowed),
        )
        .route(
            "/api/v1/auth/callback",
            get(login::callback::<P>).fallback(browser_method_not_allowed),
        )
        .route(
            "/api/v1/auth/session",
            get(session::session::<P>).fallback(browser_method_not_allowed),
        )
        .route(
            "/api/v1/auth/logout",
            post(session::logout::<P>).fallback(browser_logout_method_not_allowed),
        )
        .with_state(state)
}

async fn browser_method_not_allowed() -> Response {
    secure_response(crate::http_api::method_not_allowed().await)
}

async fn browser_logout_method_not_allowed() -> Response {
    let mut response = browser_method_not_allowed().await;
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("POST"));
    response
}

fn reject_authorization(headers: &HeaderMap) -> Result<(), BrowserResponseError> {
    if headers.contains_key(header::AUTHORIZATION) {
        return Err(BrowserResponseError::Unauthorized);
    }
    Ok(())
}

fn single_header(headers: &HeaderMap, name: impl axum::http::header::AsHeaderName) -> Option<&str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

fn provider_error(error: OidcError) -> BrowserResponseError {
    match error {
        OidcError::Rejected => BrowserResponseError::Unauthorized,
        OidcError::Operational => BrowserResponseError::Internal,
    }
}

fn internal<E>(_: E) -> BrowserResponseError {
    BrowserResponseError::Internal
}

fn redirect(location: &str) -> Result<Response, BrowserResponseError> {
    let mut response = secure_response(StatusCode::SEE_OTHER.into_response());
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(location).map_err(internal)?,
    );
    Ok(response)
}

fn set_cookie(response: &mut Response, cookie: String) -> Result<(), BrowserResponseError> {
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::try_from(cookie).map_err(internal)?,
    );
    Ok(())
}

fn secure_response(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

enum BrowserResponseError {
    Invalid,
    Unauthorized,
    Forbidden,
    Internal,
}

impl From<ProtectedRequestError> for BrowserResponseError {
    fn from(error: ProtectedRequestError) -> Self {
        match error {
            ProtectedRequestError::Unauthorized | ProtectedRequestError::BrowserUnauthorized => {
                Self::Unauthorized
            }
            ProtectedRequestError::Forbidden => Self::Forbidden,
            ProtectedRequestError::Internal => Self::Internal,
        }
    }
}

impl IntoResponse for BrowserResponseError {
    fn into_response(self) -> Response {
        let (status, kind, title) = match self {
            Self::Invalid => (StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request"),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "urn:fleetiq:problem:unauthorized",
                "Authentication required",
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "urn:fleetiq:problem:forbidden",
                "Access denied",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL,
                "Internal server error",
            ),
        };
        secure_response(problem(status, kind, title))
    }
}
