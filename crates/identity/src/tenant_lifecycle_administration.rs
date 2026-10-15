use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, TenantId, TenantLifecycleChangeId, UnixTimeMillis};

use crate::recorded_time::RecordedTime;
use crate::{AuthorizedPlatform, ManageTenantLifecycle};

/// Requested operational state for one stable tenant boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantLifecycleChangeKind {
    /// Permit fresh tenant authorization decisions.
    Activate,
    /// Deny fresh tenant authorization decisions while retaining tenant data.
    Suspend,
}

/// Trusted input for one idempotent tenant lifecycle command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantLifecycleChangeRequest {
    change_id: TenantLifecycleChangeId,
    tenant_id: TenantId,
    kind: TenantLifecycleChangeKind,
    recorded_at: RecordedTime,
}

impl TenantLifecycleChangeRequest {
    /// Creates a request with a host-assigned identity and recorded time.
    pub const fn new(
        change_id: TenantLifecycleChangeId,
        tenant_id: TenantId,
        kind: TenantLifecycleChangeKind,
        recorded_at: UnixTimeMillis,
    ) -> Self {
        Self {
            change_id,
            tenant_id,
            kind,
            recorded_at: RecordedTime::Exact(recorded_at),
        }
    }

    /// Creates a redeliverable request retaining the first durable host receipt time.
    ///
    /// All identities and business fields, including the authenticated administrator,
    /// must still match. Use [`Self::new`] when time is immutable command content.
    pub const fn for_delivery(
        change_id: TenantLifecycleChangeId,
        tenant_id: TenantId,
        kind: TenantLifecycleChangeKind,
        recorded_at: UnixTimeMillis,
    ) -> Self {
        Self {
            change_id,
            tenant_id,
            kind,
            recorded_at: RecordedTime::FirstReceipt(recorded_at),
        }
    }
}

/// Complete auditable tenant lifecycle change assembled after authorization.
///
/// Construction is private to the service so persistence cannot accept a
/// caller-selected administrator as trusted audit context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantLifecycleChange {
    change_id: TenantLifecycleChangeId,
    administrator_actor_id: ActorId,
    tenant_id: TenantId,
    kind: TenantLifecycleChangeKind,
    recorded_at: RecordedTime,
}

impl TenantLifecycleChange {
    /// Returns the platform-scoped idempotency and audit identity.
    pub const fn change_id(&self) -> &TenantLifecycleChangeId {
        &self.change_id
    }

    /// Returns the authenticated actor that authorized the change.
    pub const fn administrator_actor_id(&self) -> &ActorId {
        &self.administrator_actor_id
    }

    /// Returns the stable tenant whose operational state is requested.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the requested lifecycle transition.
    pub const fn kind(&self) -> TenantLifecycleChangeKind {
        self.kind
    }

    /// Returns the proposed host time; redeliveries retain the stored first receipt.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at.value()
    }

    /// Checks stored time under the explicit retry policy.
    ///
    /// Persistence must also compare every identity and business field.
    pub fn accepts_recorded_at(&self, stored: UnixTimeMillis) -> bool {
        self.recorded_at.accepts(stored)
    }
}

/// Whether a recorded command changed current tenant state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantLifecycleChangeEffect {
    /// The tenant's current operational state changed.
    StateChanged,
    /// The requested state already held, so only audit evidence was appended.
    NoStateChange,
}

/// Durable disposition of an idempotent tenant lifecycle command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantLifecycleChangeOutcome {
    /// This call durably recorded the command and its effect.
    Recorded(TenantLifecycleChangeEffect),
    /// An exact earlier command already recorded the same effect.
    AlreadyRecorded(TenantLifecycleChangeEffect),
}

/// Persistence boundary for atomic tenant state mutation and immutable audit.
pub trait TenantLifecycleAdministrationRepository: Send + Sync {
    /// Storage, invariant, or command-identity collision failure.
    type Error: Error + Send + Sync + 'static;

    /// Applies current tenant state and records its audit command atomically.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the command cannot be resolved
    /// durably. State must never change without matching audit evidence.
    fn apply<'a>(
        &'a self,
        change: &'a TenantLifecycleChange,
    ) -> impl Future<Output = Result<TenantLifecycleChangeOutcome, Self::Error>> + Send + 'a;
}

/// Applies authorized tenant lifecycle changes through an auditable repository.
#[derive(Clone, Debug)]
pub struct TenantLifecycleAdministration<R> {
    repository: R,
}

impl<R> TenantLifecycleAdministration<R> {
    /// Creates the administration service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> TenantLifecycleAdministration<R>
where
    R: TenantLifecycleAdministrationRepository,
{
    /// Applies one change using exact `ManageTenantLifecycle` evidence.
    ///
    /// The evidence is consumed and supplies the administrator identity.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the operation fails.
    pub async fn apply(
        &self,
        authorization: AuthorizedPlatform<ManageTenantLifecycle>,
        request: TenantLifecycleChangeRequest,
    ) -> Result<TenantLifecycleChangeOutcome, TenantLifecycleAdministrationError<R::Error>> {
        self.repository
            .apply(&TenantLifecycleChange {
                change_id: request.change_id,
                administrator_actor_id: authorization.actor_id().clone(),
                tenant_id: request.tenant_id,
                kind: request.kind,
                recorded_at: request.recorded_at,
            })
            .await
            .map_err(TenantLifecycleAdministrationError::Repository)
    }
}

/// Failure to apply an authorized tenant lifecycle command.
#[derive(Debug)]
pub enum TenantLifecycleAdministrationError<E> {
    /// The atomic persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for TenantLifecycleAdministrationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => {
                formatter.write_str("tenant lifecycle administration could not be persisted")
            }
        }
    }
}

impl<E> Error for TenantLifecycleAdministrationError<E>
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
mod tests;
