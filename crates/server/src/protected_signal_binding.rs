//! HTTP projection of one immutable, tenant-owned signal-binding revision.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State, rejection::PathRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{
    ActorIdentityResolver, CallerAuthenticationService, ExternalIdentityAuthenticator,
    TenantAccessPolicy, TenantAuthorizationService,
};
use fleetiq_domain::{SignalBinding, SignalBindingId, TenantId, UnixTimeMillis};
use fleetiq_metadata::{SignalBindingApplicationService, SignalBindingRepository};
use fleetiq_postgres::{
    PostgresActorIdentityResolver, PostgresSignalBindingRepository, PostgresTenantAccessPolicy,
};
use serde::Serialize;
use sqlx::PgPool;

use crate::{
    bearer_authentication::ConfiguredBearerAuthenticator,
    config::BearerIdentityConfig,
    protected_request::{
        BAD_REQUEST, INTERNAL, NOT_FOUND, authenticate, authorize_metadata_read, no_store,
        parse_record_revision, problem, protected_method_not_allowed,
    },
};

struct SignalBindingQueryState<A, R, P, Q> {
    authentication: CallerAuthenticationService<A, R>,
    authorization: TenantAuthorizationService<P>,
    bindings: SignalBindingApplicationService<Q>,
}

/// Builds an exact binding-revision route from the existing authorized use case.
///
/// The application service derives tenant scope from consumed authorization
/// evidence rather than trusting the path as a repository scope.
fn signal_binding_router<A, R, P, Q>(
    authentication: CallerAuthenticationService<A, R>,
    authorization: TenantAuthorizationService<P>,
    bindings: SignalBindingApplicationService<Q>,
) -> Router
where
    A: ExternalIdentityAuthenticator<Credential = String> + 'static,
    R: ActorIdentityResolver + 'static,
    P: TenantAccessPolicy + 'static,
    Q: SignalBindingRepository + 'static,
{
    let state = Arc::new(SignalBindingQueryState {
        authentication,
        authorization,
        bindings,
    });
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/signal-bindings/{binding_id}/revisions/{revision}",
            get(get_revision::<A, R, P, Q>).fallback(protected_method_not_allowed),
        )
        .with_state(state)
}

/// Binds configured bearer identity and current PostgreSQL policy to binding lookup.
pub(crate) fn postgres_signal_binding_router(pool: PgPool, bearer: BearerIdentityConfig) -> Router {
    signal_binding_router(
        CallerAuthenticationService::new(
            ConfiguredBearerAuthenticator::new(bearer),
            PostgresActorIdentityResolver::new(pool.clone()),
        ),
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),
        SignalBindingApplicationService::new(PostgresSignalBindingRepository::new(pool)),
    )
}

async fn get_revision<A, R, P, Q>(
    State(state): State<Arc<SignalBindingQueryState<A, R, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String)>, PathRejection>,
) -> Response
where
    A: ExternalIdentityAuthenticator<Credential = String>,
    R: ActorIdentityResolver,
    P: TenantAccessPolicy,
    Q: SignalBindingRepository,
{
    let actor = match authenticate(&headers, &state.authentication).await {
        Ok(actor) => actor,
        Err(error) => return error.into_response(),
    };
    let Ok(Path((tenant, binding_id, revision))) = path else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let (Ok(tenant), Ok(binding_id), Some(revision)) = (
        TenantId::new(tenant),
        SignalBindingId::new(binding_id),
        parse_record_revision(&revision),
    ) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match authorize_metadata_read(actor, tenant, &state.authorization).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    match state
        .bindings
        .find_revision(grant, &binding_id, revision)
        .await
    {
        Ok(Some(binding)) => no_store(Json(SignalBindingResponse::from(&binding)).into_response()),
        Ok(None) => problem(StatusCode::NOT_FOUND, NOT_FOUND, "Resource not found"),
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL,
            "Internal server error",
        ),
    }
}

#[derive(Serialize)]
struct SignalBindingResponse<'a> {
    id: &'a str,
    tenant_id: &'a str,
    // Decimal text retains the complete u64 revision range in JavaScript.
    revision: String,
    recorded_at_ms: i64,
    source: SignalSourceResponse<'a>,
    target: SignalTargetResponse<'a>,
    effective_interval: EffectiveIntervalResponse,
}

impl<'a> From<&'a SignalBinding> for SignalBindingResponse<'a> {
    fn from(binding: &'a SignalBinding) -> Self {
        let interval = binding.effective_interval();
        Self {
            id: binding.id().as_str(),
            tenant_id: binding.tenant_id().as_str(),
            revision: binding.revision().get().to_string(),
            recorded_at_ms: binding.recorded_at().get(),
            source: SignalSourceResponse {
                device_asset: binding.source().device_asset_id().as_str(),
                endpoint_id: binding.source().endpoint_id().as_str(),
                signal_id: binding.source().signal_id().as_str(),
            },
            target: SignalTargetResponse {
                asset_id: binding.target().asset_id().as_str(),
                property: PropertyReferenceResponse {
                    id: binding.target().property().id().as_str(),
                    version: binding.target().property().version().get(),
                },
            },
            effective_interval: EffectiveIntervalResponse {
                start_ms: interval.start().get(),
                end_ms: interval.end().map(UnixTimeMillis::get),
            },
        }
    }
}

#[derive(Serialize)]
struct SignalSourceResponse<'a> {
    #[serde(rename = "device_asset_id")]
    device_asset: &'a str,
    endpoint_id: &'a str,
    signal_id: &'a str,
}

#[derive(Serialize)]
struct SignalTargetResponse<'a> {
    asset_id: &'a str,
    property: PropertyReferenceResponse<'a>,
}

#[derive(Serialize)]
struct PropertyReferenceResponse<'a> {
    id: &'a str,
    version: u32,
}

#[derive(Serialize)]
struct EffectiveIntervalResponse {
    start_ms: i64,
    end_ms: Option<i64>,
}

#[cfg(test)]
mod tests;
