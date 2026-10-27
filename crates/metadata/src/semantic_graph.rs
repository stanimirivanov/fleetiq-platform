//! Bounded semantic exploration with explicit temporal and completeness policy.
mod budget;
mod ports;
mod service;
#[cfg(test)]
mod tests;
mod traversal;
use crate::AssetCatalogueEntry;
pub use budget::{GraphBudget, InvalidGraphBudget};
use fleetiq_domain::{AssetId, AssetRelationship, SignalBinding, UnixTimeMillis};
pub use ports::{SemanticGraphRead, SemanticGraphRepository};
pub use service::{GraphExplorationError, SemanticGraphService};

/// One rooted graph at explicit effective/known cutoffs; root depth is zero.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticGraphQuery {
    root: AssetId,
    effective_at: UnixTimeMillis,
    known_at: UnixTimeMillis,
    budget: GraphBudget,
}
impl SemanticGraphQuery {
    /// Pins target, temporal semantics and validated traversal budgets.
    pub const fn new(
        root: AssetId,
        effective_at: UnixTimeMillis,
        known_at: UnixTimeMillis,
        budget: GraphBudget,
    ) -> Self {
        Self {
            root,
            effective_at,
            known_at,
            budget,
        }
    }
    /// Returns the root asset; absence is not an empty known graph.
    pub const fn root(&self) -> &AssetId {
        &self.root
    }
    /// Returns physical-time applicability cutoff.
    pub const fn effective_at(&self) -> UnixTimeMillis {
        self.effective_at
    }
    /// Returns latest-known-revision cutoff, independent of event time.
    pub const fn known_at(&self) -> UnixTimeMillis {
        self.known_at
    }
    /// Returns enforced traversal/output ceilings.
    pub const fn budget(&self) -> GraphBudget {
        self.budget
    }
}
/// Why the returned graph cannot claim complete reachable adjacency.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum GraphTruncation {
    /// An observed edge would introduce an asset beyond the depth budget.
    Depth,
    /// An observed edge would exceed the unique asset budget.
    Assets,
    /// A relationship adjacency query had more than its bounded prefix.
    Relationships,
    /// A binding adjacency query had more than its bounded prefix.
    Bindings,
}
/// One internally consistent bounded subgraph, not a full-graph inventory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticGraph {
    query: SemanticGraphQuery,
    assets: Vec<AssetCatalogueEntry>,
    relationships: Vec<AssetRelationship>,
    bindings: Vec<SignalBinding>,
    truncation: Vec<GraphTruncation>,
}
impl SemanticGraph {
    /// Returns the exact root, cutoffs and applied budgets.
    pub const fn query(&self) -> &SemanticGraphQuery {
        &self.query
    }
    /// Returns ID-ordered unique assets, including the root.
    pub fn assets(&self) -> &[AssetCatalogueEntry] {
        &self.assets
    }
    /// Returns ID-ordered typed directed revisions with both endpoints included.
    pub fn relationships(&self) -> &[AssetRelationship] {
        &self.relationships
    }
    /// Returns ID-ordered source/target attributions with both assets included.
    pub fn bindings(&self) -> &[SignalBinding] {
        &self.bindings
    }
    /// Returns observed cut reasons; empty means complete at this database snapshot.
    pub fn truncation(&self) -> &[GraphTruncation] {
        &self.truncation
    }
}
