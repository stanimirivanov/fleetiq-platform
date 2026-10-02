//! Protected exact-revision and bounded bitemporal source-binding reads.

use std::sync::Arc;

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
use fleetiq_access::{
    ActorIdentityResolver, CallerAuthenticationService, ExternalIdentityAuthenticator,
    TenantAccessPolicy, TenantAuthorizationService,
};
use fleetiq_domain::{
    AssetId, DeviceEndpointId, SignalBinding, SignalBindingId, SignalSource, SourceSignalId,
    TenantId, UnixTimeMillis,
};
use fleetiq_metadata::{
    SignalBindingApplicationService, SignalBindingRepository, SignalBindingSnapshotPageQuery,
    SignalBindingSnapshotQuery,
};
use fleetiq_postgres::{
    PostgresActorIdentityResolver, PostgresSignalBindingRepository, PostgresTenantAccessPolicy,
};
use serde::{Deserialize, Serialize};
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

/// Builds binding reads from the existing authorized use case.
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
            "/api/v1/tenants/{tenant_id}/signal-bindings",
            get(get_source_snapshot::<A, R, P, Q>).fallback(protected_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/signal-bindings/{binding_id}/revisions/{revision}",
            get(get_revision::<A, R, P, Q>).fallback(protected_method_not_allowed),
        )
        .with_state(state)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotParameters {
    device_asset_id: String,
    endpoint_id: String,
    signal_id: String,
    effective_at_ms: i64,
    known_at_ms: i64,
    limit: Option<u16>,
    after: Option<String>,
}

#[derive(Serialize)]
struct SnapshotResponse<'a> {
    source: SignalSourceResponse<'a>,
    effective_at_ms: i64,
    known_at_ms: i64,
    bindings: Vec<SignalBindingResponse<'a>>,
    next_after: Option<&'a str>,
}

async fn get_source_snapshot<A, R, P, Q>(
    State(state): State<Arc<SignalBindingQueryState<A, R, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    parameters: Result<Query<SnapshotParameters>, QueryRejection>,
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
    let Ok(Path(tenant)) = path else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let Ok(tenant) = TenantId::new(tenant) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match authorize_metadata_read(actor, tenant, &state.authorization).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    let Ok(Query(parameters)) = parameters else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let (Ok(device), Ok(endpoint), Ok(signal)) = (
        AssetId::new(parameters.device_asset_id),
        DeviceEndpointId::new(parameters.endpoint_id),
        SourceSignalId::new(parameters.signal_id),
    ) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let source = SignalSource::new(device, endpoint, signal);
    let Ok(after) = parameters.after.map(SignalBindingId::new).transpose() else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let snapshot = SignalBindingSnapshotQuery::new(
        UnixTimeMillis::new(parameters.effective_at_ms),
        UnixTimeMillis::new(parameters.known_at_ms),
    );
    let Ok(limit) = u8::try_from(parameters.limit.unwrap_or(50)) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let Ok(query) = SignalBindingSnapshotPageQuery::new(snapshot, limit, after) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    match state
        .bindings
        .find_for_source_page(grant, &source, &query)
        .await
    {
        Ok(page) => no_store(
            Json(SnapshotResponse {
                source: SignalSourceResponse {
                    device_asset: source.device_asset_id().as_str(),
                    endpoint_id: source.endpoint_id().as_str(),
                    signal_id: source.signal_id().as_str(),
                },
                effective_at_ms: parameters.effective_at_ms,
                known_at_ms: parameters.known_at_ms,
                bindings: page
                    .bindings()
                    .iter()
                    .map(SignalBindingResponse::from)
                    .collect(),
                next_after: page.next_after().map(SignalBindingId::as_str),
            })
            .into_response(),
        ),
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL,
            "Internal server error",
        ),
    }
}

/// Binds configured bearer identity and current PostgreSQL policy to binding reads.
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
