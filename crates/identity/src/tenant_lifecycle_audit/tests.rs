use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::ActorId;

use crate::{
    AccessDecision, PlatformAccessPolicy, PlatformAccessRequest, PlatformAuthorizationService,
};

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestRepositoryError;

impl fmt::Display for TestRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("test repository failed")
    }
}

impl Error for TestRepositoryError {}

#[derive(Clone, Debug, Default)]
struct RecordingRepository {
    calls: Arc<Mutex<usize>>,
    fail: bool,
}

impl RecordingRepository {
    fn failing() -> Self {
        Self {
            fail: true,
            ..Self::default()
        }
    }
}

impl TenantLifecycleAuditRepository for RecordingRepository {
    type Error = TestRepositoryError;

    fn find_page<'a>(
        &'a self,
        _query: &'a TenantLifecycleAuditQuery,
    ) -> impl Future<Output = Result<TenantLifecycleAuditPage, Self::Error>> + Send + 'a {
        *self.calls.lock().expect("call lock poisoned") += 1;
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(TenantLifecycleAuditPage::new(Vec::new(), None))
        })
    }
}

struct AllowAllPolicy;

impl PlatformAccessPolicy for AllowAllPolicy {
    type Error = Infallible;

    fn evaluate<'a>(
        &'a self,
        _request: &'a PlatformAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
        ready(Ok(AccessDecision::Allow))
    }
}

#[test]
fn page_size_is_bounded() {
    assert_eq!(
        TenantLifecycleAuditPageSize::new(0),
        Err(TenantLifecycleAuditPageSizeError::Zero)
    );
    assert_eq!(
        TenantLifecycleAuditPageSize::new(1).expect("minimum").get(),
        1
    );
    assert_eq!(
        TenantLifecycleAuditPageSize::new(100)
            .expect("maximum")
            .get(),
        100
    );
    assert_eq!(
        TenantLifecycleAuditPageSize::new(101),
        Err(TenantLifecycleAuditPageSizeError::TooLarge {
            maximum: 100,
            actual: 101,
        })
    );
}

#[tokio::test]
async fn manage_tenant_lifecycle_authorization_allows_one_query() {
    let repository = RecordingRepository::default();
    let page = TenantLifecycleAudit::new(repository.clone())
        .find_page(authorization::<ManageTenantLifecycle>().await, &query())
        .await
        .expect("authorized query should succeed");

    assert_eq!(page.entries().len(), 0);
    assert_eq!(*repository.calls.lock().expect("call lock poisoned"), 1);
}

#[tokio::test]
async fn preserves_repository_failure_as_the_error_source() {
    let error = TenantLifecycleAudit::new(RecordingRepository::failing())
        .find_page(authorization::<ManageTenantLifecycle>().await, &query())
        .await
        .expect_err("repository failure should be returned");

    assert!(matches!(
        error,
        TenantLifecycleAuditError::Repository(TestRepositoryError)
    ));
    assert!(error.source().is_some());
}

fn query() -> TenantLifecycleAuditQuery {
    TenantLifecycleAuditQuery::new(
        TenantLifecycleAuditPageSize::new(25).expect("valid page size"),
        None,
    )
}

async fn authorization<A: crate::PlatformActionMarker>() -> AuthorizedPlatform<A> {
    PlatformAuthorizationService::new(AllowAllPolicy)
        .authorize::<A>(
            crate::test_support::authenticated_actor(
                ActorId::new("actor-admin").expect("valid actor ID"),
            )
            .await,
        )
        .await
        .expect("test policy should authorize request")
}
