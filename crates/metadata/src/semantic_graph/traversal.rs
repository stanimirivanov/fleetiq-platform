//! Deterministic breadth-first inclusion policy, independent of storage.
use super::{GraphBudget, GraphTruncation};
use fleetiq_domain::{
    AssetId, AssetRelationship, AssetRelationshipId, SignalBinding, SignalBindingId,
};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Traversal {
    pub(super) assets: BTreeSet<AssetId>,
    pub(super) relationships: BTreeMap<AssetRelationshipId, AssetRelationship>,
    pub(super) bindings: BTreeMap<SignalBindingId, SignalBinding>,
    pub(super) truncation: BTreeSet<GraphTruncation>,
}
impl Traversal {
    pub(super) fn new(root: AssetId) -> Self {
        Self {
            assets: BTreeSet::from([root]),
            relationships: BTreeMap::new(),
            bindings: BTreeMap::new(),
            truncation: BTreeSet::new(),
        }
    }
    pub(super) fn add_relationships(
        &mut self,
        mut rows: Vec<AssetRelationship>,
        depth: u8,
        budget: GraphBudget,
        next: &mut BTreeSet<AssetId>,
    ) {
        let remaining =
            usize::from(budget.relationships()).saturating_sub(self.relationships.len());
        if rows.len() > remaining {
            self.truncation.insert(GraphTruncation::Relationships);
            rows.truncate(remaining);
        }
        for row in rows {
            if self.admit(
                row.source_asset_id(),
                row.target_asset_id(),
                depth,
                budget,
                next,
            ) {
                self.relationships.insert(row.id().clone(), row);
            }
        }
    }
    pub(super) fn add_bindings(
        &mut self,
        mut rows: Vec<SignalBinding>,
        depth: u8,
        budget: GraphBudget,
        next: &mut BTreeSet<AssetId>,
    ) {
        let remaining = usize::from(budget.bindings()).saturating_sub(self.bindings.len());
        if rows.len() > remaining {
            self.truncation.insert(GraphTruncation::Bindings);
            rows.truncate(remaining);
        }
        for row in rows {
            if self.admit(
                row.source().device_asset_id(),
                row.target().asset_id(),
                depth,
                budget,
                next,
            ) {
                self.bindings.insert(row.id().clone(), row);
            }
        }
    }
    fn admit(
        &mut self,
        source: &AssetId,
        target: &AssetId,
        depth: u8,
        budget: GraphBudget,
        next: &mut BTreeSet<AssetId>,
    ) -> bool {
        let missing: BTreeSet<_> = [source, target]
            .into_iter()
            .filter(|id| !self.assets.contains(*id))
            .cloned()
            .collect();
        if missing.is_empty() {
            return true;
        }
        let depth_cut = depth == budget.depth();
        let asset_cut = self.assets.len() + missing.len() > usize::from(budget.assets());
        if depth_cut {
            self.truncation.insert(GraphTruncation::Depth);
        }
        if asset_cut {
            self.truncation.insert(GraphTruncation::Assets);
        }
        if depth_cut || asset_cut {
            return false;
        }
        next.extend(missing.iter().cloned());
        self.assets.extend(missing);
        true
    }
}
