//! Protected graph orchestration; all expansion occurs in one read session.
mod expand;
use super::{
    SemanticGraph, SemanticGraphQuery, SemanticGraphRead, SemanticGraphRepository,
    traversal::Traversal,
};
use fleetiq_access::{AuthorizedTenant, ReadMetadata};
use std::{collections::BTreeSet, error::Error, fmt};

/// Application service with metadata-only authority; never exposes telemetry values.
pub struct SemanticGraphService<R> {
    repository: R,
}
impl<R> SemanticGraphService<R> {
    /// Composes a consistent-read adapter without choosing a database.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}
impl<R: SemanticGraphRepository> SemanticGraphService<R> {
    /// Explores both directions while preserving each edge's directed meaning.
    ///
    /// # Errors
    /// Returns safe structured storage or invalid-snapshot failures. Unknown
    /// roots return absence; read failure never returns a misleading empty graph.
    pub async fn explore(
        &self,
        grant: AuthorizedTenant<ReadMetadata>,
        query: &SemanticGraphQuery,
    ) -> Result<Option<SemanticGraph>, GraphExplorationError<R::Error>> {
        let mut read = self
            .repository
            .open(grant.tenant_id(), query)
            .await
            .map_err(GraphExplorationError::Repository)?;
        let root = read
            .assets(std::slice::from_ref(query.root()))
            .await
            .map_err(GraphExplorationError::Repository)?;
        if root.is_empty() {
            return Ok(None);
        }
        if root.len() != 1
            || root[0].id() != query.root()
            || root[0].tenant_id() != grant.tenant_id()
        {
            return Err(GraphExplorationError::InvalidSnapshot);
        }
        let mut state = Traversal::new(query.root().clone());
        let mut frontier = vec![query.root().clone()];
        let budget = query.budget();
        for depth in 0..=budget.depth() {
            if frontier.is_empty() {
                break;
            }
            frontier = expand::layer(
                &mut read,
                &mut state,
                grant.tenant_id(),
                query,
                &frontier,
                depth,
            )
            .await?;
        }
        let ids = state.assets.into_iter().collect::<Vec<_>>();
        let assets = read
            .assets(&ids)
            .await
            .map_err(GraphExplorationError::Repository)?;
        let actual = assets
            .iter()
            .map(|a| a.id().clone())
            .collect::<BTreeSet<_>>();
        if assets.len() != ids.len()
            || actual != ids.into_iter().collect()
            || assets.iter().any(|a| a.tenant_id() != grant.tenant_id())
        {
            return Err(GraphExplorationError::InvalidSnapshot);
        }
        Ok(Some(SemanticGraph {
            query: query.clone(),
            assets,
            relationships: state.relationships.into_values().collect(),
            bindings: state.bindings.into_values().collect(),
            truncation: state.truncation.into_iter().collect(),
        }))
    }
}
/// Storage failure or an adapter violating the consistent bounded graph contract.
#[derive(Debug)]
pub enum GraphExplorationError<E> {
    /// Infrastructure details are retained only as an internal error cause.
    Repository(E),
    /// The snapshot returned inconsistent tenants, endpoints or cardinalities.
    InvalidSnapshot,
}
impl<E> fmt::Display for GraphExplorationError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("semantic graph exploration failed")
    }
}
impl<E: Error + 'static> Error for GraphExplorationError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(e) => Some(e),
            Self::InvalidSnapshot => None,
        }
    }
}
