use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, PlatformAccessChangeId, UnixTimeMillis};

use crate::{AuthorizedPlatformAction, PlatformAction};

/// Requested mutation of one exact platform action grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformAccessChangeKind {
    /// Ensure that the exact grant exists.
    Grant,
    /// Ensure that the exact grant does not exist.
    Revoke,
}

/// Trusted input for one idempotent platform access command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformAccessChangeRequest {
    change_id: PlatformAccessChangeId,
    target_actor_id: ActorId,
    action: PlatformAction,
    kind: PlatformAccessChangeKind,
    recorded_at: UnixTimeMillis,
}

impl PlatformAccessChangeRequest {
    /// Creates a request with host-assigned identity and recorded time.
    pub const fn new(
        change_id: PlatformAccessChangeId,
        target_actor_id: ActorId,
        action: PlatformAction,
        kind: PlatformAccessChangeKind,
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

/// Complete auditable platform access change assembled after authorization.
///
/// Construction is private to the application service so persistence cannot
/// accept a caller-selected administrator as trusted audit context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformAccessChange {
    change_id: PlatformAccessChangeId,
    administrator_actor_id: ActorId,
    target_actor_id: ActorId,
    action: PlatformAction,
    kind: PlatformAccessChangeKind,
    recorded_at: UnixTimeMillis,
}

impl PlatformAccessChange {
    /// Returns the platform-scoped idempotency and audit identity.
    pub const fn change_id(&self) -> &PlatformAccessChangeId {
        &self.change_id
    }

    /// Returns the authenticated actor that authorized the change.
    pub const fn administrator_actor_id(&self) -> &ActorId {
        &self.administrator_actor_id
    }

    /// Returns the actor whose grant is changed.
    pub const fn target_actor_id(&self) -> &ActorId {
        &self.target_actor_id
    }

    /// Returns the exact platform action being granted or revoked.
    pub const fn action(&self) -> PlatformAction {
        self.action
    }

    /// Returns whether the command grants or revokes access.
    pub const fn kind(&self) -> PlatformAccessChangeKind {
        self.kind
    }

    /// Returns the trusted host time recorded for the command.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }
}

/// Whether a recorded command changed current platform grant state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformAccessChangeEffect {
    /// The current grant state changed.
    StateChanged,
    /// The requested state already held, so only the audit command was recorded.
    NoStateChange,
}

/// Durable disposition of an idempotent platform access command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformAccessChangeOutcome {
    /// This call durably recorded the command and its effect.
    Recorded(PlatformAccessChangeEffect),
    /// An exact earlier command already recorded the same effect.
    AlreadyRecorded(PlatformAccessChangeEffect),
}

/// Persistence boundary for atomic platform grant mutation and immutable audit.
pub trait PlatformAccessAdministrationRepository: Send + Sync {
    /// Storage, invariant, or command-identity collision failure.
    type Error: Error + Send + Sync + 'static;

    /// Applies current grant state and records its audit command atomically.
    ///
    /// Exact retries return [`PlatformAccessChangeOutcome::AlreadyRecorded`].
    /// Reusing a change identity for different content is an error.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the complete command cannot be
    /// durably resolved. No grant mutation may commit without its audit record.
    fn apply<'a>(
        &'a self,
        change: &'a PlatformAccessChange,
    ) -> impl Future<Output = Result<PlatformAccessChangeOutcome, Self::Error>> + Send + 'a;
}

/// Applies authorized platform grant changes through an auditable repository.
#[derive(Clone, Debug)]
pub struct PlatformAccessAdministration<R> {
    repository: R,
}

impl<R> PlatformAccessAdministration<R> {
    /// Creates the administration service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> PlatformAccessAdministration<R>
where
    R: PlatformAccessAdministrationRepository,
{
    /// Applies one access change using exact `ManagePlatformAccess` evidence.
    ///
    /// The evidence is consumed, and the authenticated administrator comes
    /// from it rather than caller-provided request data.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformAccessAdministrationError::WrongAction`] for other
    /// platform evidence, or wraps the repository error when persistence fails.
    pub async fn apply(
        &self,
        authorization: AuthorizedPlatformAction,
        request: PlatformAccessChangeRequest,
    ) -> Result<PlatformAccessChangeOutcome, PlatformAccessAdministrationError<R::Error>> {
        if authorization.action() != PlatformAction::ManagePlatformAccess {
            return Err(PlatformAccessAdministrationError::WrongAction {
                actual: authorization.action(),
            });
        }

        let change = PlatformAccessChange {
            change_id: request.change_id,
            administrator_actor_id: authorization.actor_id().clone(),
            target_actor_id: request.target_actor_id,
            action: request.action,
            kind: request.kind,
            recorded_at: request.recorded_at,
        };

        self.repository
            .apply(&change)
            .await
            .map_err(PlatformAccessAdministrationError::Repository)
    }
}

/// Failure to apply an authorized platform access command.
#[derive(Debug)]
pub enum PlatformAccessAdministrationError<E> {
    /// Authorization evidence permits another platform operation.
    WrongAction {
        /// Action carried by the rejected evidence.
        actual: PlatformAction,
    },
    /// The atomic persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for PlatformAccessAdministrationError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongAction { actual } => write!(
                formatter,
                "platform access administration requires ManagePlatformAccess authorization, got {actual:?}"
            ),
            Self::Repository(_) => {
                formatter.write_str("platform access administration could not be persisted")
            }
        }
    }
}

impl<E> Error for PlatformAccessAdministrationError<E>
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
        AccessDecision, PlatformAccessPolicy, PlatformAccessRequest, PlatformAuthorizationService,
    };

    use super::*;

    #[derive(Clone, Default)]
    struct RecordingRepository {
        changes: Arc<Mutex<Vec<PlatformAccessChange>>>,
    }

    impl PlatformAccessAdministrationRepository for RecordingRepository {
        type Error = Infallible;

        fn apply<'a>(
            &'a self,
            change: &'a PlatformAccessChange,
        ) -> impl Future<Output = Result<PlatformAccessChangeOutcome, Self::Error>> + Send + 'a
        {
            self.changes
                .lock()
                .expect("recording lock should remain available")
                .push(change.clone());
            ready(Ok(PlatformAccessChangeOutcome::Recorded(
                PlatformAccessChangeEffect::StateChanged,
            )))
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
    async fn derives_the_administrator_from_authorization_evidence() {
        let repository = RecordingRepository::default();
        let recorded = repository.changes.clone();
        let outcome = PlatformAccessAdministration::new(repository)
            .apply(
                authorization(PlatformAction::ManagePlatformAccess).await,
                request(),
            )
            .await
            .expect("recording repository should apply the change");

        assert_eq!(
            outcome,
            PlatformAccessChangeOutcome::Recorded(PlatformAccessChangeEffect::StateChanged)
        );
        let changes = recorded
            .lock()
            .expect("recording lock should remain available");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].administrator_actor_id().as_str(), "operator-a");
        assert_eq!(changes[0].target_actor_id().as_str(), "operator-b");
    }

    #[tokio::test]
    async fn rejects_other_platform_evidence_before_persistence() {
        let repository = RecordingRepository::default();
        let recorded = repository.changes.clone();
        let error = PlatformAccessAdministration::new(repository)
            .apply(
                authorization(PlatformAction::ManageIdentity).await,
                request(),
            )
            .await
            .expect_err("identity management must not administer platform grants");

        assert!(matches!(
            error,
            PlatformAccessAdministrationError::WrongAction {
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

    async fn authorization(action: PlatformAction) -> AuthorizedPlatformAction {
        PlatformAuthorizationService::new(AllowPolicy)
            .authorize(
                crate::test_support::authenticated_actor(actor_id("operator-a")).await,
                action,
            )
            .await
            .expect("test policy should authorize")
    }

    fn request() -> PlatformAccessChangeRequest {
        PlatformAccessChangeRequest::new(
            PlatformAccessChangeId::new("platform-change-a").expect("valid change ID"),
            actor_id("operator-b"),
            PlatformAction::ProvisionTenant,
            PlatformAccessChangeKind::Grant,
            UnixTimeMillis::new(1_000),
        )
    }

    fn actor_id(value: &str) -> ActorId {
        ActorId::new(value).expect("valid actor ID")
    }
}
