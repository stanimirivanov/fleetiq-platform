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
use fleetiq_domain::{Asset, AssetId, TenantId};
use fleetiq_metadata::{
    AssetApplicationService, AssetCatalogueEntry, AssetPageQuery, AssetRepository,
};
use fleetiq_postgres::{
    PostgresActorIdentityResolver, PostgresAssetRepository, PostgresTenantAccessPolicy,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    bearer_authentication::ConfiguredBearerAuthenticator,
    config::BearerIdentityConfig,
    protected_request::{
        BAD_REQUEST, INTERNAL, NOT_FOUND, authenticate, authorize_metadata_read, no_store, problem,
        protected_method_not_allowed,
    },
};

struct AssetQueryState<A, R, P, Q> {
    authentication: CallerAuthenticationService<A, R>,
    authorization: TenantAuthorizationService<P>,
    assets: AssetApplicationService<Q>,
}

/// Composes the first protected resource from existing policy and metadata ports.
///
/// The selected tenant is authorized for every request before the repository
/// receives it. This function is generic only to permit in-process HTTP tests
/// with deterministic adapters; the process uses PostgreSQL adapters below.
fn asset_router<A, R, P, Q>(
    authentication: CallerAuthenticationService<A, R>,
    authorization: TenantAuthorizationService<P>,
    assets: AssetApplicationService<Q>,
) -> Router
where
    A: ExternalIdentityAuthenticator<Credential = String> + 'static,
    R: ActorIdentityResolver + 'static,
    P: TenantAccessPolicy + 'static,
    Q: AssetRepository + 'static,
{
    let state = Arc::new(AssetQueryState {
        authentication,
        authorization,
        assets,
    });
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/assets",
            get(get_asset_page::<A, R, P, Q>).fallback(protected_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}",
            get(get_asset::<A, R, P, Q>).fallback(protected_method_not_allowed),
        )
        .with_state(state)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetPageParameters {
    limit: Option<u16>,
    after: Option<String>,
}

#[derive(Serialize)]
struct AssetPageResponse<'a> {
    assets: Vec<AssetSummaryResponse<'a>>,
    next_after: Option<&'a str>,
}

#[derive(Serialize)]
struct AssetSummaryResponse<'a> {
    id: &'a str,
    tenant_id: &'a str,
    name: &'a str,
    asset_type: AssetTypeResponse<'a>,
}

impl<'a> From<&'a AssetCatalogueEntry> for AssetSummaryResponse<'a> {
    fn from(asset: &'a AssetCatalogueEntry) -> Self {
        Self {
            id: asset.id().as_str(),
            tenant_id: asset.tenant_id().as_str(),
            name: asset.name().as_str(),
            asset_type: AssetTypeResponse {
                id: asset.asset_type().id().as_str(),
                version: asset.asset_type().version().get(),
            },
        }
    }
}

async fn get_asset_page<A, R, P, Q>(
    State(state): State<Arc<AssetQueryState<A, R, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    parameters: Result<Query<AssetPageParameters>, QueryRejection>,
) -> Response
where
    A: ExternalIdentityAuthenticator<Credential = String>,
    R: ActorIdentityResolver,
    P: TenantAccessPolicy,
    Q: AssetRepository,
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
    let Ok(after) = parameters.after.map(AssetId::new).transpose() else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let Ok(limit) = u8::try_from(parameters.limit.unwrap_or(50)) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let Ok(query) = AssetPageQuery::new(limit, after) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    match state.assets.list_page(grant, &query).await {
        Ok(page) => no_store(
            Json(AssetPageResponse {
                assets: page
                    .assets()
                    .iter()
                    .map(AssetSummaryResponse::from)
                    .collect(),
                next_after: page.next_after().map(AssetId::as_str),
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

/// Binds the protected route to credential validation and current PostgreSQL policy.
pub(crate) fn postgres_asset_router(pool: PgPool, bearer: BearerIdentityConfig) -> Router {
    asset_router(
        CallerAuthenticationService::new(
            ConfiguredBearerAuthenticator::new(bearer),
            PostgresActorIdentityResolver::new(pool.clone()),
        ),
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),
        AssetApplicationService::new(PostgresAssetRepository::new(pool)),
    )
}

async fn get_asset<A, R, P, Q>(
    State(state): State<Arc<AssetQueryState<A, R, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response
where
    A: ExternalIdentityAuthenticator<Credential = String>,
    R: ActorIdentityResolver,
    P: TenantAccessPolicy,
    Q: AssetRepository,
{
    let actor = match authenticate(&headers, &state.authentication).await {
        Ok(actor) => actor,
        Err(error) => return error.into_response(),
    };

    let Ok(Path((tenant, asset))) = path else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let (Ok(tenant), Ok(asset_id)) = (TenantId::new(tenant), AssetId::new(asset)) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match authorize_metadata_read(actor, tenant, &state.authorization).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    match state.assets.find_by_id(grant, &asset_id).await {
        Ok(Some(asset)) => no_store(Json(AssetResponse::from(&asset)).into_response()),
        Ok(None) => problem(StatusCode::NOT_FOUND, NOT_FOUND, "Resource not found"),
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL,
            "Internal server error",
        ),
    }
}

#[derive(Serialize)]
struct AssetResponse<'a> {
    id: &'a str,
    tenant_id: &'a str,
    name: &'a str,
    asset_type: AssetTypeResponse<'a>,
    external_identifiers: Vec<ExternalIdentifierResponse<'a>>,
}

impl<'a> From<&'a Asset> for AssetResponse<'a> {
    fn from(asset: &'a Asset) -> Self {
        Self {
            id: asset.id().as_str(),
            tenant_id: asset.tenant_id().as_str(),
            name: asset.name().as_str(),
            asset_type: AssetTypeResponse {
                id: asset.asset_type().id().as_str(),
                version: asset.asset_type().version().get(),
            },
            external_identifiers: asset
                .external_identifiers()
                .iter()
                .map(|identifier| ExternalIdentifierResponse {
                    kind: identifier.kind().as_str(),
                    authority: identifier.authority().map(AsRef::as_ref),
                    value: identifier.value().as_str(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct AssetTypeResponse<'a> {
    id: &'a str,
    version: u32,
}

#[derive(Serialize)]
struct ExternalIdentifierResponse<'a> {
    kind: &'a str,
    authority: Option<&'a str>,
    value: &'a str,
}

#[cfg(test)]
mod tests;
