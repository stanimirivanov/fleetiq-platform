//! Thin HTTP adapter for protected immutable history; no semantic resolution.
mod cursor;
mod dto;
mod handlers;
#[cfg(test)]
mod tests;
use crate::protected_request::{RequestAuthenticator, protected_method_not_allowed};
use axum::{Router, routing::get};
use fleetiq_access::TenantAuthorizationService;
use fleetiq_history::PropertyHistoryService;
use fleetiq_postgres::{PostgresAssetPropertyChangeView, PostgresTenantAccessPolicy};
use sqlx::PgPool;
use std::sync::Arc;
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    history: PropertyHistoryService<PostgresAssetPropertyChangeView>,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    Router::new()
        .route("/api/v1/tenants/{tenant_id}/assets/{asset_id}/properties/{property_id}/versions/{version}/history",
            get(handlers::list::<A>).fallback(protected_method_not_allowed))
        .route("/api/v1/tenants/{tenant_id}/assets/{asset_id}/properties/{property_id}/versions/{version}/interpretations/{interpretation}",
            get(handlers::find::<A>).fallback(protected_method_not_allowed))
        .with_state(Arc::new(Context {
            authentication, authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),
            history: PropertyHistoryService::new(PostgresAssetPropertyChangeView::new(pool)),
        }))
}
