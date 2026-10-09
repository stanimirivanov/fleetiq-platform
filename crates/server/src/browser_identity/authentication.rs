//! Authentication from opaque session evidence and the current identity mapping.

use std::{convert::Infallible, future::ready};

use axum::http::{HeaderMap, header};
use fleetiq_access::{
    AuthenticatedActor, CallerAuthenticationError, CallerAuthenticationService,
    ExternalIdentityAuthenticator,
};
use fleetiq_domain::ExternalIdentity;
use fleetiq_postgres::{PostgresActorIdentityResolver, PostgresBrowserIdentityRepository};
use sqlx::PgPool;

use crate::protected_request::{ProtectedRequestError, RequestAuthenticator};

use super::credential::{SESSION_COOKIE, credential, digest, now, provider_digest};

/// Authenticates browser requests and binds every session to its original actor.
///
/// Current actor activation and identity assignment are resolved on every request.
/// Tenant grants remain the responsibility of the protected operation's policy.
#[derive(Clone)]
pub(crate) struct BrowserRequestAuthenticator {
    repository: PostgresBrowserIdentityRepository,
    resolver: PostgresActorIdentityResolver,
    provider_digest: String,
}

impl BrowserRequestAuthenticator {
    pub(crate) fn new(pool: PgPool, config: &super::BrowserIdentityConfig) -> Self {
        Self {
            repository: PostgresBrowserIdentityRepository::new(pool.clone()),
            resolver: PostgresActorIdentityResolver::new(pool),
            provider_digest: provider_digest(config),
        }
    }

    pub(super) async fn authenticate_session(
        &self,
        headers: &HeaderMap,
    ) -> Result<(AuthenticatedActor, i64), ProtectedRequestError> {
        // Reject ambiguous credential channels. Browser mode never accepts the
        // separately configured workload bearer used by machine integrations.
        if headers.contains_key(header::AUTHORIZATION) {
            return Err(ProtectedRequestError::BrowserUnauthorized);
        }
        let secret = credential(headers, SESSION_COOKIE)?
            .ok_or(ProtectedRequestError::BrowserUnauthorized)?;
        let session = self
            .repository
            .find_session(&digest(&secret), now()?)
            .await
            .map_err(|_| ProtectedRequestError::Internal)?
            .ok_or(ProtectedRequestError::BrowserUnauthorized)?;
        if session.provider_digest != self.provider_digest {
            return Err(ProtectedRequestError::BrowserUnauthorized);
        }
        let actor = CallerAuthenticationService::new(TrustedSessionIdentity, self.resolver.clone())
            .authenticate(&session.identity)
            .await
            .map_err(|error| authentication_error(&error))?;
        if actor.actor_id() != &session.actor_id {
            return Err(ProtectedRequestError::BrowserUnauthorized);
        }
        Ok((actor, session.expires_at.get()))
    }
}

impl RequestAuthenticator for BrowserRequestAuthenticator {
    async fn authenticate(
        &self,
        headers: &HeaderMap,
    ) -> Result<AuthenticatedActor, ProtectedRequestError> {
        self.authenticate_session(headers)
            .await
            .map(|(actor, _)| actor)
    }
}

/// Private adapter: only an unexpired database session can supply this identity.
///
/// It is deliberately inaccessible to HTTP input. `CallerAuthenticationService`
/// supplies the evidence constructor and performs the current exact resolution.
struct TrustedSessionIdentity;

impl ExternalIdentityAuthenticator for TrustedSessionIdentity {
    type Credential = ExternalIdentity;
    type Error = Infallible;

    fn authenticate<'a>(
        &'a self,
        identity: &'a ExternalIdentity,
    ) -> impl Future<Output = Result<Option<ExternalIdentity>, Self::Error>> + Send + 'a {
        ready(Ok(Some(identity.clone())))
    }
}

fn authentication_error<E>(
    error: &CallerAuthenticationError<Infallible, E>,
) -> ProtectedRequestError {
    match error {
        CallerAuthenticationError::Rejected | CallerAuthenticationError::Unregistered => {
            ProtectedRequestError::BrowserUnauthorized
        }
        CallerAuthenticationError::Authenticator(_) | CallerAuthenticationError::Resolver(_) => {
            ProtectedRequestError::Internal
        }
    }
}
