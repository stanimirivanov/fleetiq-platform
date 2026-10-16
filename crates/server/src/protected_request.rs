//! HTTP-only authentication, authorization, and safe response mapping shared by protected operations.

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

/// Bounded transport outcomes from the shared protected-operation boundary.
#[derive(Debug)]
pub(crate) enum ProtectedRequestError {
    Unauthorized,
    BrowserUnauthorized,
    Forbidden,
    Internal,
}

impl IntoResponse for ProtectedRequestError {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthorized => challenge(),
            Self::BrowserUnauthorized => problem(
                StatusCode::UNAUTHORIZED,
                UNAUTHORIZED,
                "Authentication required",
            ),
            Self::Forbidden => problem(StatusCode::FORBIDDEN, FORBIDDEN, "Access denied"),
            Self::Internal => problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL,
                "Internal server error",
            ),
        }
    }
}

/// Authenticates HTTP credentials without coupling protected handlers to a login strategy.
///
/// Implementations return evidence only after validating possession and the
/// current external-identity mapping. Browser sessions additionally bind that
/// mapping to the actor who established the session.
pub(crate) trait RequestAuthenticator: Send + Sync {
    fn authenticate<'a>(
        &'a self,
        headers: &'a HeaderMap,
    ) -> impl Future<Output = Result<AuthenticatedActor, ProtectedRequestError>> + Send + 'a;
}

impl<A, R> RequestAuthenticator for CallerAuthenticationService<A, R>
where
    A: ExternalIdentityAuthenticator<Credential = String>,
    R: ActorIdentityResolver,
{
    async fn authenticate(
        &self,
        headers: &HeaderMap,
    ) -> Result<AuthenticatedActor, ProtectedRequestError> {
        let credential = bearer_credential(headers).ok_or(ProtectedRequestError::Unauthorized)?;
        self.authenticate(&credential)
            .await
            .map_err(|error| match error {
                CallerAuthenticationError::Rejected | CallerAuthenticationError::Unregistered => {
                    ProtectedRequestError::Unauthorized
                }
                CallerAuthenticationError::Authenticator(_)
                | CallerAuthenticationError::Resolver(_) => ProtectedRequestError::Internal,
            })
    }
}

/// Establishes caller evidence before parsing tenant or resource identifiers.
pub(crate) async fn authenticate<A: RequestAuthenticator>(
    headers: &HeaderMap,
    service: &A,
) -> Result<AuthenticatedActor, ProtectedRequestError> {
    service.authenticate(headers).await
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

/// Let Axum calculate Allow from merged GET/POST route ownership.
pub(crate) async fn resource_method_not_allowed() -> Response {
    let mut response = protected_method_not_allowed().await;
    response.headers_mut().remove(header::ALLOW);
    response
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

/// Samples trusted host receipt time; clients never supply persisted audit timestamps.
pub(crate) fn now() -> Result<fleetiq_domain::UnixTimeMillis, ProtectedRequestError> {
    let milliseconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ProtectedRequestError::Internal)?
        .as_millis();
    let milliseconds = i64::try_from(milliseconds).map_err(|_| ProtectedRequestError::Internal)?;
    Ok(fleetiq_domain::UnixTimeMillis::new(milliseconds))
}
