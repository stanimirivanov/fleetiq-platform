//! One bounded breadth-first layer, with no per-node reads.
use super::super::{SemanticGraphQuery, SemanticGraphRead, traversal::Traversal};
use super::GraphExplorationError;
use fleetiq_domain::{AssetId, TenantId};
use std::collections::BTreeSet;
pub(super) async fn layer<S: SemanticGraphRead>(
    read: &mut S,
    state: &mut Traversal,
    tenant: &TenantId,
    query: &SemanticGraphQuery,
    frontier: &[AssetId],
    depth: u8,
) -> Result<Vec<AssetId>, GraphExplorationError<S::Error>> {
    let budget = query.budget();
    let mut next = BTreeSet::new();
    let excluded = state.relationships.keys().cloned().collect::<Vec<_>>();
    let limit = budget.relationships().saturating_sub(
        u16::try_from(excluded.len()).map_err(|_| GraphExplorationError::InvalidSnapshot)?,
    ) + 1;
    let rows = read
        .relationships(frontier, &excluded, limit)
        .await
        .map_err(GraphExplorationError::Repository)?;
    if rows.len() > usize::from(limit)
        || rows.iter().any(|r| {
            r.tenant_id() != tenant
                || r.recorded_at() > query.known_at()
                || !r.effective_interval().contains(query.effective_at())
                || !(frontier.contains(r.source_asset_id())
                    || frontier.contains(r.target_asset_id()))
        })
    {
        return Err(GraphExplorationError::InvalidSnapshot);
    }
    state.add_relationships(rows, depth, budget, &mut next);
    let excluded = state.bindings.keys().cloned().collect::<Vec<_>>();
    let limit = budget.bindings().saturating_sub(
        u16::try_from(excluded.len()).map_err(|_| GraphExplorationError::InvalidSnapshot)?,
    ) + 1;
    let rows = read
        .bindings(frontier, &excluded, limit)
        .await
        .map_err(GraphExplorationError::Repository)?;
    if rows.len() > usize::from(limit)
        || rows.iter().any(|r| {
            r.tenant_id() != tenant
                || r.recorded_at() > query.known_at()
                || !r.effective_interval().contains(query.effective_at())
                || !(frontier.contains(r.source().device_asset_id())
                    || frontier.contains(r.target().asset_id()))
        })
    {
        return Err(GraphExplorationError::InvalidSnapshot);
    }
    state.add_bindings(rows, depth, budget, &mut next);
    Ok(next.into_iter().collect())
}
