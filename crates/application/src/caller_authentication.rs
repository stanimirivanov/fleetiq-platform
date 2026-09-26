use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, ExternalIdentity};

use crate::ActorIdentityResolver;

/// Validates provider-specific credentials and establishes an external identity.
///
/// Implementations own token, certificate, session, signature, issuer,
/// audience, expiry, nonce, and key validation appropriate to their credential
/// type. Credentials and provider claims remain outside domain values and must
/// not be included in routine errors or debug output. `Ok(None)` means the
/// credentials were rejected without disclosing why.
pub trait ExternalIdentityAuthenticator: Send + Sync {
    /// Provider-specific credential presented by the caller.
    type Credential: Sync;

    /// Operational failure while validating credentials.
    type Error: Error + Send + Sync + 'static;

    /// Authenticates credentials and returns their trusted external identity.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when validation cannot be completed.
    /// Invalid, expired, or otherwise rejected credentials return `Ok(None)`.
    fn authenticate<'a>(
        &'a self,
        credential: &'a Self::Credential,
    ) -> impl Future<Output = Result<Option<ExternalIdentity>, Self::Error>> + Send + 'a;
}

/// Evidence that a caller was authenticated and mapped to an internal actor.
///
/// The type has no public constructor. It is produced only after
/// [`CallerAuthenticationService`] validates credentials and resolves their
/// trusted external identity. Authentication alone grants no tenant action.
#[derive(Debug, Eq, PartialEq)]
#[must_use = "authenticated actor evidence must be consumed by request authorization"]
pub struct AuthenticatedActor {
    actor_id: ActorId,
}

impl AuthenticatedActor {
    #[cfg(test)]
    pub(crate) const fn from_trusted_actor_id(actor_id: ActorId) -> Self {
        Self { actor_id }
    }

    /// Returns the stable internal actor identity.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }

    /// Consumes the evidence and returns the stable internal actor identity.
    pub fn into_actor_id(self) -> ActorId {
        self.actor_id
    }
}

/// Authenticates a caller and resolves it to a stable internal actor.
///
/// Credential validation always precedes identity resolution. Rejected
/// credentials never reach the resolver, and a valid but unprovisioned
/// external identity does not create an actor automatically.
#[derive(Clone, Debug)]
pub struct CallerAuthenticationService<A, R> {
    authenticator: A,
    resolver: R,
}

impl<A, R> CallerAuthenticationService<A, R> {
    /// Creates the service from credential validation and actor-resolution ports.
    pub const fn new(authenticator: A, resolver: R) -> Self {
        Self {
            authenticator,
            resolver,
        }
    }
}

impl<A, R> CallerAuthenticationService<A, R>
where
    A: ExternalIdentityAuthenticator,
    R: ActorIdentityResolver,
{
    /// Authenticates credentials and resolves their registered internal actor.
    ///
    /// A transport must map both rejected credentials and unregistered
    /// identities to the same generic unauthorized response so callers cannot
    /// enumerate provisioned principals.
    ///
    /// # Errors
    ///
    /// Returns [`CallerAuthenticationError::Rejected`] for invalid credentials,
    /// [`CallerAuthenticationError::Unregistered`] when valid credentials have
    /// no actor mapping, or preserves an operational authenticator or resolver
    /// failure.
    pub async fn authenticate(
        &self,
        credential: &A::Credential,
    ) -> Result<AuthenticatedActor, CallerAuthenticationError<A::Error, R::Error>> {
        let identity = self
            .authenticator
            .authenticate(credential)
            .await
            .map_err(CallerAuthenticationError::Authenticator)?
            .ok_or(CallerAuthenticationError::Rejected)?;

        let actor_id = self
            .resolver
            .resolve(&identity)
            .await
            .map_err(CallerAuthenticationError::Resolver)?
            .ok_or(CallerAuthenticationError::Unregistered)?;

        Ok(AuthenticatedActor { actor_id })
    }
}

/// Failure to authenticate and resolve a caller.
#[derive(Debug)]
pub enum CallerAuthenticationError<A, R> {
    /// Credentials were invalid, expired, or otherwise rejected.
    Rejected,
    /// Valid credentials identified a principal with no internal actor mapping.
    Unregistered,
    /// Credential validation could not be completed.
    Authenticator(A),
    /// The external identity mapping could not be read.
    Resolver(R),
}

impl<A, R> fmt::Display for CallerAuthenticationError<A, R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected | Self::Unregistered => {
                formatter.write_str("caller authentication failed")
            }
            Self::Authenticator(_) => {
                formatter.write_str("credential validation could not be completed")
            }
            Self::Resolver(_) => {
                formatter.write_str("authenticated identity could not be resolved")
            }
        }
    }
}

impl<A, R> Error for CallerAuthenticationError<A, R>
where
    A: Error + 'static,
    R: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Authenticator(error) => Some(error),
            Self::Resolver(error) => Some(error),
            Self::Rejected | Self::Unregistered => None,
        }
    }
}

#[cfg(test)]
mod tests;
