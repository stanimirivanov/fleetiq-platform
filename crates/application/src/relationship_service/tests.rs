use std::{
    convert::Infallible,
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::{
    ActorId, AssetId, AssetRelationship, AssetRelationshipId, DefinitionVersion, EffectiveInterval,
    RecordRevision, RecordedRevision, RelationshipEndpoints, RelationshipTypeId,
    RelationshipTypeRef, TenantId, UnixTimeMillis,
};

use crate::{
    AccessDecision, AuthorizedTenant, ManageMetadata, ReadMetadata, RelationshipRepository,
    RelationshipSnapshotQuery, TenantAccessPolicy, TenantAccessRequest, TenantAuthorizationService,
};

use super::{RelationshipApplicationError, RelationshipApplicationService};

#[derive(Clone, Debug, Eq, PartialEq)]
enum RepositoryCall {
    InsertRevision(AssetRelationship),
    FindRevision(TenantId, AssetRelationshipId, RecordRevision),
    FindForAsset(TenantId, AssetId, RelationshipSnapshotQuery),
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
struct RecordingRelationshipRepository {
    calls: Arc<Mutex<Vec<RepositoryCall>>>,
    fail: bool,
    exact_revision: Option<AssetRelationship>,
    snapshot: Vec<AssetRelationship>,
}

impl RecordingRelationshipRepository {
    fn returning(relationship: AssetRelationship) -> Self {
        Self {
            exact_revision: Some(relationship.clone()),
            snapshot: vec![relationship],
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

impl RelationshipRepository for RecordingRelationshipRepository {
    type Error = TestRepositoryError;

    fn insert_revision(
        &self,
        relationship: &AssetRelationship,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.record(RepositoryCall::InsertRevision(relationship.clone()));
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(())
        })
    }

    fn find_revision(
        &self,
        tenant_id: &TenantId,
        relationship_id: &AssetRelationshipId,
        revision: RecordRevision,
    ) -> impl Future<Output = Result<Option<AssetRelationship>, Self::Error>> + Send {
        self.record(RepositoryCall::FindRevision(
            tenant_id.clone(),
            relationship_id.clone(),
            revision,
        ));
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(self.exact_revision.clone())
        })
    }

    fn find_for_asset(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
        query: RelationshipSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<AssetRelationship>, Self::Error>> + Send {
        self.record(RepositoryCall::FindForAsset(
            tenant_id.clone(),
            asset_id.clone(),
            query,
        ));
        ready(if self.fail {
            Err(TestRepositoryError)
        } else {
            Ok(self.snapshot.clone())
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
    let repository = RecordingRelationshipRepository::default();
    let service = RelationshipApplicationService::new(repository.clone());
    let relationship = relationship("tenant-a");

    service
        .append_revision(manage_authorization("tenant-a").await, &relationship)
        .await
        .expect("matching tenant revision should be appended");

    assert_eq!(
        repository.calls(),
        vec![RepositoryCall::InsertRevision(relationship)]
    );
}

#[tokio::test]
async fn rejects_wrong_tenant_before_append() {
    let repository = RecordingRelationshipRepository::default();
    let service = RelationshipApplicationService::new(repository.clone());
    let relationship = relationship("tenant-b");

    let wrong_tenant = service
        .append_revision(manage_authorization("tenant-a").await, &relationship)
        .await
        .expect_err("cross-tenant relationship must be rejected");
    assert!(matches!(
        wrong_tenant,
        RelationshipApplicationError::TenantMismatch {
            authorized_tenant_id,
            relationship_tenant_id,
        } if authorized_tenant_id.as_str() == "tenant-a"
            && relationship_tenant_id.as_str() == "tenant-b"
    ));
    assert!(repository.calls().is_empty());
}

#[tokio::test]
async fn reads_exact_revision_and_snapshot_in_the_authorized_tenant() {
    let relationship = relationship("tenant-a");
    let repository = RecordingRelationshipRepository::returning(relationship.clone());
    let service = RelationshipApplicationService::new(repository.clone());
    let relationship_id = relationship.id().clone();
    let revision = relationship.revision();
    let asset_id = relationship.source_asset_id().clone();
    let query =
        RelationshipSnapshotQuery::new(UnixTimeMillis::new(1_500), UnixTimeMillis::new(2_500));

    let exact = service
        .find_revision(
            read_authorization("tenant-a").await,
            &relationship_id,
            revision,
        )
        .await
        .expect("exact revision lookup should succeed");
    let snapshot = service
        .find_for_asset(read_authorization("tenant-a").await, &asset_id, query)
        .await
        .expect("snapshot lookup should succeed");

    assert_eq!(exact, Some(relationship.clone()));
    assert_eq!(snapshot, vec![relationship]);
    assert_eq!(
        repository.calls(),
        vec![
            RepositoryCall::FindRevision(tenant_id("tenant-a"), relationship_id, revision),
            RepositoryCall::FindForAsset(tenant_id("tenant-a"), asset_id, query),
        ]
    );
}

#[tokio::test]
async fn preserves_repository_failure_as_the_error_source() {
    let service = RelationshipApplicationService::new(RecordingRelationshipRepository::failing());

    let error = service
        .find_for_asset(
            read_authorization("tenant-a").await,
            &AssetId::new("assembly-a").expect("valid asset ID"),
            RelationshipSnapshotQuery::new(UnixTimeMillis::new(1_500), UnixTimeMillis::new(2_500)),
        )
        .await
        .expect_err("repository failure should be returned");

    assert!(matches!(
        error,
        RelationshipApplicationError::Repository(TestRepositoryError)
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

fn relationship(tenant: &str) -> AssetRelationship {
    AssetRelationship::new(
        tenant_id(tenant),
        AssetRelationshipId::new("relationship-1").expect("valid relationship ID"),
        RecordedRevision::new(
            RecordRevision::new(1).expect("valid record revision"),
            UnixTimeMillis::new(2_000),
        ),
        RelationshipTypeRef::new(
            RelationshipTypeId::new("physical.contains").expect("valid relationship type ID"),
            DefinitionVersion::new(1).expect("valid definition version"),
        ),
        RelationshipEndpoints::new(
            AssetId::new("assembly-a").expect("valid source asset ID"),
            AssetId::new("component-b").expect("valid target asset ID"),
        ),
        EffectiveInterval::new(UnixTimeMillis::new(1_000), Some(UnixTimeMillis::new(3_000)))
            .expect("valid effective interval"),
    )
}

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("valid tenant ID")
}
