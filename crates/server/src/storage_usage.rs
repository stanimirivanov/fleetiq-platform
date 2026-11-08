//! Tenant logical storage evidence with explicit scope and methods.
#[cfg(test)]
mod tests;
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{ReadOperations, TenantAuthorizationError, TenantAuthorizationService};
use fleetiq_domain::TenantId;
use fleetiq_history::storage_usage::StorageMeasure;
use fleetiq_postgres::{PostgresStorageUsage, PostgresTenantAccessPolicy};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    diagnostics: PostgresStorageUsage,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/storage-usage",
            get(find::<A>).fallback(protected_request::resource_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/storage-usage/lifecycle",
            get(find_lifecycle::<A>).fallback(protected_request::resource_method_not_allowed),
        )
        .with_state(Arc::new(Context {
            authentication,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            diagnostics: PostgresStorageUsage::new(pool),
        }))
}
async fn find<A: RequestAuthenticator>(
    State(context): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<String>, axum::extract::rejection::PathRejection>,
) -> Result<Response, ProtectedRequestError> {
    let auth = authorize(&context, &headers, path).await?;
    let snapshot = tokio::time::timeout(Duration::from_secs(5), context.diagnostics.snapshot(auth))
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
        .map_err(|_| ProtectedRequestError::Internal)?;
    let classes = [
        ("accepted_messages", "wire_payload_octets", snapshot.accepted),
        ("decoded_observations", "postgres_row_json_utf8_v1", snapshot.decoded),
        ("property_interpretations", "postgres_row_json_utf8_v1", snapshot.interpretations),
    ].into_iter().map(|(kind, method, m)| json!({"kind":kind,"method":method,"records":m.records.to_string(),"bytes":m.bytes.to_string()})).collect::<Vec<_>>();
    Ok(protected_request::no_store(Json(json!({"observed_at_ms":snapshot.observed_at_ms.to_string(),"coverage":"supported_classes_only","archive":"not_measured","exports":"not_measured","physical_allocation":"not_measured","classes":classes})).into_response()))
}

async fn find_lifecycle<A: RequestAuthenticator>(
    State(context): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<String>, axum::extract::rejection::PathRejection>,
) -> Result<Response, ProtectedRequestError> {
    let auth = authorize(&context, &headers, path).await?;
    let snapshot = tokio::time::timeout(
        Duration::from_secs(5),
        context.diagnostics.lifecycle_snapshot(auth),
    )
    .await
    .map_err(|_| ProtectedRequestError::Internal)?
    .map_err(|_| ProtectedRequestError::Internal)?;
    Ok(protected_request::no_store(
        Json(json!({
            "observed_at_ms": snapshot.observed_at_ms.to_string(),
            "archive": totals(&snapshot.archive),
            "exports": {
                "downloadable": totals(&snapshot.downloadable),
                "pending": totals(&snapshot.pending),
                "expired_retained": totals(&snapshot.expired_retained),
            },
        }))
        .into_response(),
    ))
}

fn totals(measure: &StorageMeasure) -> Value {
    json!({"records": measure.records.to_string(), "bytes": measure.bytes.to_string()})
}

async fn authorize<A: RequestAuthenticator>(
    context: &Context<A>,
    headers: &HeaderMap,
    path: Result<Path<String>, axum::extract::rejection::PathRejection>,
) -> Result<fleetiq_access::AuthorizedTenant<ReadOperations>, ProtectedRequestError> {
    let actor = protected_request::authenticate(headers, &context.authentication).await?;
    let tenant = path
        .ok()
        .and_then(|p| TenantId::new(p.0).ok())
        .ok_or(ProtectedRequestError::Forbidden)?;
    context
        .authorization
        .authorize::<ReadOperations>(actor, tenant)
        .await
        .map_err(|e| match e {
            TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
            TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
        })
}
