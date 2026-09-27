use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, ActorLifecycleChangeId, UnixTimeMillis};

use crate::{AuthorizedPlatformAction, PlatformAction};

/// Requested lifecycle state for one stable internal actor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorLifecycleChangeKind {
    /// Permit registered external identities to resolve to the actor.
    Activate,
    /// Prevent registered external identities from resolving to the actor.
    Deactivate,
}

/// Trusted input for one idempotent actor lifecycle command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorLifecycleChangeRequest {
    change_id: ActorLifecycleChangeId,
    target_actor_id: ActorId,
    kind: ActorLifecycleChangeKind,
    recorded_at: UnixTimeMillis,
}

impl ActorLifecycleChangeRequest {
    /// Creates a request with a host-assigned identity and recorded time.
    pub const fn new(
        change_id: ActorLifecycleChangeId,
        target_actor_id: ActorId,
        kind: ActorLifecycleChangeKind,
        recorded_at: UnixTimeMillis,
    ) -> Self {
        Self {
            change_id,
            target_actor_id,
            kind,
            recorded_at,
        }
    }
}

/// Complete auditable actor lifecycle change assembled after authorization.
///
/// Construction is private to the service so the repository cannot receive a
/// caller-selected administrator as trusted audit context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorLifecycleChange {
    change_id: ActorLifecycleChangeId,
    administrator_actor_id: ActorId,
    target_actor_id: ActorId,
    kind: ActorLifecycleChangeKind,
    recorded_at: UnixTimeMillis,
}

impl ActorLifecycleChange {
    /// Returns the platform-scoped idempotency and audit identity.
    pub const fn change_id(&self) -> &ActorLifecycleChangeId {
        &self.change_id
    }

    /// Returns the authenticated actor that authorized the change.
    pub const fn administrator_actor_id(&self) -> &ActorId {
        &self.administrator_actor_id
    }

    /// Returns the stable actor whose lifecycle state is requested.
    pub const fn target_actor_id(&self) -> &ActorId {
        &self.target_actor_id
    }

    /// Returns the requested lifecycle transition.
    pub const fn kind(&self) -> ActorLifecycleChangeKind {
        self.kind
    }

    /// Returns the trusted host time recorded for the command.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }
}

/// Whether a recorded command changed current actor state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorLifecycleChangeEffect {
    /// The actor's current state changed.
    StateChanged,
    /// The requested state already held, so only audit evidence was appended.
    NoStateChange,
}

/// Durable disposition of an idempotent actor lifecycle command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorLifecycleChangeOutcome {
    /// This call durably recorded the command and its effect.
    Recorded(ActorLifecycleChangeEffect),
    /// An exact earlier command already recorded the same effect.
    AlreadyRecorded(ActorLifecycleChangeEffect),
}

/// Persistence boundary for atomic actor state mutation and immutable audit.
pub trait ActorLifecycleAdministrationRepository: Send + Sync {
    /// Storage, invariant, or command-identity collision failure.
    type Error: Error + Send + Sync + 'static;

    /// Applies current actor state and records its audit command atomically.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the complete command cannot be
    /// resolved durably. State must never change without its audit evidence.
    fn apply<'a>(
        &'a self,
        change: &'a ActorLifecycleChange,
    ) -> impl Future<Output = Result<ActorLifecycleChangeOutcome, Self::Error>> + Send + 'a;
}

/// Applies authorized actor lifecycle changes through an auditable repository.
#[derive(Clone, Debug)]
pub struct ActorLifecycleAdministration<R> {
    repository: R,
}

impl<R> ActorLifecycleAdministration<R> {
    /// Creates the administration service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> ActorLifecycleAdministration<R>
where
    R: ActorLifecycleAdministrationRepository,
{
    /// Applies one lifecycle change using exact `ManageIdentity` evidence.
    ///
    /// The evidence is consumed and supplies the administrator identity.
    ///
    /// # Errors
    ///
    /// Returns [`ActorLifecycleAdministrationError::WrongAction`] for other
    /// platform evidence, or preserves the repository failure.
    pub async fn apply(
        &self,
        authorization: AuthorizedPlatformAction,
        request: ActorLifecycleChangeRequest,
    ) -> Result<ActorLifecycleChangeOutcome, ActorLifecycleAdministrationError<R::Error>> {
        if authorization.action() != PlatformAction::ManageIdentity {
            return Err(ActorLifecycleAdministrationError::WrongAction {
                actual: authorization.action(),
            });
        }

        self.repository
            .apply(&ActorLifecycleChange {
                change_id: request.change_id,
                administrator_actor_id: authorization.actor_id().clone(),
                target_actor_id: request.target_actor_id,
                kind: request.kind,
                recorded_at: request.recorded_at,
            })
            .await
            .map_err(ActorLifecycleAdministrationError::Repository)
    }
}

/// Failure to apply an authorized actor lifecycle command.
#[derive(Debug)]
pub enum ActorLifecycleAdministrationError<E> {
    /// Authorization evidence permits another platform operation.
    WrongAction {
        /// Action carried by the rejected evidence.
        actual: PlatformAction,
    },
    /// The atomic persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for ActorLifecycleAdministrationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongAction { actual } => write!(
                formatter,
                "actor lifecycle administration requires ManageIdentity authorization, got {actual:?}"
            ),
            Self::Repository(_) => {
                formatter.write_str("actor lifecycle administration could not be persisted")
            }
        }
    }
}

impl<E> Error for ActorLifecycleAdministrationError<E>
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
