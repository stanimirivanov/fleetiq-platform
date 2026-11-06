//! Thin HTTP adapter for protected immutable history; no semantic resolution.
mod budget;
mod cursor;
pub(crate) mod dto;
mod handlers;
mod restored;
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
    budgets: fleetiq_postgres::PostgresHistoryBudgets,
    restores: fleetiq_postgres::PostgresRestore,
    exact: PostgresAssetPropertyChangeView,
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
            budgets: fleetiq_postgres::PostgresHistoryBudgets::new(pool.clone()),
            restores: fleetiq_postgres::PostgresRestore::new(pool.clone()),
            exact: PostgresAssetPropertyChangeView::new(pool.clone()),
    history: PropertyHistoryService::new(PostgresAssetPropertyChangeView::new(pool)),
        }))
}
