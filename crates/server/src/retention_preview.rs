//! Protected read-only retention context; no mutation or executor permit is exposed.
mod dto;
#[cfg(test)]
mod protection_tests;
#[cfg(test)]
mod tests;
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{ReadOperations, TenantAuthorizationError, TenantAuthorizationService};
use fleetiq_domain::TenantId;
use fleetiq_postgres::{PostgresRetentionPreview, PostgresTenantAccessPolicy};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    preview: PostgresRetentionPreview,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/retention-preview",
            get(find::<A>).fallback(protected_request::resource_method_not_allowed),
        )
        .with_state(Arc::new(Context {
            authentication,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            preview: PostgresRetentionPreview::new(pool),
        }))
}
fn invalid() -> Response {
    protected_request::problem(
        StatusCode::BAD_REQUEST,
        protected_request::BAD_REQUEST,
        "Invalid request",
    )
}
async fn find<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    parameters: Result<Query<dto::Parameters>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    let actor = protected_request::authenticate(&headers, &ctx.authentication).await?;
    let tenant = path
        .ok()
        .and_then(|p| TenantId::new(p.0).ok())
        .ok_or(ProtectedRequestError::Forbidden)?;
    let grant = ctx
        .authorization
        .authorize::<ReadOperations>(actor, tenant.clone())
        .await
        .map_err(|e| match e {
            TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
            TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
        })?;
    let Ok(Query(parameters)) = parameters else {
        return Ok(invalid());
    };
    let previous = match parameters.preview {
        None => None,
        Some(token) => {
            let Some(token) = crate::cursor_codec::decode::<dto::ContextToken>(&token) else {
                return Ok(invalid());
            };
            if token.kind != "retention-preview-v1" || token.tenant != tenant.as_str() {
                return Ok(invalid());
            }
            Some(token)
        }
    };
    let report = tokio::time::timeout(Duration::from_secs(5), ctx.preview.snapshot(grant))
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
        .map_err(|_| ProtectedRequestError::Internal)?;
    let candidates = report
        .candidates
        .iter()
        .map(dto::CandidateBody::from)
        .collect::<Vec<_>>();
    let fingerprint = dto::fingerprint(report.policy_revision, report.has_more, &candidates)
        .map_err(|_| ProtectedRequestError::Internal)?;
    if previous.as_ref().is_some_and(|p| {
        p.revision != report.policy_revision
            || p.fingerprint != fingerprint
            || p.observed > report.observed_at_ms
            || p.expires <= report.observed_at_ms
            || p.expires.checked_sub(p.observed) != Some(60_000)
    }) {
        return Ok(protected_request::problem(
            StatusCode::CONFLICT,
            "urn:fleetiq:problem:stale-retention-preview",
            "Retention preview changed or expired",
        ));
    }
    let expires = report
        .observed_at_ms
        .checked_add(60_000)
        .ok_or(ProtectedRequestError::Internal)?;
    let token = crate::cursor_codec::encode(&dto::ContextToken {
        kind: "retention-preview-v1".into(),
        tenant: tenant.as_str().into(),
        revision: report.policy_revision,
        observed: report.observed_at_ms,
        expires,
        fingerprint,
    })
    .map_err(|_| ProtectedRequestError::Internal)?;
    if token.len() > 8192 {
        return Err(ProtectedRequestError::Internal);
    }
    let estimated = report
        .candidates
        .iter()
        .map(fleetiq_history::lifecycle::retention::RetentionCandidate::estimated_reclaimable_bytes)
        .sum::<u64>()
        .to_string();
    Ok(protected_request::no_store(
        Json(dto::PreviewBody {
            observed_at_ms: report.observed_at_ms.to_string(),
            expires_at_ms: expires.to_string(),
            policy_revision: report.policy_revision.to_string(),
            preview_token: token,
            sample_scope: "first_100_interpretations_by_identity",
            has_more: report.has_more,
            consumer_scope: "tenant_wide_conservative",
            age_method: "verified_manifest_lease_end_v1",
            estimate_method: "three_bigint_projection_fields_v1",
            estimated_reclaimable_bytes: estimated,
            physical_reclaimable_bytes: "not_measured",
            retained_anchors: ["lineage", "idempotency"],
            reclamation_supported: false,
            candidates,
        })
        .into_response(),
    ))
}
