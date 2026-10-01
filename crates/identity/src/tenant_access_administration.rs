use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, TenantAccessChangeId, TenantId, UnixTimeMillis};

use crate::{AuthorizedTenant, ManageAccess, TenantAction};

/// Requested mutation of one exact tenant action grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantAccessChangeKind {
    /// Ensure that the exact grant exists.
    Grant,
    /// Ensure that the exact grant does not exist.
    Revoke,
}

/// Trusted input for one idempotent access administration command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantAccessChangeRequest {
    change_id: TenantAccessChangeId,
    target_actor_id: ActorId,
    action: TenantAction,
    kind: TenantAccessChangeKind,
    recorded_at: UnixTimeMillis,
}

impl TenantAccessChangeRequest {
    /// Creates a request with host-assigned identity and recorded time.
    pub const fn new(
        change_id: TenantAccessChangeId,
        target_actor_id: ActorId,
        action: TenantAction,
        kind: TenantAccessChangeKind,
        recorded_at: UnixTimeMillis,
    ) -> Self {
        Self {
            change_id,
            target_actor_id,
            action,
            kind,
            recorded_at,
        }
    }
}

/// Complete auditable tenant access change assembled after authorization.
///
/// Construction is private to the application service so infrastructure cannot
/// accept a caller-selected administrator or tenant as trusted audit context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantAccessChange {
    change_id: TenantAccessChangeId,
    tenant_id: TenantId,
    administrator_actor_id: ActorId,
    target_actor_id: ActorId,
    action: TenantAction,
    kind: TenantAccessChangeKind,
    recorded_at: UnixTimeMillis,
}

impl TenantAccessChange {
    /// Returns the tenant-scoped idempotency and audit identity.
    pub const fn change_id(&self) -> &TenantAccessChangeId {
        &self.change_id
    }

    /// Returns the tenant whose policy is changed.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the authenticated actor that authorized the change.
    pub const fn administrator_actor_id(&self) -> &ActorId {
        &self.administrator_actor_id
    }

    /// Returns the actor whose grant is changed.
    pub const fn target_actor_id(&self) -> &ActorId {
        &self.target_actor_id
    }

    /// Returns the exact action being granted or revoked.
    pub const fn action(&self) -> TenantAction {
        self.action
    }

    /// Returns whether the command grants or revokes access.
    pub const fn kind(&self) -> TenantAccessChangeKind {
        self.kind
    }

    /// Returns the trusted host time recorded for the command.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }
}

/// Whether a recorded command changed current grant state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantAccessChangeEffect {
    /// The current grant state changed.
    StateChanged,
    /// The requested state already held, so only the audit command was recorded.
    NoStateChange,
}

/// Durable disposition of an idempotent access administration command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantAccessChangeOutcome {
    /// This call durably recorded the command and its effect.
    Recorded(TenantAccessChangeEffect),
    /// An exact earlier command already recorded the same effect.
    AlreadyRecorded(TenantAccessChangeEffect),
}

/// Persistence boundary for atomic grant mutation and append-only audit.
pub trait TenantAccessAdministrationRepository: Send + Sync {
    /// Storage, invariant, or command-identity collision failure.
    type Error: Error + Send + Sync + 'static;

    /// Applies current grant state and records its audit command atomically.
    ///
    /// Exact retries return [`TenantAccessChangeOutcome::AlreadyRecorded`].
    /// Reusing a change identity for different content is an error.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the complete command cannot be
    /// durably resolved. No grant mutation may commit without its audit record.
    fn apply<'a>(
        &'a self,
        change: &'a TenantAccessChange,
    ) -> impl Future<Output = Result<TenantAccessChangeOutcome, Self::Error>> + Send + 'a;
}

/// Applies authorized tenant grant changes through an auditable repository.
#[derive(Clone, Debug)]
pub struct TenantAccessAdministration<R> {
    repository: R,
}

impl<R> TenantAccessAdministration<R> {
    /// Creates the administration service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> TenantAccessAdministration<R>
where
    R: TenantAccessAdministrationRepository,
{
    /// Applies one access change using exact `ManageAccess` authorization.
    ///
    /// The authorization evidence is consumed so it cannot be reused for a
    /// second administration command. Tenant and administrator identity are
    /// derived from that evidence rather than caller-provided request fields.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the atomic mutation and audit operation fails.
    pub async fn apply(
        &self,
        authorization: AuthorizedTenant<ManageAccess>,
        request: TenantAccessChangeRequest,
    ) -> Result<TenantAccessChangeOutcome, TenantAccessAdministrationError<R::Error>> {
        let change = TenantAccessChange {
            change_id: request.change_id,
            tenant_id: authorization.tenant_id().clone(),
            administrator_actor_id: authorization.actor_id().clone(),
            target_actor_id: request.target_actor_id,
            action: request.action,
            kind: request.kind,
            recorded_at: request.recorded_at,
        };

        self.repository
            .apply(&change)
            .await
            .map_err(TenantAccessAdministrationError::Repository)
    }
}

/// Failure to apply an authorized tenant access administration command.
#[derive(Debug)]
pub enum TenantAccessAdministrationError<E> {
    /// The atomic persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for TenantAccessAdministrationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => {
                formatter.write_str("tenant access administration could not be persisted")
            }
        }
    }
}

impl<E> Error for TenantAccessAdministrationError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
        }
    }
}
