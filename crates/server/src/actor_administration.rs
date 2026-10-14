//! Platform actor administration adapter; use cases and invariants stay in identity.
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
    AuthorizedPlatform, ManageIdentity, PlatformAuthorizationError, PlatformAuthorizationService,
};
use fleetiq_identity::{
    ActorCatalog, ActorLifecycleAdministration, ActorLifecycleAudit, ActorProvisioningService,
};
use fleetiq_postgres::{
    PostgresActorCatalogRepository, PostgresActorLifecycleAdministration,
    PostgresActorLifecycleAuditRepository, PostgresActorProvisioning, PostgresPlatformAccessPolicy,
};
use sqlx::PgPool;
use std::sync::Arc;

struct AdministrationState<A> {
    authentication: A,
    mutation: MutationGuard,
    authorization: PlatformAuthorizationService<PostgresPlatformAccessPolicy>,
    provisioning: ActorProvisioningService<PostgresActorProvisioning>,
    lifecycle: ActorLifecycleAdministration<PostgresActorLifecycleAdministration>,
    catalog: ActorCatalog<PostgresActorCatalogRepository>,
    audit: ActorLifecycleAudit<PostgresActorLifecycleAuditRepository>,
}
/// Compose existing application services without granting platform authority from tenant hints.
pub(crate) fn postgres_actor_administration_router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new()
        .route(
            "/api/v1/admin/actors",
            get(queries::catalog::<A>)
                .post(commands::provision::<A>)
                .fallback(catalog_method_not_allowed),
        )
        .route(
            "/api/v1/admin/actors/{actor_id}/lifecycle",
            post(commands::lifecycle::<A>).fallback(command_method_not_allowed),
        )
        .route(
            "/api/v1/admin/actor-lifecycle-changes",
            get(queries::audit::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(Arc::new(AdministrationState {
            authentication,
            mutation,
            authorization: PlatformAuthorizationService::new(PostgresPlatformAccessPolicy::new(
                pool.clone(),
            )),
            provisioning: ActorProvisioningService::new(PostgresActorProvisioning::new(
                pool.clone(),
            )),
            lifecycle: ActorLifecycleAdministration::new(
                PostgresActorLifecycleAdministration::new(pool.clone()),
            ),
            catalog: ActorCatalog::new(PostgresActorCatalogRepository::new(pool.clone())),
            audit: ActorLifecycleAudit::new(PostgresActorLifecycleAuditRepository::new(pool)),
        }))
}
impl<A: RequestAuthenticator> AdministrationState<A> {
    /// Authenticate and authorize before revealing malformed resource or body details.
    async fn authorize(
        &self,
        headers: &HeaderMap,
        mutation: bool,
    ) -> Result<AuthorizedPlatform<ManageIdentity>, ProtectedRequestError> {
        let actor = protected_request::authenticate(headers, &self.authentication).await?;
        if mutation {
            self.mutation.check(headers)?;
        }
        self.authorization
            .authorize::<ManageIdentity>(actor)
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
        "Command conflicts with current identity state",
    )
}
async fn catalog_method_not_allowed() -> Response {
    method_not_allowed("GET, HEAD, POST").await
}
async fn command_method_not_allowed() -> Response {
    method_not_allowed("POST").await
}
async fn method_not_allowed(allow: &'static str) -> Response {
    let mut response = protected_request::protected_method_not_allowed().await;
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static(allow));
    response
}
