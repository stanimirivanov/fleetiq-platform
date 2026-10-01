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
use fleetiq_domain::{AssetTypeDefinition, AssetTypeId, AssetTypeRef, DefinitionVersion, TenantId};
use fleetiq_metadata::{DefinitionApplicationService, DefinitionRepository};
use fleetiq_postgres::{
    PostgresActorIdentityResolver, PostgresDefinitionRepository, PostgresTenantAccessPolicy,
};
use serde::Serialize;
use sqlx::PgPool;

use crate::{
    bearer_authentication::ConfiguredBearerAuthenticator,
    config::BearerIdentityConfig,
    protected_request::{
        BAD_REQUEST, INTERNAL, NOT_FOUND, authenticate, authorize_metadata_read, no_store, problem,
        protected_method_not_allowed,
    },
};

struct AssetTypeQueryState<A, R, P, Q> {
    authentication: CallerAuthenticationService<A, R>,
    authorization: TenantAuthorizationService<P>,
    definitions: DefinitionApplicationService<Q>,
}

/// Builds an exact-version asset-type query from existing metadata ports.
///
/// The requested version is validated before use. The definition repository
/// receives tenant scope solely through consumed `ReadMetadata` evidence.
fn asset_type_router<A, R, P, Q>(
    authentication: CallerAuthenticationService<A, R>,
    authorization: TenantAuthorizationService<P>,
    definitions: DefinitionApplicationService<Q>,
) -> Router
where
    A: ExternalIdentityAuthenticator<Credential = String> + 'static,
    R: ActorIdentityResolver + 'static,
    P: TenantAccessPolicy + 'static,
    Q: DefinitionRepository + 'static,
{
    let state = Arc::new(AssetTypeQueryState {
        authentication,
        authorization,
        definitions,
    });
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/asset-types/{type_id}/versions/{version}",
            get(get_asset_type::<A, R, P, Q>).fallback(protected_method_not_allowed),
        )
        .with_state(state)
}

/// Wires the configured credential and PostgreSQL adapters to definition lookup.
pub(crate) fn postgres_asset_type_router(pool: PgPool, bearer: BearerIdentityConfig) -> Router {
    asset_type_router(
        CallerAuthenticationService::new(
            ConfiguredBearerAuthenticator::new(bearer),
            PostgresActorIdentityResolver::new(pool.clone()),
        ),
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),
        DefinitionApplicationService::new(PostgresDefinitionRepository::new(pool)),
    )
}

async fn get_asset_type<A, R, P, Q>(
    State(state): State<Arc<AssetTypeQueryState<A, R, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String)>, PathRejection>,
) -> Response
where
    A: ExternalIdentityAuthenticator<Credential = String>,
    R: ActorIdentityResolver,
    P: TenantAccessPolicy,
    Q: DefinitionRepository,
{
    let actor = match authenticate(&headers, &state.authentication).await {
        Ok(actor) => actor,
        Err(error) => return error.into_response(),
    };
    let Ok(Path((tenant, type_id, version))) = path else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let (Ok(tenant), Ok(type_id), Some(version)) = (
        TenantId::new(tenant),
        AssetTypeId::new(type_id),
        parse_version(&version),
    ) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match authorize_metadata_read(actor, tenant, &state.authorization).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    let reference = AssetTypeRef::new(type_id, version);
    match state.definitions.find_asset_type(grant, &reference).await {
        Ok(Some(definition)) => {
            no_store(Json(AssetTypeResponse::from(&definition)).into_response())
        }
        Ok(None) => problem(StatusCode::NOT_FOUND, NOT_FOUND, "Resource not found"),
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL,
            "Internal server error",
        ),
    }
}

fn parse_version(value: &str) -> Option<DefinitionVersion> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value
        .parse::<u32>()
        .ok()
        .and_then(|version| DefinitionVersion::new(version).ok())
}

#[derive(Serialize)]
struct AssetTypeResponse<'a> {
    id: &'a str,
    version: u32,
    name: &'a str,
    supported_properties: Vec<PropertyReferenceResponse<'a>>,
}

impl<'a> From<&'a AssetTypeDefinition> for AssetTypeResponse<'a> {
    fn from(definition: &'a AssetTypeDefinition) -> Self {
        Self {
            id: definition.reference().id().as_str(),
            version: definition.reference().version().get(),
            name: definition.name().as_str(),
            supported_properties: definition
                .supported_properties()
                .iter()
                .map(|reference| PropertyReferenceResponse {
                    id: reference.id().as_str(),
                    version: reference.version().get(),
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct PropertyReferenceResponse<'a> {
    id: &'a str,
    version: u32,
}

#[cfg(test)]
mod tests;
