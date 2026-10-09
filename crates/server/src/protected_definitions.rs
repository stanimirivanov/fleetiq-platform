use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State, rejection::PathRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{TenantAccessPolicy, TenantAuthorizationService};
use fleetiq_domain::{
    AssetTypeDefinition, AssetTypeId, AssetTypeRef, DefinitionVersion, PropertyDefinition,
    PropertyDefinitionId, PropertyDefinitionRef, PropertyValueKind, RelationshipTypeDefinition,
    RelationshipTypeId, RelationshipTypeRef, TenantId, UnitId,
};
use fleetiq_metadata::{DefinitionApplicationService, DefinitionRepository};
use fleetiq_postgres::{PostgresDefinitionRepository, PostgresTenantAccessPolicy};
use serde::Serialize;
use sqlx::PgPool;

use crate::protected_request::{
    BAD_REQUEST, INTERNAL, NOT_FOUND, RequestAuthenticator, authenticate, authorize_metadata_read,
    no_store, problem, protected_method_not_allowed,
};

struct DefinitionQueryState<A, P, Q> {
    authentication: A,
    authorization: TenantAuthorizationService<P>,
    definitions: DefinitionApplicationService<Q>,
}

/// Builds exact-version semantic definition queries from existing metadata ports.
///
/// The requested version is validated before use. The definition repository
/// receives tenant scope solely through consumed `ReadMetadata` evidence.
fn definition_router<A, P, Q>(
    authentication: A,
    authorization: TenantAuthorizationService<P>,
    definitions: DefinitionApplicationService<Q>,
) -> Router
where
    A: RequestAuthenticator + 'static,
    P: TenantAccessPolicy + 'static,
    Q: DefinitionRepository + 'static,
{
    let state = Arc::new(DefinitionQueryState {
        authentication,
        authorization,
        definitions,
    });
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/asset-types/{type_id}/versions/{version}",
            get(get_asset_type::<A, P, Q>).fallback(protected_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/properties/{property_id}/versions/{version}",
            get(get_property::<A, P, Q>).fallback(protected_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/relationship-types/{type_id}/versions/{version}",
            get(get_relationship_type::<A, P, Q>).fallback(protected_method_not_allowed),
        )
        .with_state(state)
}

/// Wires the HTTP authentication and PostgreSQL adapters to definition lookup.
pub(crate) fn postgres_definition_router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
) -> Router {
    definition_router(
        authentication,
        TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),
        DefinitionApplicationService::new(PostgresDefinitionRepository::new(pool)),
    )
}

async fn get_asset_type<A, P, Q>(
    State(state): State<Arc<DefinitionQueryState<A, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String)>, PathRejection>,
) -> Response
where
    A: RequestAuthenticator,
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

async fn get_property<A, P, Q>(
    State(state): State<Arc<DefinitionQueryState<A, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String)>, PathRejection>,
) -> Response
where
    A: RequestAuthenticator,
    P: TenantAccessPolicy,
    Q: DefinitionRepository,
{
    let actor = match authenticate(&headers, &state.authentication).await {
        Ok(actor) => actor,
        Err(error) => return error.into_response(),
    };
    let Ok(Path((tenant, property_id, version))) = path else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let (Ok(tenant), Ok(property_id), Some(version)) = (
        TenantId::new(tenant),
        PropertyDefinitionId::new(property_id),
        parse_version(&version),
    ) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match authorize_metadata_read(actor, tenant, &state.authorization).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    let reference = PropertyDefinitionRef::new(property_id, version);
    match state.definitions.find_property(grant, &reference).await {
        Ok(Some(definition)) => no_store(Json(PropertyResponse::from(&definition)).into_response()),
        Ok(None) => problem(StatusCode::NOT_FOUND, NOT_FOUND, "Resource not found"),
        Err(_) => problem(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL,
            "Internal server error",
        ),
    }
}

async fn get_relationship_type<A, P, Q>(
    State(state): State<Arc<DefinitionQueryState<A, P, Q>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String)>, PathRejection>,
) -> Response
where
    A: RequestAuthenticator,
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
        RelationshipTypeId::new(type_id),
        parse_version(&version),
    ) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match authorize_metadata_read(actor, tenant, &state.authorization).await {
        Ok(grant) => grant,
        Err(error) => return error.into_response(),
    };
    let reference = RelationshipTypeRef::new(type_id, version);
    match state
        .definitions
        .find_relationship_type(grant, &reference)
        .await
    {
        Ok(Some(definition)) => {
            no_store(Json(RelationshipTypeResponse::from(&definition)).into_response())
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

#[derive(Serialize)]
struct PropertyResponse<'a> {
    id: &'a str,
    version: u32,
    name: &'a str,
    value_kind: &'static str,
    canonical_unit: Option<&'a str>,
}

impl<'a> From<&'a PropertyDefinition> for PropertyResponse<'a> {
    fn from(definition: &'a PropertyDefinition) -> Self {
        let value_kind = match definition.value_kind() {
            PropertyValueKind::Boolean => "boolean",
            PropertyValueKind::Integer => "integer",
            PropertyValueKind::Decimal => "decimal",
            PropertyValueKind::Text => "text",
        };
        Self {
            id: definition.id().as_str(),
            version: definition.version().get(),
            name: definition.name().as_str(),
            value_kind,
            canonical_unit: definition.canonical_unit().map(UnitId::as_str),
        }
    }
}

#[derive(Serialize)]
struct RelationshipTypeResponse<'a> {
    id: &'a str,
    version: u32,
    name: &'a str,
}

impl<'a> From<&'a RelationshipTypeDefinition> for RelationshipTypeResponse<'a> {
    fn from(definition: &'a RelationshipTypeDefinition) -> Self {
        Self {
            id: definition.reference().id().as_str(),
            version: definition.reference().version().get(),
            name: definition.name().as_str(),
        }
    }
}

#[cfg(test)]
use crate::bearer_authentication::ConfiguredBearerAuthenticator;
#[cfg(test)]
use fleetiq_access::CallerAuthenticationService;
#[cfg(test)]
mod tests;
