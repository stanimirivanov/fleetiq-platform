use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, TenantId, TenantProvisioningId, UnixTimeMillis};

use crate::{AuthorizedPlatformAction, PlatformAction};

/// Trusted input for one idempotent tenant provisioning command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantProvisioningRequest {
    provisioning_id: TenantProvisioningId,
    tenant_id: TenantId,
    initial_administrator_actor_id: ActorId,
    recorded_at: UnixTimeMillis,
}

impl TenantProvisioningRequest {
    /// Creates a request with host-assigned identity and recorded time.
    pub const fn new(
        provisioning_id: TenantProvisioningId,
        tenant_id: TenantId,
        initial_administrator_actor_id: ActorId,
        recorded_at: UnixTimeMillis,
    ) -> Self {
        Self {
            provisioning_id,
            tenant_id,
            initial_administrator_actor_id,
            recorded_at,
        }
    }
}

/// Complete auditable tenant provisioning command assembled after authorization.
///
/// Construction is private to the application service so persistence cannot
/// accept a caller-selected provisioner as trusted audit context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantProvisioningCommand {
    provisioning_id: TenantProvisioningId,
    tenant_id: TenantId,
    provisioner_actor_id: ActorId,
    initial_administrator_actor_id: ActorId,
    recorded_at: UnixTimeMillis,
}

impl TenantProvisioningCommand {
    /// Returns the platform-scoped idempotency and audit identity.
    pub const fn provisioning_id(&self) -> &TenantProvisioningId {
        &self.provisioning_id
    }

    /// Returns the tenant to create.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the authenticated actor that authorized provisioning.
    pub const fn provisioner_actor_id(&self) -> &ActorId {
        &self.provisioner_actor_id
    }

    /// Returns the actor that receives the initial tenant grants.
    pub const fn initial_administrator_actor_id(&self) -> &ActorId {
        &self.initial_administrator_actor_id
    }

    /// Returns the trusted host time recorded for the command.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }
}

/// Durable disposition of an idempotent tenant provisioning command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantProvisioningOutcome {
    /// This call atomically created the tenant, initial grants, and audit fact.
    Provisioned,
    /// An exact earlier command already provisioned the tenant.
    AlreadyProvisioned,
}

/// Persistence boundary for atomic tenant provisioning and immutable audit.
pub trait TenantProvisioningRepository: Send + Sync {
    /// Storage, invariant, or command-identity collision failure.
    type Error: Error + Send + Sync + 'static;

    /// Creates the tenant, initial administrator grants, and audit atomically.
    ///
    /// Exact retries return [`TenantProvisioningOutcome::AlreadyProvisioned`].
    /// Reusing either the provisioning identity or tenant identity for different
    /// command content is an error.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the complete command cannot be
    /// durably resolved. No tenant or grant may commit without the immutable
    /// provisioning record.
    fn provision<'a>(
        &'a self,
        command: &'a TenantProvisioningCommand,
    ) -> impl Future<Output = Result<TenantProvisioningOutcome, Self::Error>> + Send + 'a;
}

/// Provisions tenants through exact platform authorization evidence.
#[derive(Clone, Debug)]
pub struct TenantProvisioningService<R> {
    repository: R,
}

impl<R> TenantProvisioningService<R> {
    /// Creates the service using the supplied provisioning repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> TenantProvisioningService<R>
where
    R: TenantProvisioningRepository,
{
    /// Applies one tenant provisioning command with `ProvisionTenant` evidence.
    ///
    /// Authorization evidence is consumed so it cannot be reused for another
    /// command. The authenticated provisioner identity comes from that evidence
    /// rather than from caller-controlled request fields.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the atomic provisioning operation fails.
    pub async fn provision(
        &self,
        authorization: AuthorizedPlatformAction,
        request: TenantProvisioningRequest,
    ) -> Result<TenantProvisioningOutcome, TenantProvisioningError<R::Error>> {
        if authorization.action() != PlatformAction::ProvisionTenant {
            return Err(TenantProvisioningError::WrongAction {
                actual: authorization.action(),
            });
        }

        let command = TenantProvisioningCommand {
            provisioning_id: request.provisioning_id,
            tenant_id: request.tenant_id,
            provisioner_actor_id: authorization.actor_id().clone(),
            initial_administrator_actor_id: request.initial_administrator_actor_id,
            recorded_at: request.recorded_at,
        };

        self.repository
            .provision(&command)
            .await
            .map_err(TenantProvisioningError::Repository)
    }
}

/// Failure to apply an authorized tenant provisioning command.
#[derive(Debug)]
pub enum TenantProvisioningError<E> {
    /// Authorization evidence permits another platform operation.
    WrongAction {
        /// Action carried by the rejected evidence.
        actual: PlatformAction,
    },
    /// The atomic persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for TenantProvisioningError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongAction { actual } => write!(
                formatter,
                "tenant provisioning requires ProvisionTenant authorization, got {actual:?}"
            ),
            Self::Repository(_) => {
                formatter.write_str("tenant provisioning could not be persisted")
            }
        }
    }
}

impl<E> Error for TenantProvisioningError<E>
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

    use crate::{
        AccessDecision, AuthenticatedActor, PlatformAccessPolicy, PlatformAccessRequest,
        PlatformAuthorizationService,
    };

    use super::*;

    #[derive(Clone, Default)]
    struct RecordingRepository {
        commands: Arc<Mutex<Vec<TenantProvisioningCommand>>>,
    }

    impl TenantProvisioningRepository for RecordingRepository {
        type Error = Infallible;

        fn provision<'a>(
            &'a self,
            command: &'a TenantProvisioningCommand,
        ) -> impl Future<Output = Result<TenantProvisioningOutcome, Self::Error>> + Send + 'a
        {
            self.commands
                .lock()
                .expect("recording lock should remain available")
                .push(command.clone());
            ready(Ok(TenantProvisioningOutcome::Provisioned))
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
    async fn derives_trusted_provisioner_from_consumed_authorization() {
        let repository = RecordingRepository::default();
        let recorded = repository.commands.clone();
        let service = TenantProvisioningService::new(repository);

        let outcome = service
            .provision(
                authorization("operator-a", PlatformAction::ProvisionTenant).await,
                request(),
            )
            .await
            .expect("recording repository should provision");

        assert_eq!(outcome, TenantProvisioningOutcome::Provisioned);
        let commands = recorded
            .lock()
            .expect("recording lock should remain available");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].provisioner_actor_id().as_str(), "operator-a");
        assert_eq!(commands[0].tenant_id().as_str(), "tenant-a");
        assert_eq!(
            commands[0].initial_administrator_actor_id().as_str(),
            "admin-a"
        );
    }

    #[tokio::test]
    async fn rejects_identity_management_evidence_before_persistence() {
        let repository = RecordingRepository::default();
        let recorded = repository.commands.clone();
        let error = TenantProvisioningService::new(repository)
            .provision(
                authorization("operator-a", PlatformAction::ManageIdentity).await,
                request(),
            )
            .await
            .expect_err("identity management evidence must not provision a tenant");

        assert!(matches!(
            error,
            TenantProvisioningError::WrongAction {
                actual: PlatformAction::ManageIdentity
            }
        ));
        assert!(
            recorded
                .lock()
                .expect("recording lock should remain available")
                .is_empty()
        );
    }

    async fn authorization(actor: &str, action: PlatformAction) -> AuthorizedPlatformAction {
        PlatformAuthorizationService::new(AllowPolicy)
            .authorize(
                AuthenticatedActor::from_trusted_actor_id(actor_id(actor)),
                action,
            )
            .await
            .expect("test policy should authorize provisioning")
    }

    fn request() -> TenantProvisioningRequest {
        TenantProvisioningRequest::new(
            TenantProvisioningId::new("provisioning-a").expect("valid provisioning ID"),
            TenantId::new("tenant-a").expect("valid tenant ID"),
            actor_id("admin-a"),
            UnixTimeMillis::new(1_000),
        )
    }

    fn actor_id(value: &str) -> ActorId {
        ActorId::new(value).expect("valid actor ID")
    }
}
