use super::*;
use crate::{AccessDecision, TenantAccessPolicy, TenantAccessRequest, TenantAuthorizationService};
use fleetiq_domain::{
    AssetId, AssetRelationshipId, DefinitionVersion, EffectiveInterval, RecordedRevision,
    RelationshipEndpoints, RelationshipTypeId, RelationshipTypeRef,
};
use std::{
    convert::Infallible,
    future::ready,
    sync::{Arc, Mutex},
};
type Call = (MetadataRevision, ActorId);
#[derive(Clone, Default)]
struct Recorder {
    calls: Arc<Mutex<Vec<Call>>>,
    fail: bool,
}
impl RevisionCommandRepository for Recorder {
    type Error = std::io::Error;
    fn append(
        &self,
        value: &MetadataRevision,
        actor: &ActorId,
    ) -> impl Future<Output = Result<RevisionOutcome, Self::Error>> + Send {
        self.calls
            .lock()
            .unwrap()
            .push((value.clone(), actor.clone()));
        ready(if self.fail {
            Err(std::io::Error::other("storage failed"))
        } else {
            Ok(RevisionOutcome::Appended(RevisionReceipt::new(
                actor.clone(),
                value.recorded_at(),
            )))
        })
    }
}
struct Allow;
impl TenantAccessPolicy for Allow {
    type Error = Infallible;
    fn evaluate(
        &self,
        _: &TenantAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Infallible>> + Send {
        ready(Ok(AccessDecision::Allow))
    }
}
async fn grant() -> AuthorizedTenant<ManageMetadata> {
    TenantAuthorizationService::new(Allow)
        .authorize::<ManageMetadata>(
            crate::test_support::authenticated_actor(ActorId::new("operator-a").unwrap()).await,
            TenantId::new("tenant-a").unwrap(),
        )
        .await
        .unwrap()
}
fn relationship(tenant: &str, time: i64, target: &str) -> MetadataRevision {
    MetadataRevision::Relationship(AssetRelationship::new(
        TenantId::new(tenant).unwrap(),
        AssetRelationshipId::new("fact-a").unwrap(),
        RecordedRevision::new(RecordRevision::new(1).unwrap(), UnixTimeMillis::new(time)),
        RelationshipTypeRef::new(
            RelationshipTypeId::new("contains").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
        RelationshipEndpoints::new(
            AssetId::new("source").unwrap(),
            AssetId::new(target).unwrap(),
        ),
        EffectiveInterval::new(UnixTimeMillis::new(0), None).unwrap(),
    ))
}
#[tokio::test]
async fn consumed_authority_selects_actor_and_foreign_scope_never_reaches_storage() {
    let repo = Recorder::default();
    let service = RevisionCommandService::new(repo.clone());
    let value = relationship("tenant-a", 123, "target");
    assert_eq!(
        service.append(grant().await, &value).await.unwrap(),
        RevisionOutcome::Appended(RevisionReceipt::new(
            ActorId::new("operator-a").unwrap(),
            UnixTimeMillis::new(123)
        ))
    );
    assert!(matches!(
        service
            .append(grant().await, &relationship("tenant-b", 123, "target"))
            .await,
        Err(RevisionCommandError::TenantMismatch)
    ));
    assert_eq!(
        *repo.calls.lock().unwrap(),
        vec![(value, ActorId::new("operator-a").unwrap())]
    );
}
#[tokio::test]
async fn unresolved_repository_failure_preserves_source() {
    let service = RevisionCommandService::new(Recorder {
        fail: true,
        ..Recorder::default()
    });
    let e = service
        .append(grant().await, &relationship("tenant-a", 0, "target"))
        .await
        .unwrap_err();
    assert!(matches!(e, RevisionCommandError::Repository(_)));
    assert!(e.source().is_some());
}
#[test]
fn retry_equality_excludes_receipt_time_but_includes_endpoints_and_tenant() {
    let first = relationship("tenant-a", 10, "target");
    assert!(first.same_content(&relationship("tenant-a", 999, "target")));
    assert!(!first.same_content(&relationship("tenant-a", 10, "other")));
    assert!(!first.same_content(&relationship("tenant-b", 10, "target")));
}
