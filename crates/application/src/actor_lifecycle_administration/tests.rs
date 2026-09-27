use std::{
    convert::Infallible,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use crate::{
    AccessDecision, AuthenticatedActor, PlatformAccessPolicy, PlatformAccessRequest,
    PlatformAuthorizationService,
};

use super::*;

#[derive(Clone, Default)]
struct RecordingRepository {
    changes: Arc<Mutex<Vec<ActorLifecycleChange>>>,
}

impl ActorLifecycleAdministrationRepository for RecordingRepository {
    type Error = Infallible;

    fn apply<'a>(
        &'a self,
        change: &'a ActorLifecycleChange,
    ) -> impl Future<Output = Result<ActorLifecycleChangeOutcome, Self::Error>> + Send + 'a {
        self.changes
            .lock()
            .expect("recording lock should remain available")
            .push(change.clone());
        ready(Ok(ActorLifecycleChangeOutcome::Recorded(
            ActorLifecycleChangeEffect::StateChanged,
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
async fn derives_administrator_from_exact_authorization() {
    let repository = RecordingRepository::default();
    let recorded = repository.changes.clone();

    ActorLifecycleAdministration::new(repository)
        .apply(
            authorization(PlatformAction::ManageIdentity).await,
            request(),
        )
        .await
        .expect("recording repository should apply the change");

    let changes = recorded
        .lock()
        .expect("recording lock should remain available");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].administrator_actor_id().as_str(), "operator-a");
    assert_eq!(changes[0].target_actor_id().as_str(), "actor-a");
}

#[tokio::test]
async fn rejects_other_platform_evidence_before_persistence() {
    let repository = RecordingRepository::default();
    let recorded = repository.changes.clone();

    let error = ActorLifecycleAdministration::new(repository)
        .apply(
            authorization(PlatformAction::ManagePlatformAccess).await,
            request(),
        )
        .await
        .expect_err("platform grant authority must not change actor state");

    assert!(matches!(
        error,
        ActorLifecycleAdministrationError::WrongAction {
            actual: PlatformAction::ManagePlatformAccess
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
            AuthenticatedActor::from_trusted_actor_id(actor_id("operator-a")),
            action,
        )
        .await
        .expect("test policy should authorize")
}

fn request() -> ActorLifecycleChangeRequest {
    ActorLifecycleChangeRequest::new(
        ActorLifecycleChangeId::new("actor-lifecycle-a").expect("valid change ID"),
        actor_id("actor-a"),
        ActorLifecycleChangeKind::Deactivate,
        UnixTimeMillis::new(1_000),
    )
}

fn actor_id(value: &str) -> ActorId {
    ActorId::new(value).expect("valid actor ID")
}
