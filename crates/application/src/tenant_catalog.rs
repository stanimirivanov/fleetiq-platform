use std::{error::Error, fmt, future::Future};

use fleetiq_domain::TenantId;

use crate::{AuthorizedPlatform, ManageTenantLifecycle};

const MAX_PAGE_SIZE: u16 = 100;

/// Current operational state of one stable tenant boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantLifecycleState {
    /// Fresh tenant authorization decisions may be allowed by exact grants.
    Active,
    /// Fresh tenant authorization decisions are denied while data is retained.
    Suspended,
}

/// One tenant returned by the platform catalogue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantCatalogEntry {
    tenant_id: TenantId,
    state: TenantLifecycleState,
}

impl TenantCatalogEntry {
    /// Reconstructs one catalogue entry from its stable identity and state.
    pub const fn new(tenant_id: TenantId, state: TenantLifecycleState) -> Self {
        Self { tenant_id, state }
    }

    /// Returns the stable tenant identity.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the tenant's current operational state.
    pub const fn state(&self) -> TenantLifecycleState {
        self.state
    }
}

/// Stable position after which a later tenant catalogue page is requested.
///
/// Catalogue order is ascending by tenant identity. The cursor is exclusive so
/// a tenant returned at the end of one page is not repeated on the next page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantCatalogCursor {
    tenant_id: TenantId,
}

impl TenantCatalogCursor {
    /// Creates a cursor from the final entry returned by a preceding page.
    pub const fn new(tenant_id: TenantId) -> Self {
        Self { tenant_id }
    }

    /// Returns the cursor's tenant-identity component.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }
}

/// Validated maximum number of tenant catalogue entries in one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantCatalogPageSize(u16);

impl TenantCatalogPageSize {
    /// Creates a page size in the inclusive range `1..=100`.
    ///
    /// # Errors
    ///
    /// Returns [`TenantCatalogPageSizeError`] when `value` is zero or exceeds
    /// the maximum supported page size.
    pub const fn new(value: u16) -> Result<Self, TenantCatalogPageSizeError> {
        if value == 0 {
            Err(TenantCatalogPageSizeError::Zero)
        } else if value > MAX_PAGE_SIZE {
            Err(TenantCatalogPageSizeError::TooLarge {
                maximum: MAX_PAGE_SIZE,
                actual: value,
            })
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the validated page size.
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Invalid requested tenant catalogue page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantCatalogPageSizeError {
    /// A page cannot contain zero entries.
    Zero,
    /// The requested size exceeds the bounded query limit.
    TooLarge {
        /// Maximum supported page size.
        maximum: u16,
        /// Rejected page size.
        actual: u16,
    },
}

impl fmt::Display for TenantCatalogPageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("tenant catalogue page size must be nonzero"),
            Self::TooLarge { maximum, actual } => write!(
                formatter,
                "tenant catalogue page size {actual} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for TenantCatalogPageSizeError {}

/// Bounded request for the platform tenant catalogue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantCatalogQuery {
    page_size: TenantCatalogPageSize,
    after: Option<TenantCatalogCursor>,
}

impl TenantCatalogQuery {
    /// Creates a catalogue query, optionally continuing after a preceding page.
    pub const fn new(page_size: TenantCatalogPageSize, after: Option<TenantCatalogCursor>) -> Self {
        Self { page_size, after }
    }

    /// Returns the maximum number of entries requested.
    pub const fn page_size(&self) -> TenantCatalogPageSize {
        self.page_size
    }

    /// Returns the exclusive cursor after which later identities are selected.
    pub const fn after(&self) -> Option<&TenantCatalogCursor> {
        self.after.as_ref()
    }
}

/// One bounded page of the platform tenant catalogue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantCatalogPage {
    entries: Vec<TenantCatalogEntry>,
    next: Option<TenantCatalogCursor>,
}

impl TenantCatalogPage {
    /// Constructs a page from repository-selected entries and continuation.
    pub fn new(entries: Vec<TenantCatalogEntry>, next: Option<TenantCatalogCursor>) -> Self {
        Self { entries, next }
    }

    /// Returns tenant entries in ascending stable-identity order.
    pub fn entries(&self) -> &[TenantCatalogEntry] {
        &self.entries
    }

    /// Returns the cursor for a later page when more entries exist.
    pub const fn next(&self) -> Option<&TenantCatalogCursor> {
        self.next.as_ref()
    }
}

/// Persistence boundary for bounded platform tenant catalogue queries.
pub trait TenantCatalogRepository: Send + Sync {
    /// Storage or reconstruction failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Finds one deterministic page ordered by stable tenant identity.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the page cannot be loaded or a
    /// durable tenant identity cannot be reconstructed.
    fn find_page<'a>(
        &'a self,
        query: &'a TenantCatalogQuery,
    ) -> impl Future<Output = Result<TenantCatalogPage, Self::Error>> + Send + 'a;
}

/// Coordinates authorized reads of the platform tenant catalogue.
#[derive(Clone, Debug)]
pub struct TenantCatalog<R> {
    repository: R,
}

impl<R> TenantCatalog<R> {
    /// Creates the catalogue service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> TenantCatalog<R>
where
    R: TenantCatalogRepository,
{
    /// Finds one page with exact `ManageTenantLifecycle` authorization.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the operation fails.
    pub async fn find_page(
        &self,
        _authorization: AuthorizedPlatform<ManageTenantLifecycle>,
        query: &TenantCatalogQuery,
    ) -> Result<TenantCatalogPage, TenantCatalogError<R::Error>> {
        self.repository
            .find_page(query)
            .await
            .map_err(TenantCatalogError::Repository)
    }
}

/// Failure to execute an authorized tenant catalogue query.
#[derive(Debug)]
pub enum TenantCatalogError<E> {
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for TenantCatalogError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => formatter.write_str("tenant catalogue could not be loaded"),
        }
    }
}

impl<E> Error for TenantCatalogError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests;
