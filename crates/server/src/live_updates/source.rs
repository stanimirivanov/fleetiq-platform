//! Fresh HTTP authentication and tenant policy wrap each consistent projection read.
use super::Context;
use crate::protected_request::{self, ProtectedRequestError, RequestAuthenticator};
use axum::http::HeaderMap;
use fleetiq_access::{AuthorizedTenant, ReadTelemetry, TenantAuthorizationError};
use fleetiq_domain::{ActorId, SignalTarget, TenantId};
use fleetiq_projection::CurrentPropertySnapshot;
use std::{future::Future, sync::Arc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReadFailure {
    AuthenticationRequired,
    AccessDenied,
    ScopeMissing,
    Unavailable,
}
pub(super) trait ReadSource: Send + Sync + 'static {
    fn read(&self) -> impl Future<Output = Result<CurrentPropertySnapshot, ReadFailure>> + Send;
}
pub(super) struct AuthorizedSource<A> {
    pub context: Arc<Context<A>>,
    pub headers: HeaderMap,
    pub tenant: TenantId,
    pub target: SignalTarget,
    pub actor_id: ActorId,
}
impl<A: RequestAuthenticator + 'static> AuthorizedSource<A> {
    async fn grant(&self) -> Result<AuthorizedTenant<ReadTelemetry>, ReadFailure> {
        let actor = protected_request::authenticate(&self.headers, &self.context.authentication)
            .await
            .map_err(|error| match error {
                ProtectedRequestError::Unauthorized
                | ProtectedRequestError::BrowserUnauthorized => ReadFailure::AuthenticationRequired,
                _ => ReadFailure::Unavailable,
            })?;
        if actor.actor_id() != &self.actor_id {
            return Err(ReadFailure::AuthenticationRequired);
        }
        self.context
            .authorization
            .authorize::<ReadTelemetry>(actor, self.tenant.clone())
            .await
            .map_err(|error| match error {
                TenantAuthorizationError::Denied => ReadFailure::AccessDenied,
                TenantAuthorizationError::Policy(_) => ReadFailure::Unavailable,
            })
    }
}
impl<A: RequestAuthenticator + 'static> ReadSource for AuthorizedSource<A> {
    async fn read(&self) -> Result<CurrentPropertySnapshot, ReadFailure> {
        let grant = self.grant().await?;
        let snapshot = self.context.current.snapshot(grant, &self.target).await;
        // Storage can suspend. Recheck the original actor and grant after that
        // await as well, rather than emitting under authority from before I/O.
        let _fresh_grant = self.grant().await?;
        snapshot
            .map_err(|_| ReadFailure::Unavailable)?
            .ok_or(ReadFailure::ScopeMissing)
    }
}
