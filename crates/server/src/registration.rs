//! Registration transport composes a narrow metadata use case, not SQL policy.
mod commands;
mod dto;
#[cfg(test)]
mod tests;
use crate::{
    mutation_guard::MutationGuard,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::post,
};
use fleetiq_access::{
    AuthorizedTenant, ManageMetadata, TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_domain::TenantId;
use fleetiq_metadata::RegistrationService;
use fleetiq_postgres::{PostgresRegistrationRepository, PostgresTenantAccessPolicy};
use sqlx::PgPool;
use std::sync::Arc;

struct RegistrationState<A> {
    authentication: A,
    mutation: MutationGuard,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    registrations: RegistrationService<PostgresRegistrationRepository>,
}
pub(crate) fn postgres_registration_router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/assets",
            post(commands::asset::<A>),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}/device",
            post(commands::device::<A>),
        )
        .layer(DefaultBodyLimit::max(32 * 1024))
        .with_state(Arc::new(RegistrationState {
            authentication,
            mutation,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            registrations: RegistrationService::new(PostgresRegistrationRepository::new(pool)),
        }))
}
impl<A: RequestAuthenticator> RegistrationState<A> {
    /// Authenticate and check current ownership before exposing input errors.
    async fn authorize(
        &self,
        headers: &HeaderMap,
        tenant: Option<&str>,
    ) -> Result<AuthorizedTenant<ManageMetadata>, ProtectedRequestError> {
        let actor = protected_request::authenticate(headers, &self.authentication).await?;
        self.mutation.check(headers)?;
        let tenant = tenant
            .and_then(|id| TenantId::new(id).ok())
            .ok_or(ProtectedRequestError::Forbidden)?;
        self.authorization
            .authorize::<ManageMetadata>(actor, tenant)
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
