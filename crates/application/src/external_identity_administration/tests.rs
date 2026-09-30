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
    changes: Arc<Mutex<Vec<ExternalIdentityChange>>>,
}

impl ExternalIdentityAdministrationRepository for RecordingRepository {
    type Error = Infallible;

    fn apply<'a>(
        &'a self,
        change: &'a ExternalIdentityChange,
    ) -> impl Future<Output = Result<ExternalIdentityChangeOutcome, Self::Error>> + Send + 'a {
        self.changes
            .lock()
            .expect("recording lock should remain available")
            .push(change.clone());
        ready(Ok(ExternalIdentityChangeOutcome::Recorded(
            ExternalIdentityChangeEffect::StateChanged,
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
async fn derives_the_administrator_from_manage_identity_evidence() {
    let repository = RecordingRepository::default();
    let recorded = repository.changes.clone();
    let outcome = ExternalIdentityAdministration::new(repository)
        .apply(authorization::<ManageIdentity>().await, request())
        .await
        .expect("recording repository should accept the change");

    assert_eq!(
        outcome,
        ExternalIdentityChangeOutcome::Recorded(ExternalIdentityChangeEffect::StateChanged)
    );
    let changes = recorded
        .lock()
        .expect("recording lock should remain available");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].administrator_actor_id().as_str(), "operator-a");
    assert_eq!(changes[0].target_actor_id().as_str(), "actor-a");
}

async fn authorization<A: crate::PlatformActionMarker>() -> AuthorizedPlatform<A> {
    PlatformAuthorizationService::new(AllowPolicy)
        .authorize::<A>(crate::test_support::authenticated_actor(actor_id("operator-a")).await)
        .await
        .expect("test policy should authorize")
}

fn request() -> ExternalIdentityChangeRequest {
    ExternalIdentityChangeRequest::new(
        ExternalIdentityChangeId::new("identity-change-a").expect("valid change ID"),
        actor_id("actor-a"),
        ExternalIdentity::new(
            IdentityAuthorityId::new("workforce-oidc").expect("valid authority ID"),
            ExternalIdentitySubject::new("subject-a").expect("valid subject"),
        ),
        ExternalIdentityChangeKind::Attach,
        UnixTimeMillis::new(1_000),
    )
}

fn actor_id(value: &str) -> ActorId {
    ActorId::new(value).expect("valid actor ID")
}
