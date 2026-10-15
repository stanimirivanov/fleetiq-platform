//! HTTP adapter for exact platform grants and immutable change evidence.
mod commands;
mod dto;
mod queries;
#[cfg(test)]
mod tests;

use crate::{
    mutation_guard::MutationGuard,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use fleetiq_access::{
    AuthorizedPlatform, ManagePlatformAccess, PlatformAuthorizationError,
    PlatformAuthorizationService,
};
use fleetiq_identity::{PlatformAccessAdministration, PlatformAccessAudit, PlatformGrantCatalog};
use fleetiq_postgres::{
    PostgresPlatformAccessAdministration, PostgresPlatformAccessAuditRepository,
    PostgresPlatformAccessPolicy, PostgresPlatformGrantCatalogRepository,
};
use sqlx::PgPool;
use std::sync::Arc;

struct AdministrationState<A> {
    authentication: A,
    mutation: MutationGuard,
    authorization: PlatformAuthorizationService<PostgresPlatformAccessPolicy>,
    changes: PlatformAccessAdministration<PostgresPlatformAccessAdministration>,
    catalog: PlatformGrantCatalog<PostgresPlatformGrantCatalogRepository>,
    audit: PlatformAccessAudit<PostgresPlatformAccessAuditRepository>,
}

/// Composes existing identity use cases; neither transport nor DTOs own grant policy.
pub(crate) fn postgres_platform_access_administration_router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/actors/{actor_id}/platform-grants",
            get(queries::catalog::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .route(
            "/api/v1/admin/actors/{actor_id}/platform-access-changes",
            post(commands::change::<A>).fallback(command_method_not_allowed),
        )
        .route(
            "/api/v1/admin/platform-access-changes",
            get(queries::audit::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(Arc::new(AdministrationState {
            authentication,
            mutation,
            authorization: PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(
                pool.clone(),
            )),
            changes: PlatformAccessAdministration::new(PostgresPlatformAccessAdministration::new(
                pool.clone(),
            )),
            catalog: PlatformGrantCatalog::new(PostgresPlatformGrantCatalogRepository::new(
                pool.clone(),
            )),
            audit: PlatformAccessAudit::new(PostgresPlatformAccessAuditRepository::new(pool)),
        }))
}

impl<A: RequestAuthenticator> AdministrationState<A> {
    /// Establish current authority before exposing parser or resource details.
    async fn authorize(
        &self,
        headers: &HeaderMap,
        mutation: bool,
    ) -> Result<AuthorizedPlatform<ManagePlatformAccess>, ProtectedRequestError> {
        let actor = protected_request::authenticate(headers, &self.authentication).await?;
        if mutation {
            self.mutation.check(headers)?;
        }
        self.authorization
            .authorize::<ManagePlatformAccess>(actor)
            .await
            .map_err(|error| match error {
                PlatformAuthorizationError::Denied => ProtectedRequestError::Forbidden,
                PlatformAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
            })
    }
}

fn invalid() -> Response {
    protected_request::problem(
        StatusCode::BAD_REQUEST,
        protected_request::BAD_REQUEST,
        "Invalid request",
    )
}
fn internal() -> Response {
    ProtectedRequestError::Internal.into_response()
}
fn conflict() -> Response {
    protected_request::problem(
        StatusCode::CONFLICT,
        "urn:fleetiq:problem:conflict",
        "Command conflicts with current platform access state",
    )
}
async fn command_method_not_allowed() -> Response {
    let mut response = protected_request::protected_method_not_allowed().await;
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("POST"));
    response
}
