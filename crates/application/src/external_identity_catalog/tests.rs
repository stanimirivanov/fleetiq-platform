use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::{ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId};

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

impl ExternalIdentityCatalogRepository for RecordingRepository {
    type Error = TestRepositoryError;

    fn find_page<'a>(
        &'a self,
        _query: &'a ExternalIdentityCatalogQuery,
    ) -> impl Future<Output = Result<ExternalIdentityCatalogPage, Self::Error>> + Send + 'a {
        *self.calls.lock().expect("call lock poisoned") += 1;
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(ExternalIdentityCatalogPage::new(Vec::new(), None))
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
        ExternalIdentityCatalogPageSize::new(0),
        Err(ExternalIdentityCatalogPageSizeError::Zero)
    );
    assert_eq!(
        ExternalIdentityCatalogPageSize::new(1)
            .expect("minimum")
            .get(),
        1
    );
    assert_eq!(
        ExternalIdentityCatalogPageSize::new(100)
            .expect("maximum")
            .get(),
        100
    );
    assert_eq!(
        ExternalIdentityCatalogPageSize::new(101),
        Err(ExternalIdentityCatalogPageSizeError::TooLarge {
            maximum: 100,
            actual: 101,
        })
    );
}

#[test]
fn debug_output_redacts_opaque_subjects() {
    let entry = ExternalIdentityCatalogEntry::new(identity("oidc-a", "secret-subject"));
    let cursor = ExternalIdentityCatalogCursor::new(
        IdentityAuthorityId::new("oidc-a").expect("valid authority"),
        ExternalIdentitySubject::new("secret-subject").expect("valid subject"),
    );

    assert!(!format!("{entry:?}").contains("secret-subject"));
    assert!(!format!("{cursor:?}").contains("secret-subject"));
}

#[tokio::test]
async fn identity_management_authorization_allows_one_query() {
    let repository = RecordingRepository::default();
    let page = ExternalIdentityCatalog::new(repository.clone())
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
    let error = ExternalIdentityCatalog::new(repository.clone())
        .find_page(
            authorization(PlatformAction::ManagePlatformAccess).await,
            &query(),
        )
        .await
        .expect_err("platform access management must not expose login mappings");

    assert!(matches!(
        error,
        ExternalIdentityCatalogError::WrongAction {
            actual: PlatformAction::ManagePlatformAccess
        }
    ));
    assert_eq!(*repository.calls.lock().expect("call lock poisoned"), 0);
}

#[tokio::test]
async fn preserves_repository_failure_as_the_error_source() {
    let repository = RecordingRepository {
        fail: true,
        ..RecordingRepository::default()
    };
    let error = ExternalIdentityCatalog::new(repository)
        .find_page(
            authorization(PlatformAction::ManageIdentity).await,
            &query(),
        )
        .await
        .expect_err("repository failure should be returned");

    assert!(matches!(
        error,
        ExternalIdentityCatalogError::Repository(TestRepositoryError)
    ));
    assert!(error.source().is_some());
}

fn query() -> ExternalIdentityCatalogQuery {
    ExternalIdentityCatalogQuery::new(
        ActorId::new("actor-a").expect("valid actor ID"),
        ExternalIdentityCatalogPageSize::new(25).expect("valid page size"),
        None,
    )
}

async fn authorization(action: PlatformAction) -> AuthorizedPlatformAction {
    PlatformAuthorizationService::new(AllowAllPolicy)
        .authorize(
            crate::test_support::authenticated_actor(
                ActorId::new("actor-admin").expect("valid actor ID"),
            )
            .await,
            action,
        )
        .await
        .expect("test policy should authorize request")
}

fn identity(authority: &str, subject: &str) -> ExternalIdentity {
    ExternalIdentity::new(
        IdentityAuthorityId::new(authority).expect("valid authority"),
        ExternalIdentitySubject::new(subject).expect("valid subject"),
    )
}
