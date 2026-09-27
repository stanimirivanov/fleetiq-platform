use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, ExternalIdentity, ExternalIdentityChangeId, UnixTimeMillis};

use crate::{AuthorizedPlatformAction, PlatformAction};

/// Requested mutation of one exact external-identity mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalIdentityChangeKind {
    /// Ensure that the identity maps to the target actor.
    Attach,
    /// Ensure that the identity no longer maps to the target actor.
    Detach,
}

/// Trusted input for one idempotent external-identity command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalIdentityChangeRequest {
    change_id: ExternalIdentityChangeId,
    target_actor_id: ActorId,
    external_identity: ExternalIdentity,
    kind: ExternalIdentityChangeKind,
    recorded_at: UnixTimeMillis,
}

impl ExternalIdentityChangeRequest {
    /// Creates a request with a host-assigned identity and recorded time.
    pub const fn new(
        change_id: ExternalIdentityChangeId,
        target_actor_id: ActorId,
        external_identity: ExternalIdentity,
        kind: ExternalIdentityChangeKind,
        recorded_at: UnixTimeMillis,
    ) -> Self {
        Self {
            change_id,
            target_actor_id,
            external_identity,
            kind,
            recorded_at,
        }
    }
}

/// Complete auditable identity change assembled after authorization.
///
/// Construction is private to the application service so persistence cannot
/// accept a caller-selected administrator as trusted audit context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalIdentityChange {
    change_id: ExternalIdentityChangeId,
    administrator_actor_id: ActorId,
    target_actor_id: ActorId,
    external_identity: ExternalIdentity,
    kind: ExternalIdentityChangeKind,
    recorded_at: UnixTimeMillis,
}

impl ExternalIdentityChange {
    /// Returns the platform-scoped idempotency and audit identity.
    pub const fn change_id(&self) -> &ExternalIdentityChangeId {
        &self.change_id
    }

    /// Returns the authenticated actor that authorized the change.
    pub const fn administrator_actor_id(&self) -> &ActorId {
        &self.administrator_actor_id
    }

    /// Returns the actor whose login identity is changed.
    pub const fn target_actor_id(&self) -> &ActorId {
        &self.target_actor_id
    }

    /// Returns the exact external identity being attached or detached.
    pub const fn external_identity(&self) -> &ExternalIdentity {
        &self.external_identity
    }

    /// Returns whether the command attaches or detaches the identity.
    pub const fn kind(&self) -> ExternalIdentityChangeKind {
        self.kind
    }

    /// Returns the trusted host time recorded for the command.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }
}

/// Whether a recorded command changed the current identity mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalIdentityChangeEffect {
    /// The current mapping state changed.
    StateChanged,
    /// The requested mapping state already held.
    NoStateChange,
}

/// Durable disposition of an idempotent external-identity command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalIdentityChangeOutcome {
    /// This call durably recorded the command and its effect.
    Recorded(ExternalIdentityChangeEffect),
    /// An exact earlier command already recorded the same effect.
    AlreadyRecorded(ExternalIdentityChangeEffect),
}

/// Persistence boundary for atomic external-identity mutation and audit.
pub trait ExternalIdentityAdministrationRepository: Send + Sync {
    /// Storage, invariant, or command-identity collision failure.
    type Error: Error + Send + Sync + 'static;

    /// Applies the mapping state and records its audit command atomically.
    ///
    /// Exact retries return [`ExternalIdentityChangeOutcome::AlreadyRecorded`].
    /// Reusing a change identity for different content is an error. Detaching
    /// an actor's final external identity must fail without recording an audit
    /// row or changing the mapping.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the complete command cannot be
    /// durably resolved.
    fn apply<'a>(
        &'a self,
        change: &'a ExternalIdentityChange,
    ) -> impl Future<Output = Result<ExternalIdentityChangeOutcome, Self::Error>> + Send + 'a;
}

/// Applies authorized external-identity changes through an auditable repository.
#[derive(Clone, Debug)]
pub struct ExternalIdentityAdministration<R> {
    repository: R,
}

impl<R> ExternalIdentityAdministration<R> {
    /// Creates the administration service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> ExternalIdentityAdministration<R>
where
    R: ExternalIdentityAdministrationRepository,
{
    /// Applies one identity change using exact `ManageIdentity` evidence.
    ///
    /// The evidence is consumed, and the authenticated administrator comes
    /// from it rather than caller-provided request data.
    ///
    /// # Errors
    ///
    /// Returns [`ExternalIdentityAdministrationError::WrongAction`] for other
    /// platform evidence, or wraps the repository error when persistence fails.
    pub async fn apply(
        &self,
        authorization: AuthorizedPlatformAction,
        request: ExternalIdentityChangeRequest,
    ) -> Result<ExternalIdentityChangeOutcome, ExternalIdentityAdministrationError<R::Error>> {
        if authorization.action() != PlatformAction::ManageIdentity {
            return Err(ExternalIdentityAdministrationError::WrongAction {
                actual: authorization.action(),
            });
        }

        let change = ExternalIdentityChange {
            change_id: request.change_id,
            administrator_actor_id: authorization.actor_id().clone(),
            target_actor_id: request.target_actor_id,
            external_identity: request.external_identity,
            kind: request.kind,
            recorded_at: request.recorded_at,
        };

        self.repository
            .apply(&change)
            .await
            .map_err(ExternalIdentityAdministrationError::Repository)
    }
}

/// Failure to apply an authorized external-identity command.
#[derive(Debug)]
pub enum ExternalIdentityAdministrationError<E> {
    /// Authorization evidence permits another platform operation.
    WrongAction {
        /// Action carried by the rejected evidence.
        actual: PlatformAction,
    },
    /// The atomic persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for ExternalIdentityAdministrationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongAction { actual } => write!(
                formatter,
                "external identity administration requires ManageIdentity authorization, got {actual:?}"
            ),
            Self::Repository(_) => {
                formatter.write_str("external identity administration could not be persisted")
            }
        }
    }
}

impl<E> Error for ExternalIdentityAdministrationError<E>
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
mod tests;
