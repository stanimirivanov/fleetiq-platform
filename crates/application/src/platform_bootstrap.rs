use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, ExternalIdentity, UnixTimeMillis};

/// Trusted input for establishing the first platform operator.
///
/// This request is an installation bootstrap boundary, not an authenticated
/// product command. An inbound adapter must restrict it to an explicitly
/// controlled operator workflow and must construct the external identity only
/// from trusted identity-provider configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformBootstrapRequest {
    actor_id: ActorId,
    external_identity: ExternalIdentity,
    recorded_at: UnixTimeMillis,
}

impl PlatformBootstrapRequest {
    /// Creates the complete immutable bootstrap request.
    pub const fn new(
        actor_id: ActorId,
        external_identity: ExternalIdentity,
        recorded_at: UnixTimeMillis,
    ) -> Self {
        Self {
            actor_id,
            external_identity,
            recorded_at,
        }
    }

    /// Returns the stable internal identity to establish.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }

    /// Returns the exact trusted external identity to bind to the actor.
    pub const fn external_identity(&self) -> &ExternalIdentity {
        &self.external_identity
    }

    /// Returns the trusted host time recorded for the bootstrap operation.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }
}

/// Durable disposition of the one-time platform bootstrap operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformBootstrapOutcome {
    /// This call established the first platform operator.
    Bootstrapped,
    /// The exact same bootstrap operation had already completed.
    AlreadyBootstrapped,
}

/// Persistence boundary for one-time platform bootstrap and immutable evidence.
pub trait PlatformBootstrapRepository: Send + Sync {
    /// Storage, invariant, or conflicting-bootstrap failure.
    type Error: Error + Send + Sync + 'static;

    /// Atomically establishes the initial actor, identity mapping, grant, and audit fact.
    ///
    /// An exact retry returns [`PlatformBootstrapOutcome::AlreadyBootstrapped`].
    /// Once a different actor or external identity succeeds, all later requests
    /// must fail without changing identity or authorization state. Retry time is
    /// not command content; the first successful operation owns recorded time.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the complete operation cannot be
    /// durably resolved. Partial bootstrap state must never commit.
    fn bootstrap<'a>(
        &'a self,
        request: &'a PlatformBootstrapRequest,
    ) -> impl Future<Output = Result<PlatformBootstrapOutcome, Self::Error>> + Send + 'a;
}

/// Establishes the first platform operator through a single bootstrap boundary.
#[derive(Clone, Debug)]
pub struct PlatformBootstrapService<R> {
    repository: R,
}

impl<R> PlatformBootstrapService<R> {
    /// Creates the service using the supplied bootstrap repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> PlatformBootstrapService<R>
where
    R: PlatformBootstrapRepository,
{
    /// Applies the one-time platform bootstrap request.
    ///
    /// This method performs no authentication or authorization because no
    /// internal platform authority exists before it succeeds. Callers must keep
    /// it behind a separately controlled installation boundary.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when bootstrap cannot be completed or when a
    /// different operator has already initialized the platform.
    pub async fn bootstrap(
        &self,
        request: &PlatformBootstrapRequest,
    ) -> Result<PlatformBootstrapOutcome, PlatformBootstrapError<R::Error>> {
        self.repository
            .bootstrap(request)
            .await
            .map_err(PlatformBootstrapError::Repository)
    }
}

/// Failure to establish or resolve the initial platform operator.
#[derive(Debug)]
pub enum PlatformBootstrapError<E> {
    /// The atomic persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for PlatformBootstrapError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => formatter.write_str("platform bootstrap could not be persisted"),
        }
    }
}

impl<E> Error for PlatformBootstrapError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        future::ready,
        sync::{Arc, Mutex},
    };

    use fleetiq_domain::{ExternalIdentitySubject, IdentityAuthorityId};

    use super::*;

    #[derive(Clone, Default)]
    struct RecordingRepository {
        requests: Arc<Mutex<Vec<PlatformBootstrapRequest>>>,
    }

    impl PlatformBootstrapRepository for RecordingRepository {
        type Error = Infallible;

        fn bootstrap<'a>(
            &'a self,
            request: &'a PlatformBootstrapRequest,
        ) -> impl Future<Output = Result<PlatformBootstrapOutcome, Self::Error>> + Send + 'a
        {
            self.requests
                .lock()
                .expect("recording lock should remain available")
                .push(request.clone());
            ready(Ok(PlatformBootstrapOutcome::Bootstrapped))
        }
    }

    #[tokio::test]
    async fn forwards_the_complete_trusted_bootstrap_request() {
        let repository = RecordingRepository::default();
        let recorded = repository.requests.clone();
        let service = PlatformBootstrapService::new(repository);
        let request = request();

        let outcome = service
            .bootstrap(&request)
            .await
            .expect("recording repository should bootstrap");

        assert_eq!(outcome, PlatformBootstrapOutcome::Bootstrapped);
        assert_eq!(
            recorded
                .lock()
                .expect("recording lock should remain available")
                .as_slice(),
            [request]
        );
    }

    fn request() -> PlatformBootstrapRequest {
        PlatformBootstrapRequest::new(
            ActorId::new("operator-a").expect("valid actor ID"),
            ExternalIdentity::new(
                IdentityAuthorityId::new("workforce-oidc").expect("valid authority ID"),
                ExternalIdentitySubject::new("operator-subject").expect("valid subject"),
            ),
            UnixTimeMillis::new(1_000),
        )
    }
}
