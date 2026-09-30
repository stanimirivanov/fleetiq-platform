use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, ActorProvisioningId, ExternalIdentity, UnixTimeMillis};

use crate::{AuthorizedPlatformAction, PlatformAction};

/// Trusted input for one idempotent actor provisioning command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorProvisioningRequest {
    provisioning_id: ActorProvisioningId,
    actor_id: ActorId,
    external_identity: ExternalIdentity,
    recorded_at: UnixTimeMillis,
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
            recorded_at,
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
    recorded_at: UnixTimeMillis,
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

    /// Returns the trusted host time recorded for the command.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
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
    /// Returns [`ActorProvisioningError::WrongAction`] for other platform
    /// evidence, or wraps the repository error when persistence fails.
    pub async fn provision(
        &self,
        authorization: AuthorizedPlatformAction,
        request: ActorProvisioningRequest,
    ) -> Result<ActorProvisioningOutcome, ActorProvisioningError<R::Error>> {
        if authorization.action() != PlatformAction::ManageIdentity {
            return Err(ActorProvisioningError::WrongAction {
                actual: authorization.action(),
            });
        }

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
    /// Authorization evidence permits another platform operation.
    WrongAction {
        /// Action carried by the rejected evidence.
        actual: PlatformAction,
    },
    /// The atomic persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for ActorProvisioningError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongAction { actual } => write!(
                formatter,
                "actor provisioning requires ManageIdentity authorization, got {actual:?}"
            ),
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
            Self::WrongAction { .. } => None,
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
            .provision(
                authorization(PlatformAction::ManageIdentity).await,
                request(),
            )
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

    #[tokio::test]
    async fn rejects_other_platform_evidence_before_persistence() {
        let repository = RecordingRepository::default();
        let recorded = repository.commands.clone();
        let error = ActorProvisioningService::new(repository)
            .provision(
                authorization(PlatformAction::ProvisionTenant).await,
                request(),
            )
            .await
            .expect_err("tenant provisioning evidence must not provision an actor");

        assert!(matches!(
            error,
            ActorProvisioningError::WrongAction {
                actual: PlatformAction::ProvisionTenant
            }
        ));
        assert!(
            recorded
                .lock()
                .expect("recording lock should remain available")
                .is_empty()
        );
    }

    async fn authorization(action: PlatformAction) -> AuthorizedPlatformAction {
        PlatformAuthorizationService::new(AllowPolicy)
            .authorize(
                crate::test_support::authenticated_actor(actor_id("operator-a")).await,
                action,
            )
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
