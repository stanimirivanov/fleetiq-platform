use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::ActorId;

use crate::{
    AccessDecision, AuthenticatedActor, PlatformAccessPolicy, PlatformAccessRequest,
    PlatformAction, PlatformAuthorizationService,
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

impl ExternalIdentityAuditRepository for RecordingRepository {
    type Error = TestRepositoryError;

    fn find_page<'a>(
        &'a self,
        _query: &'a ExternalIdentityAuditQuery,
    ) -> impl Future<Output = Result<ExternalIdentityAuditPage, Self::Error>> + Send + 'a {
        *self.calls.lock().expect("call lock poisoned") += 1;
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(ExternalIdentityAuditPage::new(Vec::new(), None))
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
        ExternalIdentityAuditPageSize::new(0),
        Err(ExternalIdentityAuditPageSizeError::Zero)
    );
    assert_eq!(
        ExternalIdentityAuditPageSize::new(1)
            .expect("minimum")
            .get(),
        1
    );
    assert_eq!(
        ExternalIdentityAuditPageSize::new(100)
            .expect("maximum")
            .get(),
        100
    );
    assert_eq!(
        ExternalIdentityAuditPageSize::new(101),
        Err(ExternalIdentityAuditPageSizeError::TooLarge {
            maximum: 100,
            actual: 101,
        })
    );
}

#[tokio::test]
async fn manage_identity_authorization_allows_one_query() {
    let repository = RecordingRepository::default();
    let page = ExternalIdentityAudit::new(repository.clone())
        .find_page(
            authorization(PlatformAction::ManageIdentity).await,
            &query(),
        )
        .await
        .expect("authorized query should succeed");

    assert!(page.entries().is_empty());
    assert_eq!(*repository.calls.lock().expect("call lock poisoned"), 1);
}

#[tokio::test]
async fn rejects_another_action_before_repository_access() {
    let repository = RecordingRepository::default();
    let error = ExternalIdentityAudit::new(repository.clone())
        .find_page(
            authorization(PlatformAction::ManagePlatformAccess).await,
            &query(),
        )
        .await
        .expect_err("platform access administration must not expose identity audit");

    assert!(matches!(
        error,
        ExternalIdentityAuditError::WrongAction {
            actual: PlatformAction::ManagePlatformAccess
        }
    ));
    assert_eq!(*repository.calls.lock().expect("call lock poisoned"), 0);
}

#[tokio::test]
async fn preserves_repository_failure_as_the_error_source() {
    let error = ExternalIdentityAudit::new(RecordingRepository::failing())
        .find_page(
            authorization(PlatformAction::ManageIdentity).await,
            &query(),
        )
        .await
        .expect_err("repository failure should be returned");

    assert!(matches!(
        error,
        ExternalIdentityAuditError::Repository(TestRepositoryError)
    ));
    assert!(error.source().is_some());
}

fn query() -> ExternalIdentityAuditQuery {
    ExternalIdentityAuditQuery::new(
        ExternalIdentityAuditPageSize::new(25).expect("valid page size"),
        None,
    )
}

async fn authorization(action: PlatformAction) -> crate::AuthorizedPlatformAction {
    PlatformAuthorizationService::new(AllowAllPolicy)
        .authorize(
            AuthenticatedActor::from_trusted_actor_id(
                ActorId::new("actor-admin").expect("valid actor ID"),
            ),
            action,
        )
        .await
        .expect("test policy should authorize request")
}
