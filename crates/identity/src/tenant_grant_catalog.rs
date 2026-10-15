use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, TenantId};

use crate::pagination::{KeysetPage, PageSize, PageSizeError};
use crate::{AuthorizedTenant, ManageAccess, TenantAction};

/// One exact current tenant action grant held by a stable actor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantGrantCatalogEntry {
    action: TenantAction,
}

impl TenantGrantCatalogEntry {
    /// Reconstructs one current tenant grant.
    pub const fn new(action: TenantAction) -> Self {
        Self { action }
    }

    /// Returns the exact tenant action granted to the selected actor.
    pub const fn action(self) -> TenantAction {
        self.action
    }
}

/// Stable position after which another current tenant-grant page is requested.
///
/// Catalogue order follows the durable tenant action name. The cursor is
/// exclusive so a grant returned at the end of one page is not repeated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantGrantCatalogCursor {
    action: TenantAction,
}

impl TenantGrantCatalogCursor {
    /// Creates a cursor from the final grant returned by a preceding page.
    pub const fn new(action: TenantAction) -> Self {
        Self { action }
    }

    /// Returns the cursor's exact tenant action.
    pub const fn action(self) -> TenantAction {
        self.action
    }
}

/// Validated maximum number of current tenant grants in one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantGrantCatalogPageSize(PageSize);

impl TenantGrantCatalogPageSize {
    /// Creates a page size in the inclusive range `1..=100`.
    ///
    /// # Errors
    ///
    /// Returns [`TenantGrantCatalogPageSizeError`] when `value` is zero or
    /// exceeds the maximum supported page size.
    pub const fn new(value: u16) -> Result<Self, TenantGrantCatalogPageSizeError> {
        match PageSize::new(value) {
            Ok(size) => Ok(Self(size)),
            Err(PageSizeError::Zero) => Err(TenantGrantCatalogPageSizeError::Zero),
            Err(PageSizeError::TooLarge { maximum, actual }) => {
                Err(TenantGrantCatalogPageSizeError::TooLarge { maximum, actual })
            }
        }
    }

    /// Returns the validated page size.
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

/// Invalid requested current tenant-grant page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantGrantCatalogPageSizeError {
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

impl fmt::Display for TenantGrantCatalogPageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("tenant grant catalogue page size must be nonzero"),
            Self::TooLarge { maximum, actual } => write!(
                formatter,
                "tenant grant catalogue page size {actual} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for TenantGrantCatalogPageSizeError {}

/// Bounded request for one actor's current tenant grants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantGrantCatalogQuery {
    actor_id: ActorId,
    page_size: TenantGrantCatalogPageSize,
    after: Option<TenantGrantCatalogCursor>,
}

impl TenantGrantCatalogQuery {
    /// Creates an actor-scoped query, optionally continuing after a prior page.
    pub const fn new(
        actor_id: ActorId,
        page_size: TenantGrantCatalogPageSize,
        after: Option<TenantGrantCatalogCursor>,
    ) -> Self {
        Self {
            actor_id,
            page_size,
            after,
        }
    }

    /// Returns the stable actor whose current tenant grants are requested.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }

    /// Returns the maximum number of entries requested.
    pub const fn page_size(&self) -> TenantGrantCatalogPageSize {
        self.page_size
    }

    /// Returns the exclusive cursor after which later grants are selected.
    pub const fn after(&self) -> Option<TenantGrantCatalogCursor> {
        self.after
    }
}

/// One bounded page of an actor's current tenant grants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantGrantCatalogPage {
    page: KeysetPage<TenantGrantCatalogEntry, TenantGrantCatalogCursor>,
}

impl TenantGrantCatalogPage {
    /// Constructs a page from repository-selected entries and continuation.
    pub fn new(
        entries: Vec<TenantGrantCatalogEntry>,
        next: Option<TenantGrantCatalogCursor>,
    ) -> Self {
        Self {
            page: KeysetPage::new(entries, next),
        }
    }

    /// Returns exact grants in ascending durable action-name order.
    pub fn entries(&self) -> &[TenantGrantCatalogEntry] {
        self.page.entries()
    }

    /// Returns the cursor for a later page when more grants exist.
    pub const fn next(&self) -> Option<TenantGrantCatalogCursor> {
        self.page.next_copied()
    }
}

/// Persistence boundary for bounded current tenant-grant queries.
pub trait TenantGrantCatalogRepository: Send + Sync {
    /// Storage or reconstruction failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Finds one deterministic page scoped to the exact tenant and requested actor.
    /// Missing actors and actors without grants both produce an empty page.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the page cannot be loaded or a
    /// durable action cannot be reconstructed.
    fn find_page<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        query: &'a TenantGrantCatalogQuery,
    ) -> impl Future<Output = Result<TenantGrantCatalogPage, Self::Error>> + Send + 'a;
}

/// Coordinates authorized reads of current tenant grant state.
#[derive(Clone, Debug)]
pub struct TenantGrantCatalog<R> {
    repository: R,
}

impl<R> TenantGrantCatalog<R> {
    /// Creates the catalogue service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> TenantGrantCatalog<R>
where
    R: TenantGrantCatalogRepository,
{
    /// Finds one actor-scoped page with exact `ManageAccess` evidence.
    ///
    /// Tenant scope comes exclusively from consumed authorization evidence.
    /// Retained grants do not prove an inactive actor can act.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the operation fails.
    pub async fn find_page(
        &self,
        authorization: AuthorizedTenant<ManageAccess>,
        query: &TenantGrantCatalogQuery,
    ) -> Result<TenantGrantCatalogPage, TenantGrantCatalogError<R::Error>> {
        self.repository
            .find_page(authorization.tenant_id(), query)
            .await
            .map_err(TenantGrantCatalogError::Repository)
    }
}

/// Failure to execute an authorized current tenant-grant query.
#[derive(Debug)]
pub enum TenantGrantCatalogError<E> {
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for TenantGrantCatalogError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => {
                formatter.write_str("tenant grant catalogue could not be loaded")
            }
        }
    }
}

impl<E> Error for TenantGrantCatalogError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Repository(error) => Some(error),
        }
    }
}
