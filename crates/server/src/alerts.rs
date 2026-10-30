//! Protected observed-alert queue, exact detail and bounded immutable history.
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
    AuthorizedTenant, ReadAlerts, TenantAuthorizationError, TenantAuthorizationService,
};
use fleetiq_analytics::{AlertPhase, AlertRuleId};
use fleetiq_domain::TenantId;
use fleetiq_postgres::{PostgresAlertReads, PostgresTenantAccessPolicy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{fmt::Write, sync::Arc};
struct Context<A> {
    authentication: A,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    store: PostgresAlertReads,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(pool: PgPool, authentication: A) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/alerts",
            get(list::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/alerts/{rule_id}/revisions/{revision}",
            get(find::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/alerts/{rule_id}/revisions/{revision}/transitions",
            get(history::<A>).fallback(protected_request::protected_method_not_allowed),
        )
        .with_state(Arc::new(Context {
            authentication,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            store: PostgresAlertReads::new(pool),
        }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    phase: Option<String>,
    limit: Option<u16>,
    after: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageParameters {
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
    rule: String,
    revision: u32,
    version: i64,
}
type TenantPath = Result<Path<String>, PathRejection>;
type ExactPath = Result<Path<(String, String, String)>, PathRejection>;
#[derive(Debug)]
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
                "Invalid alert read request",
            ),
            Self::Missing => protected_request::problem(
                StatusCode::NOT_FOUND,
                protected_request::NOT_FOUND,
                "Alert not found",
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
) -> Result<AuthorizedTenant<ReadAlerts>, ReadError> {
    let actor = protected_request::authenticate(h, &c.authentication)
        .await
        .map_err(ReadError::Protected)?;
    let tenant = tenant
        .and_then(|t| TenantId::new(t).ok())
        .ok_or(ReadError::Protected(ProtectedRequestError::Forbidden))?;
    c.authorization
        .authorize::<ReadAlerts>(actor, tenant)
        .await
        .map_err(|e| match e {
            TenantAuthorizationError::Denied => {
                ReadError::Protected(ProtectedRequestError::Forbidden)
            }
            TenantAuthorizationError::Policy(_) => internal(),
        })
}

fn page_limit(limit: Option<u16>) -> Result<u16, ReadError> {
    let limit = limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(bad());
    }
    Ok(limit)
}
fn identity(path: ExactPath) -> Result<(AlertRuleId, u32), ReadError> {
    let Path((_, rule, revision)) = path.map_err(|_| bad())?;
    let n = revision.parse::<u32>().map_err(|_| bad())?;
    if n == 0 || n.to_string() != revision {
        return Err(bad());
    }
    Ok((AlertRuleId::new(rule).map_err(|_| bad())?, n))
}
async fn list<A: RequestAuthenticator>(
    ExtractState(c): ExtractState<Arc<Context<A>>>,
    h: HeaderMap,
    path: TenantPath,
    query: Result<Query<Parameters>, QueryRejection>,
) -> Reply {
    let auth = authorize(&c, &h, path.as_ref().ok().map(|Path(t)| t.as_str())).await?;
    let Query(p) = query.map_err(|_| bad())?;
    let phase = match p.phase.as_deref() {
        None => None,
        Some("inactive") => Some(AlertPhase::Inactive),
        Some("pending") => Some(AlertPhase::Pending),
        Some("firing") => Some(AlertPhase::Firing),
        Some("resolved") => Some(AlertPhase::Resolved),
        _ => return Err(bad()),
    };
    let limit = page_limit(p.limit)?;
    let binding = scope((
        "alert-queue-v1",
        auth.tenant_id().as_str(),
        phase.map(AlertPhase::as_str),
    ))?;
    let after = if let Some(token) = p.after {
        let k: Key = cursor_codec::decode(&token).ok_or_else(bad)?;
        if k.scope != binding || k.version != 0 || k.revision == 0 {
            return Err(bad());
        }
        Some((AlertRuleId::new(k.rule).map_err(|_| bad())?, k.revision))
    } else {
        None
    };
    let mut rows = c
        .store
        .list(&auth, phase, limit, after)
        .await
        .map_err(|_| internal())?;
    let more = rows.len() > usize::from(limit);
    rows.truncate(usize::from(limit));
    let next = if more {
        let r = rows.last().ok_or_else(internal)?;
        Some(
            cursor_codec::encode(&Key {
                scope: binding,
                rule: r.rule.to_string(),
                revision: r.revision,
                version: 0,
            })
            .map_err(|_| internal())?,
        )
    } else {
        None
    };
    let alerts = rows
        .iter()
        .map(dto::summary)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|()| internal())?;
    body(&json!({"alerts":alerts,"next_after":next}))
}
async fn find<A: RequestAuthenticator>(
    ExtractState(c): ExtractState<Arc<Context<A>>>,
    h: HeaderMap,
    path: ExactPath,
    query: Result<Query<Empty>, QueryRejection>,
) -> Reply {
    let auth = authorize(&c, &h, path.as_ref().ok().map(|Path((t, _, _))| t.as_str())).await?;
    query.map_err(|_| bad())?;
    let (rule, revision) = identity(path)?;
    let row = c
        .store
        .find(&auth, &rule, revision)
        .await
        .map_err(|_| internal())?
        .ok_or_else(missing)?;
    body(
        &json!({"alert":dto::summary(&row).map_err(|()|internal())?,"definition":crate::alert_rules::Parameters::from_definition(&row.definition)}),
    )
}
async fn history<A: RequestAuthenticator>(
    ExtractState(c): ExtractState<Arc<Context<A>>>,
    h: HeaderMap,
    path: ExactPath,
    query: Result<Query<PageParameters>, QueryRejection>,
) -> Reply {
    let auth = authorize(&c, &h, path.as_ref().ok().map(|Path((t, _, _))| t.as_str())).await?;
    let (rule, revision) = identity(path)?;
    let Query(p) = query.map_err(|_| bad())?;
    let limit = page_limit(p.limit)?;
    let binding = scope((
        "alert-transitions-v1",
        auth.tenant_id().as_str(),
        rule.as_str(),
        revision,
    ))?;
    let after = if let Some(token) = p.after {
        let k: Key = cursor_codec::decode(&token).ok_or_else(bad)?;
        if k.scope != binding || k.rule != rule.as_str() || k.revision != revision || k.version <= 0
        {
            return Err(bad());
        }
        Some(k.version)
    } else {
        None
    };
    let row = c
        .store
        .find(&auth, &rule, revision)
        .await
        .map_err(|_| internal())?
        .ok_or_else(missing)?;
    let mut rows = c
        .store
        .transitions(&auth, &rule, revision, limit, after)
        .await
        .map_err(|_| internal())?;
    let more = rows.len() > usize::from(limit);
    rows.truncate(usize::from(limit));
    let next = if more {
        let r = rows.last().ok_or_else(internal)?;
        Some(
            cursor_codec::encode(&Key {
                scope: binding,
                rule: rule.to_string(),
                revision,
                version: r.version,
            })
            .map_err(|_| internal())?,
        )
    } else {
        None
    };
    let transitions = rows
        .iter()
        .map(|r| dto::transition(r, &row.definition))
        .collect::<Vec<_>>();
    body(&json!({"transitions":transitions,"next_after":next}))
}
