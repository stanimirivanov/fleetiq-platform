//! Protected immutable alert-rule revision and explicit activation boundary.
use crate::{
    mutation_guard::MutationGuard,
    protected_request::{self, ProtectedRequestError, RequestAuthenticator},
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, State as ExtractState,
        rejection::{JsonRejection, PathRejection},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use fleetiq_access::{
    AuthorizedTenant, ManageAlerts, ReadAlerts, TenantActionMarker, TenantAuthorizationError,
    TenantAuthorizationService,
};
use fleetiq_analytics::{
    AlertRuleDefinition, AlertRuleId, CalculatorId, CalculatorRef, CalculatorVersion,
};
use fleetiq_domain::{AssetId, TenantId};
use fleetiq_postgres::{AlertRuleError, PostgresAlertRules, PostgresTenantAccessPolicy};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
#[cfg(test)]
mod tests;
struct State<A> {
    authentication: A,
    mutation: MutationGuard,
    authorization: TenantAuthorizationService<PostgresTenantAccessPolicy>,
    store: PostgresAlertRules,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new()
        .route(
            "/api/v1/tenants/{tenant_id}/alert-rules/{rule_id}/revisions/{revision}",
            get(find::<A>)
                .put(create::<A>)
                .fallback(protected_request::resource_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/alert-rules/{rule_id}/activation",
            axum::routing::put(change::<A>)
                .fallback(protected_request::resource_method_not_allowed),
        )
        .route(
            "/api/v1/tenants/{tenant_id}/alert-rules/{rule_id}",
            get(head::<A>).fallback(protected_request::resource_method_not_allowed),
        )
        .layer(DefaultBodyLimit::max(4096))
        .with_state(Arc::new(State {
            authentication,
            mutation,
            authorization: TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(
                pool.clone(),
            )),
            store: PostgresAlertRules::new(pool),
        }))
}
impl<A: RequestAuthenticator> State<A> {
    async fn authorize<M: TenantActionMarker>(
        &self,
        headers: &HeaderMap,
        tenant: Option<&str>,
        write: bool,
    ) -> Result<AuthorizedTenant<M>, ProtectedRequestError> {
        let actor = protected_request::authenticate(headers, &self.authentication).await?;
        if write {
            self.mutation.check(headers)?;
        }
        let tenant = tenant
            .and_then(|v| TenantId::new(v).ok())
            .ok_or(ProtectedRequestError::Forbidden)?;
        self.authorization
            .authorize::<M>(actor, tenant)
            .await
            .map_err(|e| match e {
                TenantAuthorizationError::Denied => ProtectedRequestError::Forbidden,
                TenantAuthorizationError::Policy(_) => ProtectedRequestError::Internal,
            })
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct RequiredUnit(Option<String>);
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Parameters {
    kind: String,
    calculator: String,
    version: String,
    scope: String,
    target: String,
    output_property: String,
    output_version: u32,
    output_index: u32,
    comparison: String,
    threshold: String,
    threshold_unit: RequiredUnit,
    hold_ms: u32,
    resolution: String,
    resolve_after_ms: u32,
}
impl Parameters {
    fn definition(self) -> Option<AlertRuleDefinition> {
        if self.kind != fleetiq_analytics::ALERT_RULE_GRAMMAR
            || self.resolution != "resolve_on_false"
            || self.threshold.len() > 256
            || !self
                .threshold
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'+' | b'-' | b'.' | b'e' | b'E'))
        {
            return None;
        }
        let d = AlertRuleDefinition {
            calculator: CalculatorRef::new(
                CalculatorId::new(self.calculator).ok()?,
                CalculatorVersion::new(self.version).ok()?,
            ),
            scope: AssetId::new(self.scope).ok()?,
            target: AssetId::new(self.target).ok()?,
            output: fleetiq_domain::PropertyDefinitionRef::new(
                fleetiq_domain::PropertyDefinitionId::new(self.output_property).ok()?,
                fleetiq_domain::DefinitionVersion::new(self.output_version).ok()?,
            ),
            output_index: self.output_index,
            comparison: fleetiq_analytics::AlertComparison::parse(&self.comparison)?,
            threshold: self.threshold.parse().ok()?,
            unit: self
                .threshold_unit
                .0
                .map(fleetiq_domain::UnitId::new)
                .transpose()
                .ok()?,
            hold_ms: self.hold_ms,
            resolution: fleetiq_analytics::AlertResolution::OnFalse,
            resolve_after_ms: self.resolve_after_ms,
        };
        d.is_valid().then_some(d)
    }
    pub(crate) fn from_definition(d: &AlertRuleDefinition) -> Self {
        Self {
            kind: fleetiq_analytics::ALERT_RULE_GRAMMAR.into(),
            calculator: d.calculator.id().to_string(),
            version: d.calculator.version().to_string(),
            scope: d.scope.to_string(),
            target: d.target.to_string(),
            output_property: d.output.id().to_string(),
            output_version: d.output.version().get(),
            output_index: d.output_index,
            comparison: d.comparison.as_str().into(),
            threshold: d.threshold.normalized().to_string(),
            threshold_unit: RequiredUnit(d.unit.as_ref().map(ToString::to_string)),
            hold_ms: d.hold_ms,
            resolution: "resolve_on_false".into(),
            resolve_after_ms: d.resolve_after_ms,
        }
    }
}
#[derive(Deserialize)]
struct RequiredRevision(Option<u32>);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    expected_generation: u32,
    revision: RequiredRevision,
}
type RevisionPath = Result<Path<(String, String, String)>, PathRejection>;
type RulePath = Result<Path<(String, String)>, PathRejection>;
fn problem(status: StatusCode, title: &'static str) -> Response {
    protected_request::problem(status, "urn:fleetiq:problem:alert-rule", title)
}
fn error(e: AlertRuleError) -> Response {
    match e {
        AlertRuleError::Forbidden => ProtectedRequestError::Forbidden.into_response(),
        AlertRuleError::Conflict => problem(
            StatusCode::CONFLICT,
            "Alert rule revision, activation or prerequisites conflict",
        ),
        AlertRuleError::Database => ProtectedRequestError::Internal.into_response(),
    }
}
fn revision_identity(path: RevisionPath) -> Option<(AlertRuleId, u32)> {
    let Path((_, id, revision)) = path.ok()?;
    Some((
        AlertRuleId::new(id).ok()?,
        u32::try_from(protected_request::parse_record_revision(&revision)?.get()).ok()?,
    ))
}
async fn find<A: RequestAuthenticator>(
    ExtractState(s): ExtractState<Arc<State<A>>>,
    headers: HeaderMap,
    path: RevisionPath,
) -> Response {
    let auth = match s
        .authorize::<ReadAlerts>(
            &headers,
            path.as_ref().ok().map(|Path((t, _, _))| t.as_str()),
            false,
        )
        .await
    {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    let Some((id, revision)) = revision_identity(path) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid rule identity");
    };
    match s.store.find(auth, &id, revision).await {
        Ok(Some(r)) => protected_request::no_store(
            Json(json!({
                "revision": r.revision,
                "definition": Parameters::from_definition(&r.definition),
                "actor_id": r.actor,
                "recorded_at_ms": r.recorded_at,
            }))
            .into_response(),
        ),
        Ok(None) => problem(StatusCode::NOT_FOUND, "Alert rule revision not found"),
        Err(e) => error(e),
    }
}
async fn create<A: RequestAuthenticator>(
    ExtractState(s): ExtractState<Arc<State<A>>>,
    headers: HeaderMap,
    path: RevisionPath,
    body: Result<Json<Parameters>, JsonRejection>,
) -> Response {
    let auth = match s
        .authorize::<ManageAlerts>(
            &headers,
            path.as_ref().ok().map(|Path((t, _, _))| t.as_str()),
            true,
        )
        .await
    {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    let (Some((id, revision)), Ok(Json(p))) = (revision_identity(path), body) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid rule request");
    };
    let Some(definition) = p.definition() else {
        return problem(StatusCode::BAD_REQUEST, "Invalid rule bounds");
    };
    match s.store.create(auth, &id, revision, &definition).await {
        Ok(()) => protected_request::no_store(Json(json!({"revision":revision})).into_response()),
        Err(e) => error(e),
    }
}
async fn head<A: RequestAuthenticator>(
    ExtractState(s): ExtractState<Arc<State<A>>>,
    headers: HeaderMap,
    path: RulePath,
) -> Response {
    let auth = match s
        .authorize::<ReadAlerts>(
            &headers,
            path.as_ref().ok().map(|Path((t, _))| t.as_str()),
            false,
        )
        .await
    {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    let Some(id) = path
        .ok()
        .and_then(|Path((_, id))| AlertRuleId::new(id).ok())
    else {
        return problem(StatusCode::BAD_REQUEST, "Invalid rule identity");
    };
    match s.store.head(auth, &id).await {
        Ok(Some(r)) => protected_request::no_store(
            Json(json!({
                "latest_revision": r.latest_revision,
                "activation": activation_value(&r.activation),
            }))
            .into_response(),
        ),
        Ok(None) => problem(StatusCode::NOT_FOUND, "Alert rule not found"),
        Err(e) => error(e),
    }
}
fn activation_value(r: &fleetiq_postgres::AlertRuleActivationRecord) -> serde_json::Value {
    json!({
        "generation": r.generation,
        "revision": r.revision,
        "actor_id": r.actor,
        "recorded_at_ms": r.recorded_at,
    })
}
fn receipt(r: &fleetiq_postgres::AlertRuleActivationRecord) -> Response {
    protected_request::no_store(Json(activation_value(r)).into_response())
}
async fn change<A: RequestAuthenticator>(
    ExtractState(s): ExtractState<Arc<State<A>>>,
    headers: HeaderMap,
    path: RulePath,
    body: Result<Json<Activation>, JsonRejection>,
) -> Response {
    let auth = match s
        .authorize::<ManageAlerts>(
            &headers,
            path.as_ref().ok().map(|Path((t, _))| t.as_str()),
            true,
        )
        .await
    {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    let (Some(id), Ok(Json(p))) = (
        path.ok()
            .and_then(|Path((_, id))| AlertRuleId::new(id).ok()),
        body,
    ) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid activation request");
    };
    if p.revision.0 == Some(0) {
        return problem(StatusCode::BAD_REQUEST, "Invalid activation revision");
    }
    match s
        .store
        .activate(auth, &id, p.expected_generation, p.revision.0)
        .await
    {
        Ok(r) => receipt(&r),
        Err(e) => error(e),
    }
}
