//! Reuse existing asset/relationship/binding wire types and pinned references.
use crate::{
    protected_asset::AssetSummaryResponse, protected_relationship::RelationshipResponse,
    protected_signal_binding::SignalBindingResponse,
};
use fleetiq_domain::{AssetId, UnixTimeMillis};
use fleetiq_metadata::{GraphBudget, GraphTruncation, SemanticGraph, SemanticGraphQuery};
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Parameters {
    effective_at_ms: i64,
    known_at_ms: i64,
    depth: Option<u8>,
    asset_limit: Option<u8>,
    relationship_limit: Option<u16>,
    binding_limit: Option<u16>,
}
impl Parameters {
    pub(super) fn query(self, root: AssetId) -> Option<SemanticGraphQuery> {
        let defaults = GraphBudget::default();
        let budget = GraphBudget::new(
            self.depth.unwrap_or(defaults.depth()),
            self.asset_limit.unwrap_or(defaults.assets()),
            self.relationship_limit.unwrap_or(defaults.relationships()),
            self.binding_limit.unwrap_or(defaults.bindings()),
        )
        .ok()?;
        Some(SemanticGraphQuery::new(
            root,
            UnixTimeMillis::new(self.effective_at_ms),
            UnixTimeMillis::new(self.known_at_ms),
            budget,
        ))
    }
}
#[derive(Serialize)]
pub(super) struct GraphBody<'a> {
    root_asset_id: &'a str,
    effective_at_ms: i64,
    known_at_ms: i64,
    budget: BudgetBody,
    assets: Vec<AssetSummaryResponse<'a>>,
    relationships: Vec<RelationshipResponse<'a>>,
    bindings: Vec<SignalBindingResponse<'a>>,
    truncated: bool,
    truncation: Vec<&'static str>,
}
#[derive(Serialize)]
struct BudgetBody {
    depth: u8,
    assets: u8,
    relationships: u16,
    bindings: u16,
}
impl<'a> From<&'a SemanticGraph> for GraphBody<'a> {
    fn from(graph: &'a SemanticGraph) -> Self {
        let query = graph.query();
        let budget = query.budget();
        Self {
            root_asset_id: query.root().as_str(),
            effective_at_ms: query.effective_at().get(),
            known_at_ms: query.known_at().get(),
            budget: BudgetBody {
                depth: budget.depth(),
                assets: budget.assets(),
                relationships: budget.relationships(),
                bindings: budget.bindings(),
            },
            assets: graph
                .assets()
                .iter()
                .map(AssetSummaryResponse::from)
                .collect(),
            relationships: graph
                .relationships()
                .iter()
                .map(RelationshipResponse::from)
                .collect(),
            bindings: graph
                .bindings()
                .iter()
                .map(SignalBindingResponse::from)
                .collect(),
            truncated: !graph.truncation().is_empty(),
            truncation: graph
                .truncation()
                .iter()
                .map(|r| match r {
                    GraphTruncation::Depth => "depth",
                    GraphTruncation::Assets => "assets",
                    GraphTruncation::Relationships => "relationships",
                    GraphTruncation::Bindings => "bindings",
                })
                .collect(),
        }
    }
}
