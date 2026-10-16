//! Publication and bounded discovery compose metadata services behind exact grants.
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
    Extension, Router,
    extract::DefaultBodyLimit,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{
    AuthorizedTenant, TenantActionMarker, TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_domain::TenantId;
use fleetiq_metadata::{DefinitionCatalogueService, DefinitionKind, DefinitionPublicationService};
use fleetiq_postgres::{PostgresDefinitionRepository, PostgresTenantAccessPolicy};
use sqlx::PgPool;
use std::sync::Arc;
struct DefinitionState<A> {
    authentication: A,
    mutation: MutationGuard,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    publications: DefinitionPublicationService<PostgresDefinitionRepository>,
    catalogue: DefinitionCatalogueService<PostgresDefinitionRepository>,
}
pub(crate) fn postgres_definition_administration_router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    let mut router = Router::new();
    for (kind, collection, versions) in [
        (
            DefinitionKind::AssetType,
            "/api/v1/tenants/{tenant_id}/asset-types",
            "/api/v1/tenants/{tenant_id}/asset-types/{type_id}/versions",
        ),
        (
            DefinitionKind::Property,
            "/api/v1/tenants/{tenant_id}/properties",
            "/api/v1/tenants/{tenant_id}/properties/{property_id}/versions",
        ),
        (
            DefinitionKind::RelationshipType,
            "/api/v1/tenants/{tenant_id}/relationship-types",
            "/api/v1/tenants/{tenant_id}/relationship-types/{type_id}/versions",
        ),
    ] {
        router = router.merge(
            Router::new()
                .route(
                    collection,
                    get(queries::catalogue::<A>)
                        .post(commands::publish::<A>)
                        .fallback(collection_method_not_allowed),
                )
                .route(
                    versions,
                    get(queries::versions::<A>)
                        .fallback(protected_request::protected_method_not_allowed),
                )
                .layer(Extension(kind)),
        );
    }
    router
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(Arc::new(DefinitionState {
            authentication,
            mutation,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            publications: DefinitionPublicationService::new(PostgresDefinitionRepository::new(
                pool.clone(),
            )),
            catalogue: DefinitionCatalogueService::new(PostgresDefinitionRepository::new(pool)),
        }))
}
impl<A: RequestAuthenticator> DefinitionState<A> {
    /// Check current tenant authority before reporting command, ID or query errors.
    async fn authorize<T: TenantActionMarker>(
        &self,
        headers: &HeaderMap,
        tenant: Option<&str>,
        mutation: bool,
    ) -> Result<AuthorizedTenant<T>, ProtectedRequestError> {
        let actor = protected_request::authenticate(headers, &self.authentication).await?;
        if mutation {
            self.mutation.check(headers)?;
        }
        let tenant = tenant
            .and_then(|id| TenantId::new(id).ok())
            .ok_or(ProtectedRequestError::Forbidden)?;
        self.authorization
            .authorize::<T>(actor, tenant)
            .await
            .map_err(|e| match e {
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
async fn collection_method_not_allowed() -> Response {
    let mut response = protected_request::protected_method_not_allowed().await;
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD, POST"));
    response
}
