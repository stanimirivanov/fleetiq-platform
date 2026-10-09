//! Protected exact-revision and bounded bitemporal asset-relationship reads.

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
use fleetiq_access::{TenantAccessPolicy, TenantAuthorizationService};
use fleetiq_domain::{AssetId, AssetRelationship, AssetRelationshipId, TenantId, UnixTimeMillis};
use fleetiq_metadata::{
    RelationshipApplicationService, RelationshipRepository, RelationshipSnapshotPageQuery,
    RelationshipSnapshotQuery,
};
use fleetiq_postgres::{PostgresRelationshipRepository, PostgresTenantAccessPolicy};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::protected_request::{
    BAD_REQUEST, INTERNAL, NOT_FOUND, RequestAuthenticator, authenticate, authorize_metadata_read,
    no_store, parse_record_revision, problem, protected_method_not_allowed,
};

struct RelationshipQueryState<A, P, Q> {
    authentication: A,
    authorization: TenantAuthorizationService<P>,
    relationships: RelationshipApplicationService<Q>,
}

/// Builds relationship reads from the existing policy and graph ports.
///
/// The relationship service derives tenant scope from consumed authorization
/// evidence; path data cannot provide a repository tenant independently.
fn relationship_router<A, P, Q>(
    authentication: A,
    authorization: TenantAuthorizationService<P>,
    relationships: RelationshipApplicationService<Q>,
) -> Router
where
    A: RequestAuthenticator + 'static,
    P: TenantAccessPolicy + 'static,
    Q: RelationshipRepository + 'static,
{
    let state = Arc::new(RelationshipQueryState {
        authentication,
        authorization,
        relationships,
    });
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}/relationships",
            get(get_asset_snapshot::<A, P, Q>).fallback(protected_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/relationships/{relationship_id}/revisions/{revision}",
            get(get_revision::<A, P, Q>).fallback(protected_method_not_allowed),
        )
        .with_state(state)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotParameters {
    effective_at_ms: i64,
    known_at_ms: i64,
    limit: Option<u16>,
    after: Option<String>,
}

#[derive(Serialize)]
struct SnapshotResponse<'a> {
    asset_id: &'a str,
    effective_at_ms: i64,
    known_at_ms: i64,
    relationships: Vec<RelationshipResponse<'a>>,
    next_after: Option<&'a str>,
}

async fn get_asset_snapshot<A, P, Q>(
    State(state): State<Arc<RelationshipQueryState<A, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    parameters: Result<Query<SnapshotParameters>, QueryRejection>,
) -> Response
where
    A: RequestAuthenticator,
    P: TenantAccessPolicy,
    Q: RelationshipRepository,
{
    let actor = match authenticate(&headers, &state.authentication).await {
        Ok(actor) => actor,
        Err(error) => return error.into_response(),
    };
    let Ok(Path((tenant, asset_id))) = path else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let (Ok(tenant), Ok(asset_id)) = (TenantId::new(tenant), AssetId::new(asset_id)) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match authorize_metadata_read(actor, tenant, &state.authorization).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    let Ok(Query(parameters)) = parameters else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let Ok(after) = parameters.after.map(AssetRelationshipId::new).transpose() else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let snapshot = RelationshipSnapshotQuery::new(
        UnixTimeMillis::new(parameters.effective_at_ms),
        UnixTimeMillis::new(parameters.known_at_ms),
    );
    let Ok(limit) = u8::try_from(parameters.limit.unwrap_or(50)) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let Ok(query) = RelationshipSnapshotPageQuery::new(snapshot, limit, after) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    match state
        .relationships
        .find_for_asset_page(grant, &asset_id, &query)
        .await
    {
        Ok(Some(page)) => no_store(
            Json(SnapshotResponse {
                asset_id: asset_id.as_str(),
                effective_at_ms: parameters.effective_at_ms,
                known_at_ms: parameters.known_at_ms,
                relationships: page
                    .relationships()
                    .iter()
                    .map(RelationshipResponse::from)
                    .collect(),
                next_after: page.next_after().map(AssetRelationshipId::as_str),
            })
            .into_response(),
        ),
        Ok(None) => problem(StatusCode::NOT_FOUND, NOT_FOUND, "Resource not found"),
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL,
            "Internal server error",
        ),
    }
}

/// Binds the HTTP authentication and current PostgreSQL policy to both reads.
pub(crate) fn postgres_relationship_router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
) -> Router {
    relationship_router(
        authentication,
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),
        RelationshipApplicationService::new(PostgresRelationshipRepository::new(pool)),
    )
}

async fn get_revision<A, P, Q>(
    State(state): State<Arc<RelationshipQueryState<A, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String)>, PathRejection>,
) -> Response
where
    A: RequestAuthenticator,
    P: TenantAccessPolicy,
    Q: RelationshipRepository,
{
    let actor = match authenticate(&headers, &state.authentication).await {
        Ok(actor) => actor,
        Err(error) => return error.into_response(),
    };
    let Ok(Path((tenant, relationship_id, revision))) = path else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let (Ok(tenant), Ok(relationship_id), Some(revision)) = (
        TenantId::new(tenant),
        AssetRelationshipId::new(relationship_id),
        parse_record_revision(&revision),
    ) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match authorize_metadata_read(actor, tenant, &state.authorization).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    match state
        .relationships
        .find_revision(grant, &relationship_id, revision)
        .await
    {
        Ok(Some(relationship)) => {
            no_store(Json(RelationshipResponse::from(&relationship)).into_response())
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, NOT_FOUND, "Resource not found"),
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL,
            "Internal server error",
        ),
    }
}

#[derive(Serialize)]
struct RelationshipResponse<'a> {
    id: &'a str,
    tenant_id: &'a str,
    // A decimal string preserves the full u64 revision range in JavaScript clients.
    revision: String,
    recorded_at_ms: i64,
    relationship_type: RelationshipTypeResponse<'a>,
    source_asset_id: &'a str,
    target_asset_id: &'a str,
    effective_interval: EffectiveIntervalResponse,
}

impl<'a> From<&'a AssetRelationship> for RelationshipResponse<'a> {
    fn from(relationship: &'a AssetRelationship) -> Self {
        let interval = relationship.effective_interval();
        Self {
            id: relationship.id().as_str(),
            tenant_id: relationship.tenant_id().as_str(),
            revision: relationship.revision().get().to_string(),
            recorded_at_ms: relationship.recorded_at().get(),
            relationship_type: RelationshipTypeResponse {
                id: relationship.relationship_type().id().as_str(),
                version: relationship.relationship_type().version().get(),
            },
            source_asset_id: relationship.source_asset_id().as_str(),
            target_asset_id: relationship.target_asset_id().as_str(),
            effective_interval: EffectiveIntervalResponse {
                start_ms: interval.start().get(),
                end_ms: interval.end().map(fleetiq_domain::UnixTimeMillis::get),
            },
        }
    }
}

#[derive(Serialize)]
struct RelationshipTypeResponse<'a> {
    id: &'a str,
    version: u32,
}

#[derive(Serialize)]
struct EffectiveIntervalResponse {
    start_ms: i64,
    end_ms: Option<i64>,
}

#[cfg(test)]
use crate::bearer_authentication::ConfiguredBearerAuthenticator;
#[cfg(test)]
use fleetiq_access::CallerAuthenticationService;
#[cfg(test)]
mod tests;
