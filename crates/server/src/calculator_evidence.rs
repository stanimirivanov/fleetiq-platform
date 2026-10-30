//! Bounded terminal evidence reads; no evaluation or raw preparation expansion.
mod dto;
#[cfg(test)]
mod tests;
use crate::{
    cursor_codec,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{
    Router,
    extract::{
        Path, Query, State as ExtractState,
        rejection::{PathRejection, QueryRejection},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{
    AuthorizedTenant, ReadTelemetry, TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_analytics::CalculatorEvaluationId;
use fleetiq_domain::{AssetId, TenantId};
use fleetiq_postgres::{PostgresCalculatorEvidence, PostgresTenantAccessPolicy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{fmt::Write, sync::Arc};
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    store: PostgresCalculatorEvidence,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    Router::new()
    .route("/api/v1/tenants/{tenant_id}/assets/{asset_id}/calculator-evaluations",get(list::<A>).fallback(protected_request::protected_method_not_allowed))
    .route("/api/v1/tenants/{tenant_id}/assets/{asset_id}/calculator-evaluations/{evaluation_id}",get(find::<A>).fallback(protected_request::protected_method_not_allowed))
    .route("/api/v1/tenants/{tenant_id}/assets/{asset_id}/calculator-evaluations/{evaluation_id}/outputs",get(outputs::<A>).fallback(protected_request::protected_method_not_allowed))
    .with_state(Arc::new(Context{authentication,authorization:TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),store:PostgresCalculatorEvidence::new(pool)}))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    start_at_ms: String,
    end_at_ms: String,
    limit: Option<u16>,
    after: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputParameters {
    limit: Option<u16>,
    after: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Key {
    scope: String,
    event: i64,
    evaluation: String,
    index: u32,
}
type ScopePath = Result<Path<(String, String)>, PathRejection>;
type ExactPath = Result<Path<(String, String, String)>, PathRejection>;
enum ReadError {
    Invalid,
    Missing,
    Protected(ProtectedRequestError),
}
impl IntoResponse for ReadError {
    fn into_response(self) -> Response {
        match self {
            Self::Invalid => protected_request::problem(
                StatusCode::BAD_REQUEST,
                protected_request::BAD_REQUEST,
                "Invalid evidence request",
            ),
            Self::Missing => protected_request::problem(
                StatusCode::NOT_FOUND,
                protected_request::NOT_FOUND,
                "Evaluation or scope not found",
            ),
            Self::Protected(error) => error.into_response(),
        }
    }
}
type Reply = Result<Response, ReadError>;
fn bad() -> ReadError {
    ReadError::Invalid
}
fn missing() -> ReadError {
    ReadError::Missing
}
fn internal() -> ReadError {
    ReadError::Protected(ProtectedRequestError::Internal)
}
fn body(value: &Value) -> Reply {
    let bytes = serde_json::to_vec(value).map_err(|_| internal())?;
    if bytes.len() > 1_048_576 {
        return Err(internal());
    }
    Ok(protected_request::no_store(
        ([(header::CONTENT_TYPE, "application/json")], bytes).into_response(),
    ))
}
fn millis(s: &str) -> Option<i64> {
    let n = s.parse::<i64>().ok()?;
    (n.to_string() == s).then_some(n)
}
fn scope(value: impl Serialize) -> Result<String, ReadError> {
    let mut result = String::with_capacity(64);
    for b in Sha256::digest(serde_json::to_vec(&value).map_err(|_| internal())?) {
        write!(&mut result, "{b:02x}").expect("writing to String cannot fail");
    }
    Ok(result)
}
async fn authorize<A: RequestAuthenticator>(
    c: &Context<A>,
    h: &HeaderMap,
    tenant: Option<&str>,
) -> Result<AuthorizedTenant<ReadTelemetry>, ReadError> {
    let actor = protected_request::authenticate(h, &c.authentication)
        .await
        .map_err(ReadError::Protected)?;
    let tenant = tenant
        .and_then(|t| TenantId::new(t).ok())
        .ok_or(ReadError::Protected(ProtectedRequestError::Forbidden))?;
    c.authorization
        .authorize::<ReadTelemetry>(actor, tenant)
        .await
        .map_err(|e| match e {
            TenantAuthorizationError::Denied => {
                ReadError::Protected(ProtectedRequestError::Forbidden)
            }
            TenantAuthorizationError::Policy(_) => internal(),
        })
}
async fn list<A: RequestAuthenticator>(
    ExtractState(c): ExtractState<Arc<Context<A>>>,
    h: HeaderMap,
    path: ScopePath,
    query: Result<Query<Parameters>, QueryRejection>,
) -> Reply {
    let auth = authorize(&c, &h, path.as_ref().ok().map(|Path((t, _))| t.as_str())).await?;
    let Path((_, asset)) = path.map_err(|_| bad())?;
    let asset = AssetId::new(asset).map_err(|_| bad())?;
    let Query(p) = query.map_err(|_| bad())?;
    let start = millis(&p.start_at_ms).ok_or_else(bad)?;
    let end = millis(&p.end_at_ms).ok_or_else(bad)?;
    let limit = p.limit.unwrap_or(50);
    if !(1..=100).contains(&limit)
        || !(1..=604_800_000).contains(&(i128::from(end) - i128::from(start)))
    {
        return Err(bad());
    }
    let binding = scope((
        "evaluations-v1",
        auth.tenant_id().as_str(),
        asset.as_str(),
        start,
        end,
    ))?;
    let after = if let Some(token) = p.after {
        let k: Key = cursor_codec::decode(&token).ok_or_else(bad)?;
        if k.scope != binding
            || k.index != 0
            || k.event < start
            || k.event >= end
            || CalculatorEvaluationId::new(&k.evaluation).is_err()
        {
            return Err(bad());
        }
        Some((k.event, k.evaluation))
    } else {
        None
    };
    let mut rows = c
        .store
        .list(&auth, &asset, start, end, limit, after)
        .await
        .map_err(|_| internal())?;
    let more = rows.len() > usize::from(limit);
    rows.truncate(usize::from(limit));
    let next = if more {
        let r = rows.last().ok_or_else(internal)?;
        Some(
            cursor_codec::encode(&Key {
                scope: binding,
                event: r.replay["end"].as_i64().ok_or_else(internal)?,
                evaluation: r.id.clone(),
                index: 0,
            })
            .map_err(|_| internal())?,
        )
    } else {
        None
    };
    let evaluations = rows
        .iter()
        .map(dto::summary)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|()| internal())?;
    body(&json!({"evaluations":evaluations,"next_after":next}))
}
async fn find<A: RequestAuthenticator>(
    ExtractState(c): ExtractState<Arc<Context<A>>>,
    h: HeaderMap,
    path: ExactPath,
    query: Result<Query<Empty>, QueryRejection>,
) -> Reply {
    let auth = authorize(&c, &h, path.as_ref().ok().map(|Path((t, _, _))| t.as_str())).await?;
    query.map_err(|_| bad())?;
    let Path((_, asset, id)) = path.map_err(|_| bad())?;
    let asset = AssetId::new(asset).map_err(|_| bad())?;
    CalculatorEvaluationId::new(&id).map_err(|_| bad())?;
    let row = c
        .store
        .find(&auth, &asset, &id, true)
        .await
        .map_err(|_| internal())?
        .ok_or_else(missing)?;
    body(&dto::exact(&row).map_err(|()| internal())?)
}
async fn outputs<A: RequestAuthenticator>(
    ExtractState(c): ExtractState<Arc<Context<A>>>,
    h: HeaderMap,
    path: ExactPath,
    query: Result<Query<OutputParameters>, QueryRejection>,
) -> Reply {
    let auth = authorize(&c, &h, path.as_ref().ok().map(|Path((t, _, _))| t.as_str())).await?;
    let Path((_, asset, id)) = path.map_err(|_| bad())?;
    let asset = AssetId::new(asset).map_err(|_| bad())?;
    CalculatorEvaluationId::new(&id).map_err(|_| bad())?;
    let Query(p) = query.map_err(|_| bad())?;
    let limit = p.limit.unwrap_or(20);
    if !(1..=20).contains(&limit) {
        return Err(bad());
    }
    let binding = scope((
        "outputs-v1",
        auth.tenant_id().as_str(),
        asset.as_str(),
        id.as_str(),
    ))?;
    let after = if let Some(token) = p.after {
        let k: Key = cursor_codec::decode(&token).ok_or_else(bad)?;
        if k.scope != binding || k.event != 0 || k.evaluation != id || k.index >= 1024 {
            return Err(bad());
        }
        Some(k.index)
    } else {
        None
    };
    let evaluation = c
        .store
        .find(&auth, &asset, &id, false)
        .await
        .map_err(|_| internal())?
        .ok_or_else(missing)?;
    let mut rows = c
        .store
        .outputs(&auth, &asset, &id, limit, after)
        .await
        .map_err(|_| internal())?;
    let more = rows.len() > usize::from(limit);
    rows.truncate(usize::from(limit));
    let next = if more {
        let r = rows.last().ok_or_else(internal)?;
        Some(
            cursor_codec::encode(&Key {
                scope: binding,
                event: 0,
                evaluation: id,
                index: u32::try_from(r.index).map_err(|_| internal())?,
            })
            .map_err(|_| internal())?,
        )
    } else {
        None
    };
    let outputs = rows
        .iter()
        .map(|r| dto::output(r, &evaluation))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|()| internal())?;
    body(&json!({"outputs":outputs,"next_after":next}))
}
