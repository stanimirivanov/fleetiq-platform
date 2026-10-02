//! HTTP projection of one immutable, tenant-owned relationship revision.

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
use fleetiq_domain::{AssetRelationship, AssetRelationshipId, TenantId};
use fleetiq_metadata::{RelationshipApplicationService, RelationshipRepository};
use fleetiq_postgres::{
    PostgresActorIdentityResolver, PostgresRelationshipRepository, PostgresTenantAccessPolicy,
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

struct RelationshipQueryState<A, R, P, Q> {
    authentication: CallerAuthenticationService<A, R>,
    authorization: TenantAuthorizationService<P>,
    relationships: RelationshipApplicationService<Q>,
}

/// Builds the exact-revision route from the existing policy and graph ports.
///
/// The relationship service derives tenant scope from consumed authorization
/// evidence; path data cannot provide a repository tenant independently.
fn relationship_router<A, R, P, Q>(
    authentication: CallerAuthenticationService<A, R>,
    authorization: TenantAuthorizationService<P>,
    relationships: RelationshipApplicationService<Q>,
) -> Router
where
    A: ExternalIdentityAuthenticator<Credential = String> + 'static,
    R: ActorIdentityResolver + 'static,
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
            "/api/v1/tenants/{tenant_id}/relationships/{relationship_id}/revisions/{revision}",
            get(get_revision::<A, R, P, Q>).fallback(protected_method_not_allowed),
        )
        .with_state(state)
}

/// Binds the configured credential and current PostgreSQL policy to revision lookup.
pub(crate) fn postgres_relationship_router(pool: PgPool, bearer: BearerIdentityConfig) -> Router {
    relationship_router(
        CallerAuthenticationService::new(
            ConfiguredBearerAuthenticator::new(bearer),
            PostgresActorIdentityResolver::new(pool.clone()),
        ),
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),
        RelationshipApplicationService::new(PostgresRelationshipRepository::new(pool)),
    )
}

async fn get_revision<A, R, P, Q>(
    State(state): State<Arc<RelationshipQueryState<A, R, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String)>, PathRejection>,
) -> Response
where
    A: ExternalIdentityAuthenticator<Credential = String>,
    R: ActorIdentityResolver,
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
mod tests;
