use std::{
    convert::Infallible,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use crate::{
    AccessDecision, PlatformAccessPolicy, PlatformAccessRequest, PlatformAuthorizationService,
};

use super::*;

#[derive(Clone, Default)]
struct RecordingRepository {
    changes: Arc<Mutex<Vec<TenantLifecycleChange>>>,
}

impl TenantLifecycleAdministrationRepository for RecordingRepository {
    type Error = Infallible;

    fn apply<'a>(
        &'a self,
        change: &'a TenantLifecycleChange,
    ) -> impl Future<Output = Result<TenantLifecycleChangeOutcome, Self::Error>> + Send + 'a {
        self.changes
            .lock()
            .expect("recording lock should remain available")
            .push(change.clone());
        ready(Ok(TenantLifecycleChangeOutcome::Recorded(
            TenantLifecycleChangeEffect::StateChanged,
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

    TenantLifecycleAdministration::new(repository)
        .apply(authorization::<ManageTenantLifecycle>().await, request())
        .await
        .expect("recording repository should apply the change");

    let changes = recorded
        .lock()
        .expect("recording lock should remain available");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].administrator_actor_id().as_str(), "operator-a");
    assert_eq!(changes[0].tenant_id().as_str(), "tenant-a");
}

async fn authorization<A: crate::PlatformActionMarker>() -> AuthorizedPlatform<A> {
    PlatformAuthorizationService::new(AllowPolicy)
        .authorize::<A>(crate::test_support::authenticated_actor(actor_id("operator-a")).await)
        .await
        .expect("test policy should authorize")
}

fn request() -> TenantLifecycleChangeRequest {
    TenantLifecycleChangeRequest::new(
        TenantLifecycleChangeId::new("tenant-lifecycle-a").expect("valid change ID"),
        TenantId::new("tenant-a").expect("valid tenant ID"),
        TenantLifecycleChangeKind::Suspend,
        UnixTimeMillis::new(1_000),
    )
}

fn actor_id(value: &str) -> ActorId {
    ActorId::new(value).expect("valid actor ID")
}
