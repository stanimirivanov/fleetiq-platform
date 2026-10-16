//! Tenant-scoped version discovery; exact reads retain complete aggregate meaning.
use crate::{
    AuthorizedTenant, DefinitionIdentity, DefinitionKind, DefinitionPosition, ReadMetadata,
};
use fleetiq_domain::{DefinitionName, TenantId};
use std::{error::Error, fmt, future::Future};

/// A small reference/name summary; complete content belongs to the exact GET.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefinitionSummary {
    position: DefinitionPosition,
    name: DefinitionName,
}
impl DefinitionSummary {
    /// Reconstructs a summary from validated persisted values.
    pub const fn new(position: DefinitionPosition, name: DefinitionName) -> Self {
        Self { position, name }
    }
    /// Returns the exact immutable version.
    pub const fn position(&self) -> &DefinitionPosition {
        &self.position
    }
    /// Returns its operator-facing name.
    pub const fn name(&self) -> &DefinitionName {
        &self.name
    }
}

/// Bounded ascending (ID, numeric version) traversal, optionally within one ID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefinitionCatalogueQuery {
    kind: DefinitionKind,
    limit: u8,
    identity: Option<DefinitionIdentity>,
    after: Option<DefinitionPosition>,
}
impl DefinitionCatalogueQuery {
    /// Validates page size, cursor namespace and optional exact-ID scope.
    ///
    /// # Errors
    ///
    /// Rejects limits outside 1..=100, namespace mismatches or a cursor naming
    /// another identity in a single-definition version catalogue.
    pub fn new(
        kind: DefinitionKind,
        limit: u8,
        identity: Option<DefinitionIdentity>,
        after: Option<DefinitionPosition>,
    ) -> Result<Self, InvalidDefinitionCatalogueQuery> {
        if !(1..=100).contains(&limit)
            || identity.as_ref().is_some_and(|id| id.kind() != kind)
            || after.as_ref().is_some_and(|p| {
                p.identity().kind() != kind
                    || identity.as_ref().is_some_and(|id| id != p.identity())
            })
        {
            return Err(InvalidDefinitionCatalogueQuery);
        }
        Ok(Self {
            kind,
            limit,
            identity,
            after,
        })
    }
    /// Returns the selected namespace.
    pub const fn kind(&self) -> DefinitionKind {
        self.kind
    }
    /// Returns the validated page bound.
    pub const fn limit(&self) -> u8 {
        self.limit
    }
    /// Returns the optional exact-ID filter.
    pub const fn identity(&self) -> Option<&DefinitionIdentity> {
        self.identity.as_ref()
    }
    /// Returns the exclusive value cursor.
    pub const fn after(&self) -> Option<&DefinitionPosition> {
        self.after.as_ref()
    }
}

/// Invalid limit or scope in a definition catalogue request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidDefinitionCatalogueQuery;
impl fmt::Display for InvalidDefinitionCatalogueQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid definition catalogue limit or cursor scope")
    }
}
impl Error for InvalidDefinitionCatalogueQuery {}

/// One bounded catalogue page; continuation is not a cross-request snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefinitionCataloguePage {
    entries: Vec<DefinitionSummary>,
    next: Option<DefinitionPosition>,
}
impl DefinitionCataloguePage {
    /// Assembles a repository-selected page and exclusive continuation.
    pub fn new(entries: Vec<DefinitionSummary>, next: Option<DefinitionPosition>) -> Self {
        Self { entries, next }
    }
    /// Returns summaries in ascending ID and numeric-version order.
    pub fn entries(&self) -> &[DefinitionSummary] {
        &self.entries
    }
    /// Returns the continuation when lookahead found more rows.
    pub const fn next(&self) -> Option<&DefinitionPosition> {
        self.next.as_ref()
    }
}

/// Consumer-owned persistence boundary for bounded semantic discovery.
pub trait DefinitionCatalogueRepository: Send + Sync {
    /// Query or domain reconstruction failure.
    type Error: Error + Send + Sync + 'static;
    /// Reads one namespace within exactly one tenant.
    ///
    /// Missing identities return empty pages. Fetch at most limit+1 summaries;
    /// do not load complete child aggregates or issue a query per summary.
    ///
    /// # Errors
    ///
    /// Returns storage failure when summaries cannot be reconstructed.
    fn list_page<'a>(
        &'a self,
        tenant: &'a TenantId,
        query: &'a DefinitionCatalogueQuery,
    ) -> impl Future<Output = Result<DefinitionCataloguePage, Self::Error>> + Send + 'a;
}

/// Coordinates catalogue reads with exact metadata-read authority.
#[derive(Clone, Debug)]
pub struct DefinitionCatalogueService<R> {
    repository: R,
}
impl<R> DefinitionCatalogueService<R> {
    /// Selects the outward query adapter.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}
impl<R: DefinitionCatalogueRepository> DefinitionCatalogueService<R> {
    /// Derives tenant ownership solely from the consumed grant.
    ///
    /// # Errors
    ///
    /// Propagates repository failure without inventing an empty success page.
    pub async fn list_page(
        &self,
        authorization: AuthorizedTenant<ReadMetadata>,
        query: &DefinitionCatalogueQuery,
    ) -> Result<DefinitionCataloguePage, R::Error> {
        self.repository
            .list_page(authorization.tenant_id(), query)
            .await
    }
}

#[cfg(test)]
mod tests;
