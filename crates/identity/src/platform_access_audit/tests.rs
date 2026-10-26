use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::ActorId;

use crate::{
    AccessDecision, ManagePlatformAccess, PlatformAccessPolicy, PlatformAccessRequest,
    PlatformAuthorizationService,
};

use super::{
    PlatformAccessAudit, PlatformAccessAuditError, PlatformAccessAuditPage,
    PlatformAccessAuditPageSize, PlatformAccessAuditPageSizeError, PlatformAccessAuditQuery,
    PlatformAccessAuditRepository,
};

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

impl PlatformAccessAuditRepository for RecordingRepository {
    type Error = TestRepositoryError;

    fn find_page<'a>(
        &'a self,
        _query: &'a PlatformAccessAuditQuery,
    ) -> impl Future<Output = Result<PlatformAccessAuditPage, Self::Error>> + Send + 'a {
        *self.calls.lock().expect("call lock poisoned") += 1;
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(PlatformAccessAuditPage::new(Vec::new(), None))
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
        PlatformAccessAuditPageSize::new(0),
        Err(PlatformAccessAuditPageSizeError::Zero)
    );
    assert_eq!(
        PlatformAccessAuditPageSize::new(1).expect("minimum").get(),
        1
    );
    assert_eq!(
        PlatformAccessAuditPageSize::new(100)
            .expect("maximum")
            .get(),
        100
    );
    assert_eq!(
        PlatformAccessAuditPageSize::new(101),
        Err(PlatformAccessAuditPageSizeError::TooLarge {
            maximum: 100,
            actual: 101,
        })
    );
}

#[tokio::test]
async fn manage_platform_access_authorization_allows_one_query() {
    let repository = RecordingRepository::default();
    let page = PlatformAccessAudit::new(repository.clone())
        .find_page(authorization::<ManagePlatformAccess>().await, &query())
        .await
        .expect("authorized query should succeed");

    assert_eq!(page.entries().len(), 0);
    assert_eq!(*repository.calls.lock().expect("call lock poisoned"), 1);
}

#[tokio::test]
async fn preserves_repository_failure_as_the_error_source() {
    let error = PlatformAccessAudit::new(RecordingRepository::failing())
        .find_page(authorization::<ManagePlatformAccess>().await, &query())
        .await
        .expect_err("repository failure should be returned");

    assert!(matches!(
        error,
        PlatformAccessAuditError::Repository(TestRepositoryError)
    ));
    assert!(error.source().is_some());
}

fn query() -> PlatformAccessAuditQuery {
    PlatformAccessAuditQuery::new(
        PlatformAccessAuditPageSize::new(25).expect("valid page size"),
        None,
    )
}

async fn authorization<A: crate::PlatformActionMarker>() -> crate::AuthorizedPlatform<A> {
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
