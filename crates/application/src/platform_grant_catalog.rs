use std::{error::Error, fmt, future::Future};

use fleetiq_domain::ActorId;

use crate::{AuthorizedPlatformAction, PlatformAction};

const MAX_PAGE_SIZE: u16 = 100;

/// One exact current platform action grant held by a stable actor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformGrantCatalogEntry {
    action: PlatformAction,
}

impl PlatformGrantCatalogEntry {
    /// Reconstructs one current platform grant.
    pub const fn new(action: PlatformAction) -> Self {
        Self { action }
    }

    /// Returns the exact platform action granted to the selected actor.
    pub const fn action(self) -> PlatformAction {
        self.action
    }
}

/// Stable position after which another current platform-grant page is requested.
///
/// Catalogue order follows the durable platform action name. The cursor is
/// exclusive so a grant returned at the end of one page is not repeated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformGrantCatalogCursor {
    action: PlatformAction,
}

impl PlatformGrantCatalogCursor {
    /// Creates a cursor from the final grant returned by a preceding page.
    pub const fn new(action: PlatformAction) -> Self {
        Self { action }
    }

    /// Returns the cursor's exact platform action.
    pub const fn action(self) -> PlatformAction {
        self.action
    }
}

/// Validated maximum number of current platform grants in one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformGrantCatalogPageSize(u16);

impl PlatformGrantCatalogPageSize {
    /// Creates a page size in the inclusive range `1..=100`.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformGrantCatalogPageSizeError`] when `value` is zero or
    /// exceeds the maximum supported page size.
    pub const fn new(value: u16) -> Result<Self, PlatformGrantCatalogPageSizeError> {
        if value == 0 {
            Err(PlatformGrantCatalogPageSizeError::Zero)
        } else if value > MAX_PAGE_SIZE {
            Err(PlatformGrantCatalogPageSizeError::TooLarge {
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

/// Invalid requested current platform-grant page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformGrantCatalogPageSizeError {
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

impl fmt::Display for PlatformGrantCatalogPageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("platform grant catalogue page size must be nonzero"),
            Self::TooLarge { maximum, actual } => write!(
                formatter,
                "platform grant catalogue page size {actual} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for PlatformGrantCatalogPageSizeError {}

/// Bounded request for one actor's current platform grants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformGrantCatalogQuery {
    actor_id: ActorId,
    page_size: PlatformGrantCatalogPageSize,
    after: Option<PlatformGrantCatalogCursor>,
}

impl PlatformGrantCatalogQuery {
    /// Creates an actor-scoped query, optionally continuing after a prior page.
    pub const fn new(
        actor_id: ActorId,
        page_size: PlatformGrantCatalogPageSize,
        after: Option<PlatformGrantCatalogCursor>,
    ) -> Self {
        Self {
            actor_id,
            page_size,
            after,
        }
    }

    /// Returns the stable actor whose current platform grants are requested.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }

    /// Returns the maximum number of entries requested.
    pub const fn page_size(&self) -> PlatformGrantCatalogPageSize {
        self.page_size
    }

    /// Returns the exclusive cursor after which later grants are selected.
    pub const fn after(&self) -> Option<PlatformGrantCatalogCursor> {
        self.after
    }
}

/// One bounded page of an actor's current platform grants.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformGrantCatalogPage {
    entries: Vec<PlatformGrantCatalogEntry>,
    next: Option<PlatformGrantCatalogCursor>,
}

impl PlatformGrantCatalogPage {
    /// Constructs a page from repository-selected entries and continuation.
    pub fn new(
        entries: Vec<PlatformGrantCatalogEntry>,
        next: Option<PlatformGrantCatalogCursor>,
    ) -> Self {
        Self { entries, next }
    }

    /// Returns exact grants in ascending durable action-name order.
    pub fn entries(&self) -> &[PlatformGrantCatalogEntry] {
        &self.entries
    }

    /// Returns the cursor for a later page when more grants exist.
    pub const fn next(&self) -> Option<PlatformGrantCatalogCursor> {
        self.next
    }
}

/// Persistence boundary for bounded current platform-grant queries.
pub trait PlatformGrantCatalogRepository: Send + Sync {
    /// Storage or reconstruction failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Finds one deterministic page scoped to the requested actor.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the page cannot be loaded or a
    /// durable action cannot be reconstructed.
    fn find_page<'a>(
        &'a self,
        query: &'a PlatformGrantCatalogQuery,
    ) -> impl Future<Output = Result<PlatformGrantCatalogPage, Self::Error>> + Send + 'a;
}

/// Coordinates authorized reads of current platform grant state.
#[derive(Clone, Debug)]
pub struct PlatformGrantCatalog<R> {
    repository: R,
}

impl<R> PlatformGrantCatalog<R> {
    /// Creates the catalogue service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> PlatformGrantCatalog<R>
where
    R: PlatformGrantCatalogRepository,
{
    /// Finds one actor-scoped page with exact `ManagePlatformAccess` evidence.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformGrantCatalogError::WrongAction`] when the evidence
    /// authorizes another action, or wraps the repository error when loading
    /// fails.
    pub async fn find_page(
        &self,
        authorization: AuthorizedPlatformAction,
        query: &PlatformGrantCatalogQuery,
    ) -> Result<PlatformGrantCatalogPage, PlatformGrantCatalogError<R::Error>> {
        if authorization.action() != PlatformAction::ManagePlatformAccess {
            return Err(PlatformGrantCatalogError::WrongAction {
                actual: authorization.action(),
            });
        }

        self.repository
            .find_page(query)
            .await
            .map_err(PlatformGrantCatalogError::Repository)
    }
}

/// Failure to execute an authorized current platform-grant query.
#[derive(Debug)]
pub enum PlatformGrantCatalogError<E> {
    /// Authorization evidence permits another platform operation.
    WrongAction {
        /// Action carried by the rejected evidence.
        actual: PlatformAction,
    },
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for PlatformGrantCatalogError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongAction { actual } => write!(
                formatter,
                "platform grant catalogue requires ManagePlatformAccess authorization, got {actual:?}"
            ),
            Self::Repository(_) => {
                formatter.write_str("platform grant catalogue could not be loaded")
            }
        }
    }
}

impl<E> Error for PlatformGrantCatalogError<E>
where
    E: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::WrongAction { .. } => None,
            Self::Repository(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests;
