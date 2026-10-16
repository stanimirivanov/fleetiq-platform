//! Thin command adapters preserve domain-owned revisions and selected identity.
mod dto;
mod handlers;
#[cfg(test)]
mod tests;
use crate::{
    mutation_guard::MutationGuard,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{Router, extract::DefaultBodyLimit, http::HeaderMap, routing::post};
use fleetiq_access::{
    AuthorizedTenant, ManageMetadata, TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_domain::TenantId;
use fleetiq_metadata::RevisionCommandService;
use fleetiq_postgres::{PostgresRevisionCommandRepository, PostgresTenantAccessPolicy};
use sqlx::PgPool;
use std::sync::Arc;

struct RevisionState<A> {
    authentication: A,
    mutation: MutationGuard,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    revisions: RevisionCommandService<PostgresRevisionCommandRepository>,
}
pub(crate) fn postgres_revision_router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/relationships/{relationship_id}/revisions/{revision}",
            post(handlers::relationship::<A>),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/signal-bindings/{binding_id}/revisions/{revision}",
            post(handlers::binding::<A>),
        )
        .layer(DefaultBodyLimit::max(32 * 1024))
        .with_state(Arc::new(RevisionState {
            authentication,
            mutation,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            revisions: RevisionCommandService::new(PostgresRevisionCommandRepository::new(pool)),
        }))
}
impl<A: RequestAuthenticator> RevisionState<A> {
    /// Check live authority before exposing malformed body or reference details.
    async fn authorize(
        &self,
        headers: &HeaderMap,
        tenant: Option<&str>,
    ) -> Result<AuthorizedTenant<ManageMetadata>, ProtectedRequestError> {
        let actor = protected_request::authenticate(headers, &self.authentication).await?;
        self.mutation.check(headers)?;
        let tenant = tenant
            .and_then(|v| TenantId::new(v).ok())
            .ok_or(ProtectedRequestError::Forbidden)?;
        self.authorization
            .authorize::<ManageMetadata>(actor, tenant)
            .await
            .map_err(|e| match e {
                TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
                TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
            })
    }
}
