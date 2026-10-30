//! Protected configuration revision and scheduling activation boundary.
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
    AuthorizedTenant, ManageAnalytics, ReadMetadata, TenantActionMarker, TenantAuthorizationError,
    TenantAuthorizationService,
};
use fleetiq_analytics::{
    CalculatorConfigurationId, CalculatorId, CalculatorRef, CalculatorSchedule, CalculatorVersion,
};
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_postgres::{
    CalculatorConfigurationError, PostgresCalculatorConfiguration, PostgresTenantAccessPolicy,
};
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
    store: PostgresCalculatorConfiguration,
}
pub(crate) fn router<A: RequestAuthenticator + 'static>(
    pool: PgPool,
    authentication: A,
    mutation: MutationGuard,
) -> Router {
    Router::new().route("/api/v1/tenants/{tenant_id}/calculator-configurations/{configuration_id}/revisions/{revision}",get(find::<A>).put(create::<A>).fallback(protected_request::resource_method_not_allowed))
    .route("/api/v1/tenants/{tenant_id}/calculator-configurations/{configuration_id}/activation",get(activation::<A>).put(change::<A>).fallback(protected_request::resource_method_not_allowed))
    .layer(DefaultBodyLimit::max(4096)).with_state(Arc::new(State{authentication,mutation,authorization:TenantAuthorizationService::new(PostgresTenantAccessPolicy::new(pool.clone())),store:PostgresCalculatorConfiguration::new(pool)}))
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
#[serde(deny_unknown_fields)]
struct Parameters {
    calculator: String,
    version: String,
    scope: String,
    target: String,
    first_start: i64,
    window_ms: u32,
    lateness_ms: u32,
    minimum_samples: u16,
}
impl Parameters {
    fn schedule(
        self,
        tenant: TenantId,
        id: CalculatorConfigurationId,
    ) -> Option<CalculatorSchedule> {
        let s = CalculatorSchedule {
            tenant,
            id: id.clone(),
            configuration: id,
            calculator: CalculatorRef::new(
                CalculatorId::new(self.calculator).ok()?,
                CalculatorVersion::new(self.version).ok()?,
            ),
            scope: AssetId::new(self.scope).ok()?,
            target: AssetId::new(self.target).ok()?,
            first_start: UnixTimeMillis::new(self.first_start),
            window_ms: self.window_ms,
            lateness_ms: self.lateness_ms,
            minimum_samples: self.minimum_samples,
        };
        s.is_valid().then_some(s)
    }
    fn from_schedule(s: &CalculatorSchedule) -> Self {
        Self {
            calculator: s.calculator.id().to_string(),
            version: s.calculator.version().to_string(),
            scope: s.scope.to_string(),
            target: s.target.to_string(),
            first_start: s.first_start.get(),
            window_ms: s.window_ms,
            lateness_ms: s.lateness_ms,
            minimum_samples: s.minimum_samples,
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
type ConfigPath = Result<Path<(String, String)>, PathRejection>;
fn problem(status: StatusCode, title: &'static str) -> Response {
    protected_request::problem(
        status,
        "urn:fleetiq:problem:calculator-configuration",
        title,
    )
}
fn error(e: CalculatorConfigurationError) -> Response {
    match e {
        CalculatorConfigurationError::Forbidden => ProtectedRequestError::Forbidden.into_response(),
        CalculatorConfigurationError::Conflict => problem(
            StatusCode::CONFLICT,
            "Configuration revision, activation or prerequisites conflict",
        ),
        CalculatorConfigurationError::Database => ProtectedRequestError::Internal.into_response(),
    }
}
fn revision_identity(path: RevisionPath) -> Option<(CalculatorConfigurationId, u32)> {
    let Path((_, id, revision)) = path.ok()?;
    Some((
        CalculatorConfigurationId::new(id).ok()?,
        u32::try_from(protected_request::parse_record_revision(&revision)?.get()).ok()?,
    ))
}
async fn find<A: RequestAuthenticator>(
    ExtractState(s): ExtractState<Arc<State<A>>>,
    headers: HeaderMap,
    path: RevisionPath,
) -> Response {
    let auth = match s
        .authorize::<ReadMetadata>(
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
        return problem(StatusCode::BAD_REQUEST, "Invalid configuration identity");
    };
    match s.store.find(auth,&id,revision).await{Ok(Some(r))=>protected_request::no_store(Json(json!({"revision":r.revision,"parameters":Parameters::from_schedule(&r.schedule),"actor_id":r.actor,"recorded_at_ms":r.recorded_at})).into_response()),Ok(None)=>problem(StatusCode::NOT_FOUND,"Configuration revision not found"),Err(e)=>error(e)}
}
async fn create<A: RequestAuthenticator>(
    ExtractState(s): ExtractState<Arc<State<A>>>,
    headers: HeaderMap,
    path: RevisionPath,
    body: Result<Json<Parameters>, JsonRejection>,
) -> Response {
    let auth = match s
        .authorize::<ManageAnalytics>(
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
        return problem(StatusCode::BAD_REQUEST, "Invalid configuration request");
    };
    let Some(schedule) = p.schedule(auth.tenant_id().clone(), id.clone()) else {
        return problem(StatusCode::BAD_REQUEST, "Invalid configuration bounds");
    };
    match s.store.create(auth, &id, revision, &schedule).await {
        Ok(()) => protected_request::no_store(Json(json!({"revision":revision})).into_response()),
        Err(e) => error(e),
    }
}
async fn activation<A: RequestAuthenticator>(
    ExtractState(s): ExtractState<Arc<State<A>>>,
    headers: HeaderMap,
    path: ConfigPath,
) -> Response {
    let auth = match s
        .authorize::<ReadMetadata>(
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
        .and_then(|Path((_, id))| CalculatorConfigurationId::new(id).ok())
    else {
        return problem(StatusCode::BAD_REQUEST, "Invalid configuration identity");
    };
    match s.store.activation(auth, &id).await {
        Ok(Some(r)) => receipt(&r),
        Ok(None) => problem(StatusCode::NOT_FOUND, "Configuration not found"),
        Err(e) => error(e),
    }
}
fn receipt(r: &fleetiq_postgres::CalculatorActivationRecord) -> Response {
    protected_request::no_store(Json(json!({"generation":r.generation,"revision":r.revision,"actor_id":r.actor,"recorded_at_ms":r.recorded_at})).into_response())
}
async fn change<A: RequestAuthenticator>(
    ExtractState(s): ExtractState<Arc<State<A>>>,
    headers: HeaderMap,
    path: ConfigPath,
    body: Result<Json<Activation>, JsonRejection>,
) -> Response {
    let auth = match s
        .authorize::<ManageAnalytics>(
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
            .and_then(|Path((_, id))| CalculatorConfigurationId::new(id).ok()),
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
