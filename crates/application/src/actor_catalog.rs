use std::{error::Error, fmt, future::Future};

use fleetiq_domain::ActorId;

use crate::{AuthorizedPlatform, ManageIdentity};

const MAX_PAGE_SIZE: u16 = 100;

/// Current operational state of one stable actor boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorLifecycleState {
    /// Fresh identity resolution and authorization decisions may succeed.
    Active,
    /// Fresh identity resolution and authorization fail while data is retained.
    Deactivated,
}

/// One actor returned by the platform catalogue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorCatalogEntry {
    actor_id: ActorId,
    state: ActorLifecycleState,
}

impl ActorCatalogEntry {
    /// Reconstructs one catalogue entry from its stable identity and state.
    pub const fn new(actor_id: ActorId, state: ActorLifecycleState) -> Self {
        Self { actor_id, state }
    }

    /// Returns the stable actor identity.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }

    /// Returns the actor's current operational state.
    pub const fn state(&self) -> ActorLifecycleState {
        self.state
    }
}

/// Stable position after which a later actor catalogue page is requested.
///
/// Catalogue order is ascending by actor identity. The cursor is exclusive so
/// an actor returned at the end of one page is not repeated on the next page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorCatalogCursor {
    actor_id: ActorId,
}

impl ActorCatalogCursor {
    /// Creates a cursor from the final entry returned by a preceding page.
    pub const fn new(actor_id: ActorId) -> Self {
        Self { actor_id }
    }

    /// Returns the cursor's actor-identity component.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }
}

/// Validated maximum number of actor catalogue entries in one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActorCatalogPageSize(u16);

impl ActorCatalogPageSize {
    /// Creates a page size in the inclusive range `1..=100`.
    ///
    /// # Errors
    ///
    /// Returns [`ActorCatalogPageSizeError`] when `value` is zero or exceeds
    /// the maximum supported page size.
    pub const fn new(value: u16) -> Result<Self, ActorCatalogPageSizeError> {
        if value == 0 {
            Err(ActorCatalogPageSizeError::Zero)
        } else if value > MAX_PAGE_SIZE {
            Err(ActorCatalogPageSizeError::TooLarge {
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

/// Invalid requested actor catalogue page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorCatalogPageSizeError {
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

impl fmt::Display for ActorCatalogPageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("actor catalogue page size must be nonzero"),
            Self::TooLarge { maximum, actual } => write!(
                formatter,
                "actor catalogue page size {actual} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for ActorCatalogPageSizeError {}

/// Bounded request for the platform actor catalogue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorCatalogQuery {
    page_size: ActorCatalogPageSize,
    after: Option<ActorCatalogCursor>,
}

impl ActorCatalogQuery {
    /// Creates a catalogue query, optionally continuing after a preceding page.
    pub const fn new(page_size: ActorCatalogPageSize, after: Option<ActorCatalogCursor>) -> Self {
        Self { page_size, after }
    }

    /// Returns the maximum number of entries requested.
    pub const fn page_size(&self) -> ActorCatalogPageSize {
        self.page_size
    }

    /// Returns the exclusive cursor after which later identities are selected.
    pub const fn after(&self) -> Option<&ActorCatalogCursor> {
        self.after.as_ref()
    }
}

/// One bounded page of the platform actor catalogue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorCatalogPage {
    entries: Vec<ActorCatalogEntry>,
    next: Option<ActorCatalogCursor>,
}

impl ActorCatalogPage {
    /// Constructs a page from repository-selected entries and continuation.
    pub fn new(entries: Vec<ActorCatalogEntry>, next: Option<ActorCatalogCursor>) -> Self {
        Self { entries, next }
    }

    /// Returns actor entries in ascending stable-identity order.
    pub fn entries(&self) -> &[ActorCatalogEntry] {
        &self.entries
    }

    /// Returns the cursor for a later page when more entries exist.
    pub const fn next(&self) -> Option<&ActorCatalogCursor> {
        self.next.as_ref()
    }
}

/// Persistence boundary for bounded platform actor catalogue queries.
pub trait ActorCatalogRepository: Send + Sync {
    /// Storage or reconstruction failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Finds one deterministic page ordered by stable actor identity.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the page cannot be loaded or a
    /// durable actor identity cannot be reconstructed.
    fn find_page<'a>(
        &'a self,
        query: &'a ActorCatalogQuery,
    ) -> impl Future<Output = Result<ActorCatalogPage, Self::Error>> + Send + 'a;
}

/// Coordinates authorized reads of the platform actor catalogue.
#[derive(Clone, Debug)]
pub struct ActorCatalog<R> {
    repository: R,
}

impl<R> ActorCatalog<R> {
    /// Creates the catalogue service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> ActorCatalog<R>
where
    R: ActorCatalogRepository,
{
    /// Finds one page with exact `ManageIdentity` authorization.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the operation fails.
    pub async fn find_page(
        &self,
        _authorization: AuthorizedPlatform<ManageIdentity>,
        query: &ActorCatalogQuery,
    ) -> Result<ActorCatalogPage, ActorCatalogError<R::Error>> {
        self.repository
            .find_page(query)
            .await
            .map_err(ActorCatalogError::Repository)
    }
}

/// Failure to execute an authorized actor catalogue query.
#[derive(Debug)]
pub enum ActorCatalogError<E> {
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for ActorCatalogError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => formatter.write_str("actor catalogue could not be loaded"),
        }
    }
}

impl<E> Error for ActorCatalogError<E>
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
