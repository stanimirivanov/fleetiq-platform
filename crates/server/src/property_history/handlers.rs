//! Authenticate and authorize before parsing feature inputs or reading telemetry.
use super::{Context, cursor, dto};
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::{
    Json,
    extract::{
        Path, Query, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use fleetiq_access::{AuthorizedTenant, ReadTelemetry, TenantAuthorizationError};
use fleetiq_domain::{
    AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef, SignalTarget,
    TenantId, UnixTimeMillis,
};
use fleetiq_history::PropertyHistoryQuery;
use std::sync::Arc;
type ScopePath = (String, String, String, String);

async fn authorize<A: RequestAuthenticator>(
    ctx: &Context<A>,
    headers: &HeaderMap,
    tenant: &str,
) -> Result<AuthorizedTenant<ReadTelemetry>, ProtectedRequestError> {
    let actor = protected_request::authenticate(headers, &ctx.authentication).await?;
    let tenant = TenantId::new(tenant).map_err(|_| ProtectedRequestError::Forbidden)?;
    ctx.authorization
        .authorize::<ReadTelemetry>(actor, tenant)
        .await
        .map_err(|e| match e {
            TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
            TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
        })
}
fn target(asset: String, property: String, version: &str) -> Option<SignalTarget> {
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(SignalTarget::new(
        AssetId::new(asset).ok()?,
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new(property).ok()?,
            DefinitionVersion::new(version.parse().ok()?).ok()?,
        ),
    ))
}
fn millis(value: &str) -> Option<i64> {
    let parsed = value.parse::<i64>().ok()?;
    (parsed.to_string() == value).then_some(parsed)
}
fn invalid() -> Response {
    protected_request::problem(
        StatusCode::BAD_REQUEST,
        protected_request::BAD_REQUEST,
        "Invalid request",
    )
}
pub(super) async fn list<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<ScopePath>, PathRejection>,
    parameters: Result<Query<dto::Parameters>, QueryRejection>,
) -> Result<Response, ProtectedRequestError> {
    // Even malformed paths authenticate first; invalid tenant scope reveals nothing.
    let grant = authorize(&ctx, &headers, path.as_ref().map_or("", |p| p.0.0.as_str())).await?;
    let (Ok(Path((_, asset, property, version))), Ok(Query(parameters))) = (path, parameters)
    else {
        return Ok(invalid());
    };
    let Some(target) = target(asset, property, &version) else {
        return Ok(invalid());
    };
    let (Some(start), Some(end), Ok(limit)) = (
        millis(&parameters.start_at_ms),
        millis(&parameters.end_at_ms),
        u8::try_from(parameters.limit.unwrap_or(50)),
    ) else {
        return Ok(invalid());
    };
    let after = match parameters.after {
        None => None,
        Some(s) => match cursor::page_key(&s, grant.tenant_id(), &target, start, end) {
            Some(c) => Some(c),
            None => return Ok(invalid()),
        },
    };
    let Ok(query) = PropertyHistoryQuery::new(
        target,
        UnixTimeMillis::new(start),
        UnixTimeMillis::new(end),
        limit,
        after,
    ) else {
        return Ok(invalid());
    };
    let tenant = grant.tenant_id().clone();
    let page = ctx
        .history
        .list(grant, &query)
        .await
        .map_err(|_| ProtectedRequestError::Internal)?;
    let next_after = page
        .next_after()
        .map(|key| cursor::page_token(&tenant, &query, key))
        .transpose()
        .map_err(|_| ProtectedRequestError::Internal)?;
    let changes = page
        .changes()
        .iter()
        .map(dto::ChangeBody::new)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProtectedRequestError::Internal)?;
    Ok(protected_request::no_store(
        Json(dto::HistoryPageBody {
            changes,
            next_after,
        })
        .into_response(),
    ))
}
pub(super) async fn find<A: RequestAuthenticator>(
    State(ctx): State<Arc<Context<A>>>,
    headers: HeaderMap,
    path: Result<Path<(String, String, String, String, String)>, PathRejection>,
) -> Result<Response, ProtectedRequestError> {
    let grant = authorize(&ctx, &headers, path.as_ref().map_or("", |p| p.0.0.as_str())).await?;
    let Ok(Path((_, asset, property, version, token))) = path else {
        return Ok(invalid());
    };
    let Some(target) = target(asset, property, &version) else {
        return Ok(invalid());
    };
    let Some(id) =
        cursor::decode::<cursor::Interpretation>(&token).and_then(cursor::Interpretation::id)
    else {
        return Ok(invalid());
    };
    match ctx
        .history
        .find(grant, &target, &id)
        .await
        .map_err(|_| ProtectedRequestError::Internal)?
    {
        Some(change) => Ok(protected_request::no_store(
            Json(dto::ChangeBody::new(&change).map_err(|_| ProtectedRequestError::Internal)?)
                .into_response(),
        )),
        None => Ok(protected_request::problem(
            StatusCode::NOT_FOUND,
            protected_request::NOT_FOUND,
            "Resource not found",
        )),
    }
}
