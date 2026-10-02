use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::{
    ActorId, AssetId, DefinitionVersion, DeviceEndpointId, EffectiveInterval, PropertyDefinitionId,
    PropertyDefinitionRef, RecordRevision, RecordedRevision, SignalBinding, SignalBindingId,
    SignalSource, SignalTarget, SourceSignalId, TenantId, UnixTimeMillis,
};

use crate::{
    AccessDecision, AuthorizedTenant, ManageMetadata, ReadMetadata, SignalBindingRepository,
    SignalBindingSnapshotPage, SignalBindingSnapshotPageQuery, SignalBindingSnapshotQuery,
    TenantAccessPolicy, TenantAccessRequest, TenantAuthorizationService,
};

use super::{SignalBindingApplicationError, SignalBindingApplicationService};

#[derive(Clone, Debug, Eq, PartialEq)]
enum RepositoryCall {
    InsertRevision(SignalBinding),
    FindRevision(TenantId, SignalBindingId, RecordRevision),
    FindForSource(TenantId, SignalSource, SignalBindingSnapshotQuery),
    FindForSourcePage(TenantId, SignalSource, SignalBindingSnapshotPageQuery),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestRepositoryError;

impl fmt::Display for TestRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("test repository failed")
    }
}

impl Error for TestRepositoryError {}

#[derive(Clone, Debug, Default)]
struct RecordingSignalBindingRepository {
    calls: Arc<Mutex<Vec<RepositoryCall>>>,
    fail: bool,
    exact_revision: Option<SignalBinding>,
    snapshot: Vec<SignalBinding>,
}

impl RecordingSignalBindingRepository {
    fn returning(binding: SignalBinding) -> Self {
        Self {
            exact_revision: Some(binding.clone()),
            snapshot: vec![binding],
            ..Self::default()
        }
    }

    fn failing() -> Self {
        Self {
            fail: true,
            ..Self::default()
        }
    }

    fn calls(&self) -> Vec<RepositoryCall> {
        self.calls.lock().expect("calls lock poisoned").clone()
    }

    fn record(&self, call: RepositoryCall) {
        self.calls.lock().expect("calls lock poisoned").push(call);
    }
}

impl SignalBindingRepository for RecordingSignalBindingRepository {
    type Error = TestRepositoryError;

    fn insert_revision(
        &self,
        binding: &SignalBinding,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.record(RepositoryCall::InsertRevision(binding.clone()));
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(())
        })
    }

    fn find_revision(
        &self,
        tenant_id: &TenantId,
        binding_id: &SignalBindingId,
        revision: RecordRevision,
    ) -> impl Future<Output = Result<Option<SignalBinding>, Self::Error>> + Send {
        self.record(RepositoryCall::FindRevision(
            tenant_id.clone(),
            binding_id.clone(),
            revision,
        ));
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(self.exact_revision.clone())
        })
    }

    fn find_for_source(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send {
        self.record(RepositoryCall::FindForSource(
            tenant_id.clone(),
            source.clone(),
            query,
        ));
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(self.snapshot.clone())
        })
    }

    fn find_for_source_page(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: &SignalBindingSnapshotPageQuery,
    ) -> impl Future<Output = Result<SignalBindingSnapshotPage, Self::Error>> + Send {
        self.record(RepositoryCall::FindForSourcePage(
            tenant_id.clone(),
            source.clone(),
            query.clone(),
        ));
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(SignalBindingSnapshotPage::new(self.snapshot.clone(), None))
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

#[tokio::test]
async fn appends_revision_owned_by_the_authorized_tenant() {
    let repository = RecordingSignalBindingRepository::default();
    let service = SignalBindingApplicationService::new(repository.clone());
    let binding = binding("tenant-a");

    service
        .append_revision(manage_authorization("tenant-a").await, &binding)
        .await
        .expect("matching tenant revision should be appended");

    assert_eq!(
        repository.calls(),
        vec![RepositoryCall::InsertRevision(binding)]
    );
}

#[tokio::test]
async fn rejects_wrong_tenant_before_append() {
    let repository = RecordingSignalBindingRepository::default();
    let service = SignalBindingApplicationService::new(repository.clone());
    let binding = binding("tenant-b");

    let wrong_tenant = service
        .append_revision(manage_authorization("tenant-a").await, &binding)
        .await
        .expect_err("cross-tenant binding must be rejected");
    assert!(matches!(
        wrong_tenant,
        SignalBindingApplicationError::TenantMismatch {
            authorized_tenant_id,
            binding_tenant_id,
        } if authorized_tenant_id.as_str() == "tenant-a"
            && binding_tenant_id.as_str() == "tenant-b"
    ));
    assert!(repository.calls().is_empty());
}

#[tokio::test]
async fn reads_exact_revision_and_snapshot_in_the_authorized_tenant() {
    let binding = binding("tenant-a");
    let repository = RecordingSignalBindingRepository::returning(binding.clone());
    let service = SignalBindingApplicationService::new(repository.clone());
    let binding_id = binding.id().clone();
    let revision = binding.revision();
    let source = binding.source().clone();
    let query =
        SignalBindingSnapshotQuery::new(UnixTimeMillis::new(1_500), UnixTimeMillis::new(2_500));

    let exact = service
        .find_revision(read_authorization("tenant-a").await, &binding_id, revision)
        .await
        .expect("exact revision lookup should succeed");
    let snapshot = service
        .find_for_source(read_authorization("tenant-a").await, &source, query)
        .await
        .expect("snapshot lookup should succeed");
    let page_query = SignalBindingSnapshotPageQuery::new(query, 1, None).unwrap();
    let page = service
        .find_for_source_page(read_authorization("tenant-a").await, &source, &page_query)
        .await
        .expect("page lookup should succeed");

    assert_eq!(exact, Some(binding.clone()));
    assert_eq!(snapshot, vec![binding]);
    assert_eq!(page.bindings(), snapshot);
    assert_eq!(
        repository.calls(),
        vec![
            RepositoryCall::FindRevision(tenant_id("tenant-a"), binding_id, revision),
            RepositoryCall::FindForSource(tenant_id("tenant-a"), source.clone(), query),
            RepositoryCall::FindForSourcePage(tenant_id("tenant-a"), source, page_query,),
        ]
    );
}

#[tokio::test]
async fn preserves_repository_failure_as_the_error_source() {
    let service = SignalBindingApplicationService::new(RecordingSignalBindingRepository::failing());

    let error = service
        .find_for_source(
            read_authorization("tenant-a").await,
            binding("tenant-a").source(),
            SignalBindingSnapshotQuery::new(UnixTimeMillis::new(1_500), UnixTimeMillis::new(2_500)),
        )
        .await
        .expect_err("repository failure should be returned");

    assert!(matches!(
        error,
        SignalBindingApplicationError::Repository(TestRepositoryError)
    ));
    assert!(error.source().is_some());
}

async fn manage_authorization(tenant: &str) -> AuthorizedTenant<ManageMetadata> {
    authorization::<ManageMetadata>(tenant).await
}

async fn read_authorization(tenant: &str) -> AuthorizedTenant<ReadMetadata> {
    authorization::<ReadMetadata>(tenant).await
}

async fn authorization<A: crate::TenantActionMarker>(tenant: &str) -> AuthorizedTenant<A> {
    TenantAuthorizationService::new(AllowAllPolicy)
        .authorize::<A>(
            crate::test_support::authenticated_actor(
                ActorId::new("actor-a").expect("valid actor ID"),
            )
            .await,
            tenant_id(tenant),
        )
        .await
        .expect("test policy should authorize request")
}

fn binding(tenant: &str) -> SignalBinding {
    SignalBinding::new(
        tenant_id(tenant),
        SignalBindingId::new("binding-1").expect("valid binding ID"),
        RecordedRevision::new(
            RecordRevision::new(1).expect("valid record revision"),
            UnixTimeMillis::new(2_000),
        ),
        SignalSource::new(
            AssetId::new("gateway-1").expect("valid device asset ID"),
            DeviceEndpointId::new("can-primary").expect("valid endpoint ID"),
            SourceSignalId::new("electrical.voltage").expect("valid source signal ID"),
        ),
        SignalTarget::new(
            AssetId::new("traction-motor-1").expect("valid target asset ID"),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("electrical.voltage")
                    .expect("valid property definition ID"),
                DefinitionVersion::new(1).expect("valid definition version"),
            ),
        ),
        EffectiveInterval::new(UnixTimeMillis::new(1_000), None).expect("valid effective interval"),
    )
}

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("valid tenant ID")
}
