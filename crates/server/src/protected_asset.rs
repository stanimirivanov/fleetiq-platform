use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State, rejection::PathRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{
    ActorIdentityResolver, CallerAuthenticationError, CallerAuthenticationService,
    ExternalIdentityAuthenticator, ReadMetadata, TenantAccessPolicy, TenantAuthorizationError,
    TenantAuthorizationService,
};
use fleetiq_domain::{Asset, AssetId, TenantId};
use fleetiq_metadata::{AssetApplicationService, AssetRepository};
use fleetiq_postgres::{
    PostgresActorIdentityResolver, PostgresAssetRepository, PostgresTenantAccessPolicy,
};
use serde::Serialize;
use sqlx::PgPool;

use crate::{
    bearer_authentication::ConfiguredBearerAuthenticator,
    config::BearerIdentityConfig,
    http_api::{ApiProblem, method_not_allowed},
};

const BAD_REQUEST: &str = "urn:fleetiq:problem:invalid-request";
const UNAUTHORIZED: &str = "urn:fleetiq:problem:unauthorized";
const FORBIDDEN: &str = "urn:fleetiq:problem:forbidden";
const NOT_FOUND: &str = "urn:fleetiq:problem:not-found";
const INTERNAL: &str = "urn:fleetiq:problem:internal-error";

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
            "/api/v1/tenants/{tenant_id}/assets/{asset_id}",
            get(get_asset::<A, R, P, Q>).fallback(protected_method_not_allowed),
        )
        .with_state(state)
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
    let Some(credential) = bearer_credential(&headers) else {
        return challenge();
    };
    let actor = match state.authentication.authenticate(&credential).await {
        Ok(actor) => actor,
        Err(CallerAuthenticationError::Rejected | CallerAuthenticationError::Unregistered) => {
            return challenge();
        }
        Err(
            CallerAuthenticationError::Authenticator(_) | CallerAuthenticationError::Resolver(_),
        ) => {
            return problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL,
                "Internal server error",
            );
        }
    };

    let Ok(Path((tenant, asset))) = path else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let (Ok(tenant), Ok(asset_id)) = (TenantId::new(tenant), AssetId::new(asset)) else {
        return problem(StatusCode::BAD_REQUEST, BAD_REQUEST, "Invalid request");
    };
    let grant = match state
        .authorization
        .authorize::<ReadMetadata>(actor, tenant)
        .await
    {
        Ok(grant) => grant,
        Err(TenantAuthorizationError::Denied) => {
            return problem(StatusCode::FORBIDDEN, FORBIDDEN, "Access denied");
        }
        Err(TenantAuthorizationError::Policy(_)) => {
            return problem(
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL,
                "Internal server error",
            );
        }
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

fn bearer_credential(headers: &HeaderMap) -> Option<String> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer")
        || token.len() != 64
        || !token.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(token.to_owned())
}

fn challenge() -> Response {
    let mut response = problem(
        StatusCode::UNAUTHORIZED,
        UNAUTHORIZED,
        "Authentication required",
    );
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"FleetIQ\""),
    );
    response
}

async fn protected_method_not_allowed() -> Response {
    no_store(method_not_allowed().await)
}

fn problem(status: StatusCode, kind: &'static str, title: &'static str) -> Response {
    no_store(ApiProblem::new(status, kind, title).into_response())
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
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
