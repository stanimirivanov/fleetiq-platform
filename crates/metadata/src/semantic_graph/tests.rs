use super::*;
use crate::AssetCatalogueEntry;
use fleetiq_access::{
    AccessDecision, TenantAccessPolicy, TenantAccessRequest, TenantAction,
    TenantAuthorizationService,
};
use fleetiq_domain::{
    ActorId, AssetId, AssetName, AssetRelationshipId, AssetTypeId, AssetTypeRef, DefinitionVersion,
    DeviceEndpointId, EffectiveInterval, PropertyDefinitionId, PropertyDefinitionRef,
    RecordRevision, RecordedRevision, RelationshipEndpoints, RelationshipTypeId,
    RelationshipTypeRef, SignalBindingId, SignalSource, SignalTarget, SourceSignalId, TenantId,
};
use std::{
    future::{Future, ready},
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
#[derive(Clone, Default)]
struct Model {
    assets: Vec<AssetCatalogueEntry>,
    relationships: Vec<AssetRelationship>,
    bindings: Vec<SignalBinding>,
}
struct Policy;
impl TenantAccessPolicy for Policy {
    type Error = io::Error;
    fn evaluate(
        &self,
        r: &TenantAccessRequest,
    ) -> impl Future<Output = Result<AccessDecision, Self::Error>> + Send {
        ready(Ok(
            if r.tenant_id().as_str() == "tenant-a" && r.action() == TenantAction::ReadMetadata {
                AccessDecision::Allow
            } else {
                AccessDecision::Deny
            },
        ))
    }
}
#[derive(Clone, Default)]
struct Repository {
    model: Model,
    calls: Arc<AtomicUsize>,
    fail: bool,
}
struct Read {
    model: Model,
    query: SemanticGraphQuery,
    calls: Arc<AtomicUsize>,
}
impl SemanticGraphRepository for Repository {
    type Error = io::Error;
    type Read = Read;
    fn open(
        &self,
        _: &TenantId,
        query: &SemanticGraphQuery,
    ) -> impl Future<Output = Result<Read, io::Error>> + Send {
        if self.fail {
            return ready(Err(io::Error::other("private storage diagnostic")));
        }
        ready(Ok(Read {
            model: self.model.clone(),
            query: query.clone(),
            calls: self.calls.clone(),
        }))
    }
}
impl SemanticGraphRead for Read {
    type Error = io::Error;
    fn assets(
        &mut self,
        ids: &[AssetId],
    ) -> impl Future<Output = Result<Vec<AssetCatalogueEntry>, io::Error>> + Send {
        assert!(ids.len() <= 100);
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut rows = self
            .model
            .assets
            .iter()
            .filter(|a| ids.contains(a.id()))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|a| a.id().clone());
        ready(Ok(rows))
    }
    fn relationships(
        &mut self,
        frontier: &[AssetId],
        excluded: &[AssetRelationshipId],
        limit: u16,
    ) -> impl Future<Output = Result<Vec<AssetRelationship>, io::Error>> + Send {
        assert!(frontier.len() <= 100 && limit <= 201);
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut rows = self
            .model
            .relationships
            .iter()
            .filter(|r| {
                !excluded.contains(r.id())
                    && (frontier.contains(r.source_asset_id())
                        || frontier.contains(r.target_asset_id()))
                    && r.recorded_at() <= self.query.known_at()
                    && r.effective_interval().contains(self.query.effective_at())
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|r| r.id().clone());
        rows.truncate(usize::from(limit));
        ready(Ok(rows))
    }
    fn bindings(
        &mut self,
        frontier: &[AssetId],
        excluded: &[SignalBindingId],
        limit: u16,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, io::Error>> + Send {
        assert!(frontier.len() <= 100 && limit <= 201);
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut rows = self
            .model
            .bindings
            .iter()
            .filter(|b| {
                !excluded.contains(b.id())
                    && (frontier.contains(b.source().device_asset_id())
                        || frontier.contains(b.target().asset_id()))
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|b| b.id().clone());
        rows.truncate(usize::from(limit));
        ready(Ok(rows))
    }
}
fn asset(id: &str) -> AssetCatalogueEntry {
    AssetCatalogueEntry::new(
        TenantId::new("tenant-a").unwrap(),
        AssetId::new(id).unwrap(),
        AssetName::new(id).unwrap(),
        AssetTypeRef::new(
            AssetTypeId::new("equipment").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
    )
}
fn relationship(id: &str, source: &str, target: &str) -> AssetRelationship {
    AssetRelationship::new(
        TenantId::new("tenant-a").unwrap(),
        AssetRelationshipId::new(id).unwrap(),
        RecordedRevision::new(RecordRevision::new(1).unwrap(), UnixTimeMillis::new(10)),
        RelationshipTypeRef::new(
            RelationshipTypeId::new("contains").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
        RelationshipEndpoints::new(AssetId::new(source).unwrap(), AssetId::new(target).unwrap()),
        EffectiveInterval::new(UnixTimeMillis::new(0), None).unwrap(),
    )
}
fn binding(id: &str, target: &str) -> SignalBinding {
    SignalBinding::new(
        TenantId::new("tenant-a").unwrap(),
        SignalBindingId::new(id).unwrap(),
        RecordedRevision::new(RecordRevision::new(1).unwrap(), UnixTimeMillis::new(10)),
        SignalSource::new(
            AssetId::new("a").unwrap(),
            DeviceEndpointId::new("can").unwrap(),
            SourceSignalId::new("voltage").unwrap(),
        ),
        SignalTarget::new(
            AssetId::new(target).unwrap(),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("voltage").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
        ),
        EffectiveInterval::new(UnixTimeMillis::new(0), None).unwrap(),
    )
}
async fn explore(
    repo: Repository,
    budget: GraphBudget,
) -> Result<Option<SemanticGraph>, GraphExplorationError<io::Error>> {
    let actor = crate::test_support::authenticated_actor(ActorId::new("operator-a").unwrap()).await;
    let grant = TenantAuthorizationService::new(Policy)
        .authorize(actor, TenantId::new("tenant-a").unwrap())
        .await
        .unwrap();
    SemanticGraphService::new(repo)
        .explore(
            grant,
            &SemanticGraphQuery::new(
                AssetId::new("a").unwrap(),
                UnixTimeMillis::new(50),
                UnixTimeMillis::new(50),
                budget,
            ),
        )
        .await
}
#[tokio::test]
async fn cycles_self_links_and_binding_self_monitoring_preserve_unique_vertices() {
    let repo = Repository {
        model: Model {
            assets: vec![asset("a"), asset("b"), asset("c")],
            relationships: vec![
                relationship("ab", "a", "b"),
                relationship("bc", "b", "c"),
                relationship("ca", "c", "a"),
                relationship("self", "a", "a"),
            ],
            bindings: vec![binding("self-binding", "a")],
        },
        ..Repository::default()
    };
    let calls = repo.calls.clone();
    let graph = explore(repo, GraphBudget::new(1, 100, 200, 200).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(graph.assets().len(), 3);
    assert_eq!(graph.relationships().len(), 4);
    assert_eq!(graph.bindings().len(), 1);
    assert_eq!(graph.truncation(), []);
    assert_eq!(calls.load(Ordering::Relaxed), 6); // two batches per visited layer, two asset batches
}
#[tokio::test]
async fn depth_and_size_limits_omit_edges_without_dangling_vertices() {
    let repo = Repository {
        model: Model {
            assets: vec![asset("a"), asset("b"), asset("c")],
            relationships: vec![relationship("ab", "a", "b"), relationship("ac", "a", "c")],
            bindings: vec![],
        },
        ..Repository::default()
    };
    let graph = explore(repo.clone(), GraphBudget::new(0, 100, 200, 200).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(graph.assets().len(), 1);
    assert_eq!(graph.relationships(), []);
    assert_eq!(graph.truncation(), [GraphTruncation::Depth]);
    let graph = explore(repo.clone(), GraphBudget::new(4, 2, 200, 200).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(graph.assets().len(), 2);
    assert_eq!(graph.relationships().len(), 1);
    assert!(graph.truncation().contains(&GraphTruncation::Assets));
    let graph = explore(repo, GraphBudget::new(4, 100, 1, 200).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(graph.relationships().len(), 1);
    assert!(graph.truncation().contains(&GraphTruncation::Relationships));
    for edge in graph.relationships() {
        assert!(
            graph
                .assets()
                .iter()
                .any(|a| a.id() == edge.source_asset_id())
                && graph
                    .assets()
                    .iter()
                    .any(|a| a.id() == edge.target_asset_id())
        );
    }
}
#[tokio::test]
async fn binding_budget_and_missing_roots_are_not_reported_as_complete_inventory() {
    let repo = Repository {
        model: Model {
            assets: vec![asset("a"), asset("b"), asset("c")],
            relationships: vec![],
            bindings: vec![binding("one", "b"), binding("two", "c")],
        },
        ..Repository::default()
    };
    let graph = explore(repo, GraphBudget::new(2, 100, 200, 1).unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(graph.bindings().len(), 1);
    assert!(graph.truncation().contains(&GraphTruncation::Bindings));
    assert!(
        explore(Repository::default(), GraphBudget::default())
            .await
            .unwrap()
            .is_none()
    );
    let error = explore(
        Repository {
            fail: true,
            ..Repository::default()
        },
        GraphBudget::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.to_string(), "semantic graph exploration failed");
    assert!(!error.to_string().contains("private storage"));
}
#[test]
fn invalid_budgets_are_rejected_at_the_application_boundary() {
    for (depth, assets, relationships, bindings) in [
        (5, 1, 1, 1),
        (0, 0, 1, 1),
        (0, 101, 1, 1),
        (0, 1, 0, 1),
        (0, 1, 201, 1),
        (0, 1, 1, 0),
        (0, 1, 1, 201),
    ] {
        assert!(GraphBudget::new(depth, assets, relationships, bindings).is_err());
    }
    assert!(GraphBudget::new(4, 100, 200, 200).is_ok());
}
