//! HTTP-only authentication, authorization, and safe response mapping shared by protected reads.

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use fleetiq_access::{
    ActorIdentityResolver, AuthenticatedActor, AuthorizedTenant, CallerAuthenticationError,
    CallerAuthenticationService, ExternalIdentityAuthenticator, ReadMetadata, TenantAccessPolicy,
    TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_domain::{RecordRevision, TenantId};

use crate::http_api::{ApiProblem, method_not_allowed};

pub(crate) const BAD_REQUEST: &str = "urn:fleetiq:problem:invalid-request";
pub(crate) const FORBIDDEN: &str = "urn:fleetiq:problem:forbidden";
pub(crate) const NOT_FOUND: &str = "urn:fleetiq:problem:not-found";
pub(crate) const INTERNAL: &str = "urn:fleetiq:problem:internal-error";
const UNAUTHORIZED: &str = "urn:fleetiq:problem:unauthorized";

/// Bounded transport outcomes from the shared protected-read boundary.
pub(crate) enum ProtectedRequestError {
    Unauthorized,
    Forbidden,
    Internal,
}

impl IntoResponse for ProtectedRequestError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized => challenge(),
            Self::Forbidden => problem(StatusCode::FORBIDDEN, FORBIDDEN, "Access denied"),
            Self::Internal => problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL,
                "Internal server error",
            ),
        }
    }
}

/// Validates the bearer credential before any caller-selected ID reaches policy.
pub(crate) async fn authenticate<A, R>(
    headers: &HeaderMap,
    service: &CallerAuthenticationService<A, R>,
) -> Result<AuthenticatedActor, ProtectedRequestError>
where
    A: ExternalIdentityAuthenticator<Credential = String>,
    R: ActorIdentityResolver,
{
    let Some(credential) = bearer_credential(headers) else {
        return Err(ProtectedRequestError::Unauthorized);
    };
    match service.authenticate(&credential).await {
        Ok(actor) => Ok(actor),
        Err(CallerAuthenticationError::Rejected | CallerAuthenticationError::Unregistered) => {
            Err(ProtectedRequestError::Unauthorized)
        }
        Err(
            CallerAuthenticationError::Authenticator(_) | CallerAuthenticationError::Resolver(_),
        ) => Err(ProtectedRequestError::Internal),
    }
}

/// Checks the current exact metadata-read grant and returns consumable evidence.
pub(crate) async fn authorize_metadata_read<P>(
    actor: AuthenticatedActor,
    tenant: TenantId,
    service: &TenantAuthorizationService<P>,
) -> Result<AuthorizedTenant<ReadMetadata>, ProtectedRequestError>
where
    P: TenantAccessPolicy,
{
    match service.authorize::<ReadMetadata>(actor, tenant).await {
        Ok(grant) => Ok(grant),
        Err(TenantAuthorizationError::Denied) => Err(ProtectedRequestError::Forbidden),
        Err(TenantAuthorizationError::Policy(_)) => Err(ProtectedRequestError::Internal),
    }
}

fn bearer_credential(headers: &HeaderMap) -> Option<String> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.len() != 64
        || !token.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(token.to_owned())
}

fn challenge() -> Response {
    let mut response = problem(
        StatusCode::UNAUTHORIZED,
        UNAUTHORIZED,
        "Authentication required",
    );
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"FleetIQ\""),
    );
    response
}

pub(crate) async fn protected_method_not_allowed() -> Response {
    no_store(method_not_allowed().await)
}

pub(crate) fn problem(status: StatusCode, kind: &'static str, title: &'static str) -> Response {
    no_store(ApiProblem::new(status, kind, title).into_response())
}

pub(crate) fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Parses the common positive decimal revision path contract for protected reads.
pub(crate) fn parse_record_revision(value: &str) -> Option<RecordRevision> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value
        .parse::<u64>()
        .ok()
        .and_then(|revision| RecordRevision::new(revision).ok())
}
