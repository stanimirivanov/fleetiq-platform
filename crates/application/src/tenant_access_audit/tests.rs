use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::{ActorId, TenantId};

use crate::{
    AccessDecision, AuthenticatedActor, TenantAccessPolicy, TenantAccessRequest, TenantAction,
    TenantAuthorizationService,
};

use super::{
    TenantAccessAudit, TenantAccessAuditError, TenantAccessAuditPage, TenantAccessAuditPageSize,
    TenantAccessAuditPageSizeError, TenantAccessAuditQuery, TenantAccessAuditRepository,
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
    tenants: Arc<Mutex<Vec<TenantId>>>,
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

impl TenantAccessAuditRepository for RecordingRepository {
    type Error = TestRepositoryError;

    fn find_page<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        _query: &'a TenantAccessAuditQuery,
    ) -> impl Future<Output = Result<TenantAccessAuditPage, Self::Error>> + Send + 'a {
        self.tenants
            .lock()
            .expect("tenant lock poisoned")
            .push(tenant_id.clone());
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(TenantAccessAuditPage::new(Vec::new(), None))
        })
    }
}

struct AllowAllPolicy;

impl TenantAccessPolicy for AllowAllPolicy {
    type Error = Infallible;

    fn evaluate<'a>(
        &'a self,
        _request: &'a TenantAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send + 'a {
        ready(Ok(AccessDecision::Allow))
    }
}

#[test]
fn page_size_is_bounded() {
    assert_eq!(
        TenantAccessAuditPageSize::new(0),
        Err(TenantAccessAuditPageSizeError::Zero)
    );
    assert_eq!(TenantAccessAuditPageSize::new(1).expect("minimum").get(), 1);
    assert_eq!(
        TenantAccessAuditPageSize::new(100).expect("maximum").get(),
        100
    );
    assert_eq!(
        TenantAccessAuditPageSize::new(101),
        Err(TenantAccessAuditPageSizeError::TooLarge {
            maximum: 100,
            actual: 101,
        })
    );
}

#[tokio::test]
async fn derives_tenant_scope_from_manage_access_authorization() {
    let repository = RecordingRepository::default();
    let service = TenantAccessAudit::new(repository.clone());

    let page = service
        .find_page(
            authorization("tenant-a", TenantAction::ManageAccess).await,
            &query(),
        )
        .await
        .expect("authorized query should succeed");

    assert!(page.entries().is_empty());
    assert_eq!(
        repository
            .tenants
            .lock()
            .expect("tenant lock poisoned")
            .as_slice(),
        &[TenantId::new("tenant-a").expect("valid tenant ID")]
    );
}

#[tokio::test]
async fn rejects_another_action_before_repository_access() {
    let repository = RecordingRepository::default();
    let service = TenantAccessAudit::new(repository.clone());

    let error = service
        .find_page(
            authorization("tenant-a", TenantAction::ReadMetadata).await,
            &query(),
        )
        .await
        .expect_err("metadata read authorization must not expose access audit");

    assert!(matches!(
        error,
        TenantAccessAuditError::ManageAccessRequired {
            actual: TenantAction::ReadMetadata
        }
    ));
    assert!(
        repository
            .tenants
            .lock()
            .expect("tenant lock poisoned")
            .is_empty()
    );
}

#[tokio::test]
async fn preserves_repository_failure_as_the_error_source() {
    let service = TenantAccessAudit::new(RecordingRepository::failing());

    let error = service
        .find_page(
            authorization("tenant-a", TenantAction::ManageAccess).await,
            &query(),
        )
        .await
        .expect_err("repository failure should be returned");

    assert!(matches!(
        error,
        TenantAccessAuditError::Repository(TestRepositoryError)
    ));
    assert!(error.source().is_some());
}

fn query() -> TenantAccessAuditQuery {
    TenantAccessAuditQuery::new(
        TenantAccessAuditPageSize::new(25).expect("valid page size"),
        None,
    )
}

async fn authorization(tenant: &str, action: TenantAction) -> crate::AuthorizedTenantAction {
    TenantAuthorizationService::new(AllowAllPolicy)
        .authorize(
            AuthenticatedActor::from_trusted_actor_id(
                ActorId::new("actor-admin").expect("valid actor ID"),
            ),
            TenantId::new(tenant).expect("valid tenant ID"),
            action,
        )
        .await
        .expect("test policy should authorize request")
}
