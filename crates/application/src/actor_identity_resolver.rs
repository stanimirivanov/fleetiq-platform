use std::{error::Error, future::Future};

use fleetiq_domain::{ActorId, ExternalIdentity};

/// Resolves an externally authenticated identity to a stable internal actor.
///
/// Implementations perform an exact lookup of the authority and opaque subject.
/// Absence is represented by `Ok(None)` and must fail authentication without
/// revealing whether another authority or subject is registered. This port
/// does not authenticate credentials or grant tenant access.
pub trait ActorIdentityResolver: Send + Sync {
    /// Infrastructure-specific failure returned by this resolver.
    type Error: Error + Send + Sync + 'static;

    /// Resolves one exact external identity.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when identity mapping cannot be read.
    fn resolve<'a>(
        &'a self,
        identity: &'a ExternalIdentity,
    ) -> impl Future<Output = Result<Option<ActorId>, Self::Error>> + Send + 'a;
}
