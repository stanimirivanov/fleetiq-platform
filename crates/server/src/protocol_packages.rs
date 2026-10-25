//! Protected transport for package storage/admission; activation stays separate.
mod compiler;
mod dto;
mod handlers;
#[cfg(test)]
mod tests;
use crate::{
    mutation_guard::MutationGuard,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::HeaderMap,
    routing::{get, post},
};
use compiler::IsolatedCompiler;
use fleetiq_access::{
    AuthorizedTenant, ManageProtocols, TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_domain::TenantId;
use fleetiq_ingestion::{MAX_PROTOCOL_ARTIFACT_BYTES, ProtocolCompiler, ProtocolPackageService};
use fleetiq_postgres::{
    PostgresProtocolArtifactStore, PostgresProtocolPackageCatalog, PostgresTenantAccessPolicy,
};
use sqlx::PgPool;
use std::sync::Arc;
type Service<V> =
    ProtocolPackageService<PostgresProtocolPackageCatalog, PostgresProtocolArtifactStore, V>;
struct State<A, V> {
    authentication: A,
    mutation: MutationGuard,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    packages: Service<V>,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    with_compiler(pool, authentication, mutation, IsolatedCompiler::new())
}
fn with_compiler<A: RequestAuthenticator + 'static, V: ProtocolCompiler + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
    compiler: V,
) -> Router {
    let state = Arc::new(State {
        authentication,
        mutation,
        authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
            pool.clone(),
        )),
        packages: ProtocolPackageService::new(
            PostgresProtocolPackageCatalog::new(pool.clone()),
            PostgresProtocolArtifactStore::new(pool),
            compiler,
        ),
    });
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/protocol-packages/{package_id}/versions/{package_version}",
            get(handlers::find::<A,V>).put(handlers::register::<A,V>)
                .fallback(protected_request::resource_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/protocol-packages/{package_id}/versions/{package_version}/admissions",
            post(handlers::admit::<A,V>).fallback(protected_request::resource_method_not_allowed)
                .layer(DefaultBodyLimit::max(1024)),
        )
        .layer(DefaultBodyLimit::max(MAX_PROTOCOL_ARTIFACT_BYTES))
        .with_state(state)
}
impl<A: RequestAuthenticator, V> State<A, V> {
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
            .and_then(|s| TenantId::new(s).ok())
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
