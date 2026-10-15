//! HTTP adapter for exact tenant grants and immutable change evidence.
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
    AuthorizedTenant, ManageAccess, TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_identity::{TenantAccessAdministration, TenantAccessAudit, TenantGrantCatalog};
use fleetiq_postgres::{
    PostgresTenantAccessAdministration, PostgresTenantAccessAuditRepository,
    PostgresTenantAccessPolicy, PostgresTenantGrantCatalogRepository,
};
use sqlx::PgPool;
use std::sync::Arc;

struct AdministrationState<A> {
    authentication: A,
    mutation: MutationGuard,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    changes: TenantAccessAdministration<PostgresTenantAccessAdministration>,
    catalog: TenantGrantCatalog<PostgresTenantGrantCatalogRepository>,
    audit: TenantAccessAudit<PostgresTenantAccessAuditRepository>,
}

/// Composes existing identity use cases; neither transport nor DTOs own grant policy.
pub(crate) fn postgres_tenant_access_administration_router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/actors/{actor_id}/grants",
            get(queries::catalog::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/actors/{actor_id}/access-changes",
            post(commands::change::<A>).fallback(command_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/access-changes",
            get(queries::audit::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(Arc::new(AdministrationState {
            authentication,
            mutation,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            changes: TenantAccessAdministration::new(PostgresTenantAccessAdministration::new(
                pool.clone(),
            )),
            catalog: TenantGrantCatalog::new(PostgresTenantGrantCatalogRepository::new(
                pool.clone(),
            )),
            audit: TenantAccessAudit::new(PostgresTenantAccessAuditRepository::new(pool)),
        }))
}

impl<A: RequestAuthenticator> AdministrationState<A> {
    /// Establish current authority before exposing parser or resource details.
    async fn authorize(
        &self,
        headers: &HeaderMap,
        tenant: Option<&str>,
        mutation: bool,
    ) -> Result<AuthorizedTenant<ManageAccess>, ProtectedRequestError> {
        let actor = protected_request::authenticate(headers, &self.authentication).await?;
        if mutation {
            self.mutation.check(headers)?;
        }
        let tenant = tenant
            .and_then(|value| fleetiq_domain::TenantId::new(value).ok())
            .ok_or(ProtectedRequestError::Forbidden)?;
        self.authorization
            .authorize::<ManageAccess>(actor, tenant)
            .await
            .map_err(|error| match error {
                TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
                TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
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
        "Command conflicts with current tenant access state",
    )
}
async fn command_method_not_allowed() -> Response {
    let mut response = protected_request::protected_method_not_allowed().await;
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("POST"));
    response
}
