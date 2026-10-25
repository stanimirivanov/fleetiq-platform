//! Protected device/channel commissioning, separate from internal session lookup.
mod dto;
mod handlers;
#[cfg(test)]
mod tests;
use crate::{
    mutation_guard::MutationGuard,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{Router, extract::DefaultBodyLimit, http::HeaderMap, routing::get};
use fleetiq_access::{
    AuthorizedTenant, ManageProtocols, TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_domain::TenantId;
use fleetiq_ingestion::ProtocolRoutingService;
use fleetiq_postgres::{PostgresProtocolRoutingRepository, PostgresTenantAccessPolicy};
use sqlx::PgPool;
use std::sync::Arc;
struct State<A> {
    authentication: A,
    mutation: MutationGuard,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    routes: ProtocolRoutingService<PostgresProtocolRoutingRepository>,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}/protocol-routes/{channel_id}",
            get(handlers::find::<A>)
                .put(handlers::change::<A>)
                .fallback(protected_request::resource_method_not_allowed),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(Arc::new(State {
            authentication,
            mutation,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            routes: ProtocolRoutingService::new(PostgresProtocolRoutingRepository::new(pool)),
        }))
}
impl<A: RequestAuthenticator> State<A> {
    async fn authorize(
        &self,
        headers: &HeaderMap,
        tenant: Option<&str>,
        mutation: bool,
    ) -> Result<AuthorizedTenant<ManageProtocols>, ProtectedRequestError> {
        let actor = protected_request::authenticate(headers, &self.authentication).await?;
        if mutation {
            self.mutation.check(headers)?;
        }
        let tenant = tenant
            .and_then(|value| TenantId::new(value).ok())
            .ok_or(ProtectedRequestError::Forbidden)?;
        self.authorization
            .authorize::<ManageProtocols>(actor, tenant)
            .await
            .map_err(|error| match error {
                TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
                TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
            })
    }
}
