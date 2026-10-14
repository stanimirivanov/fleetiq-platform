use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, ActorProvisioningId, ExternalIdentity, UnixTimeMillis};

use crate::recorded_time::RecordedTime;
use crate::{AuthorizedPlatform, ManageIdentity};

/// Trusted input for one idempotent actor provisioning command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorProvisioningRequest {
    provisioning_id: ActorProvisioningId,
    actor_id: ActorId,
    external_identity: ExternalIdentity,
    recorded_at: RecordedTime,
}

impl ActorProvisioningRequest {
    /// Creates a request with host-assigned identity and recorded time.
    pub const fn new(
        provisioning_id: ActorProvisioningId,
        actor_id: ActorId,
        external_identity: ExternalIdentity,
        recorded_at: UnixTimeMillis,
    ) -> Self {
        Self {
            provisioning_id,
            actor_id,
            external_identity,
            recorded_at: RecordedTime::Exact(recorded_at),
        }
    }

    /// Creates a redeliverable command using the first durable host receipt time.
    ///
    /// An adapter supplies the current trusted host time on each delivery. Exact
    /// command identity, administrator, and business content must still match;
    /// the winning transaction retains its original time on all retries.
    /// Use [`Self::new`] when recorded time is part of an immutable upstream command.
    pub const fn for_delivery(
        provisioning_id: ActorProvisioningId,
        actor_id: ActorId,
        external_identity: ExternalIdentity,
        recorded_at: UnixTimeMillis,
    ) -> Self {
        Self {
            provisioning_id,
            actor_id,
            external_identity,
            recorded_at: RecordedTime::FirstReceipt(recorded_at),
        }
    }
}

/// Complete auditable actor provisioning command assembled after authorization.
///
/// Construction is private to the application service so persistence cannot
/// accept a caller-selected provisioner as trusted audit context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorProvisioningCommand {
    provisioning_id: ActorProvisioningId,
    actor_id: ActorId,
    external_identity: ExternalIdentity,
    provisioner_actor_id: ActorId,
    recorded_at: RecordedTime,
}

impl ActorProvisioningCommand {
    /// Returns the platform-scoped command identity.
    pub const fn provisioning_id(&self) -> &ActorProvisioningId {
        &self.provisioning_id
    }

    /// Returns the stable internal actor to create.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }

    /// Returns the exact initial external identity to bind.
    pub const fn external_identity(&self) -> &ExternalIdentity {
        &self.external_identity
    }

    /// Returns the authenticated actor that authorized provisioning.
    pub const fn provisioner_actor_id(&self) -> &ActorId {
        &self.provisioner_actor_id
    }

    /// Returns the trusted host time proposed for the first durable write.
    ///
    /// Delivery retries retain the previously stored time instead of this proposal.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at.value()
    }

    /// Tests stored receipt time under this command's explicit retry policy.
    ///
    /// Repositories must also compare every command identity and business field.
    pub fn accepts_recorded_at(&self, stored: UnixTimeMillis) -> bool {
        self.recorded_at.accepts(stored)
    }
}

/// Durable disposition of an idempotent actor provisioning command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorProvisioningOutcome {
    /// This call created the actor, identity mapping, and audit fact.
    Provisioned,
    /// An exact earlier command already provisioned the actor.
    AlreadyProvisioned,
}

/// Persistence boundary for atomic actor provisioning and immutable audit.
pub trait ActorProvisioningRepository: Send + Sync {
    /// Storage, invariant, or command-identity collision failure.
    type Error: Error + Send + Sync + 'static;

    /// Creates the actor, initial identity mapping, and audit atomically.
    ///
    /// Exact retries return [`ActorProvisioningOutcome::AlreadyProvisioned`].
    /// Delivery commands retain the first durable host receipt time; immutable
    /// commands additionally require exact time equality. Compare all business
    /// fields and the authenticated provisioner under either policy.
    /// Reusing the provisioning ID, actor ID, or external identity for different
    /// command content is an error.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the complete command cannot be
    /// durably resolved. Actor existence alone grants no platform or tenant
    /// access.
    fn provision<'a>(
        &'a self,
        command: &'a ActorProvisioningCommand,
    ) -> impl Future<Output = Result<ActorProvisioningOutcome, Self::Error>> + Send + 'a;
}

/// Provisions actors through exact platform authorization evidence.
#[derive(Clone, Debug)]
pub struct ActorProvisioningService<R> {
    repository: R,
}

impl<R> ActorProvisioningService<R> {
    /// Creates the service using the supplied provisioning repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> ActorProvisioningService<R>
where
    R: ActorProvisioningRepository,
{
    /// Applies one actor provisioning request with `ManageIdentity` evidence.
    ///
    /// The authorization evidence is consumed, and the authenticated
    /// provisioner comes from that evidence rather than request data.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the operation fails.
    pub async fn provision(
        &self,
        authorization: AuthorizedPlatform<ManageIdentity>,
        request: ActorProvisioningRequest,
    ) -> Result<ActorProvisioningOutcome, ActorProvisioningError<R::Error>> {
        let command = ActorProvisioningCommand {
            provisioning_id: request.provisioning_id,
            actor_id: request.actor_id,
            external_identity: request.external_identity,
            provisioner_actor_id: authorization.actor_id().clone(),
            recorded_at: request.recorded_at,
        };
        self.repository
            .provision(&command)
            .await
            .map_err(ActorProvisioningError::Repository)
    }
}

/// Failure to apply an authorized actor provisioning command.
#[derive(Debug)]
pub enum ActorProvisioningError<E> {
    /// The atomic persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for ActorProvisioningError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => formatter.write_str("actor provisioning could not be persisted"),
        }
    }
}

impl<E> Error for ActorProvisioningError<E>
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

    use crate::{
        AccessDecision, PlatformAccessPolicy, PlatformAccessRequest, PlatformAuthorizationService,
    };

    use super::*;

    #[derive(Clone, Default)]
    struct RecordingRepository {
        commands: Arc<Mutex<Vec<ActorProvisioningCommand>>>,
    }

    impl ActorProvisioningRepository for RecordingRepository {
        type Error = Infallible;

        fn provision<'a>(
            &'a self,
            command: &'a ActorProvisioningCommand,
        ) -> impl Future<Output = Result<ActorProvisioningOutcome, Self::Error>> + Send + 'a
        {
            self.commands
                .lock()
                .expect("recording lock should remain available")
                .push(command.clone());
            ready(Ok(ActorProvisioningOutcome::Provisioned))
        }
    }

    struct AllowPolicy;

    impl PlatformAccessPolicy for AllowPolicy {
        type Error = Infallible;

        fn evaluate<'a>(
            &'a self,
            _request: &'a PlatformAccessRequest,
        ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
            ready(Ok(AccessDecision::Allow))
        }
    }

    #[tokio::test]
    async fn derives_the_provisioner_from_manage_identity_evidence() {
        let repository = RecordingRepository::default();
        let recorded = repository.commands.clone();
        let service = ActorProvisioningService::new(repository);

        let outcome = service
            .provision(authorization::<ManageIdentity>().await, request())
            .await
            .expect("recording repository should provision");

        assert_eq!(outcome, ActorProvisioningOutcome::Provisioned);
        let commands = recorded
            .lock()
            .expect("recording lock should remain available");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].provisioner_actor_id().as_str(), "operator-a");
        assert_eq!(commands[0].actor_id().as_str(), "actor-a");
    }

    async fn authorization<A: crate::PlatformActionMarker>() -> AuthorizedPlatform<A> {
        PlatformAuthorizationService::new(AllowPolicy)
            .authorize::<A>(crate::test_support::authenticated_actor(actor_id("operator-a")).await)
            .await
            .expect("test policy should authorize")
    }

    fn request() -> ActorProvisioningRequest {
        ActorProvisioningRequest::new(
            ActorProvisioningId::new("actor-provisioning-a").expect("valid actor provisioning ID"),
            actor_id("actor-a"),
            ExternalIdentity::new(
                IdentityAuthorityId::new("workforce-oidc").expect("valid authority ID"),
                ExternalIdentitySubject::new("subject-a").expect("valid subject"),
            ),
            UnixTimeMillis::new(1_000),
        )
    }

    fn actor_id(value: &str) -> ActorId {
        ActorId::new(value).expect("valid actor ID")
    }
}
