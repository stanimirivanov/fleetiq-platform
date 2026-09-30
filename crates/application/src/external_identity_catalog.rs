use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId};

use crate::{AuthorizedPlatform, ManageIdentity};

const MAX_PAGE_SIZE: u16 = 100;

/// One current external identity attached to a stable actor.
#[derive(Clone, Eq, PartialEq)]
pub struct ExternalIdentityCatalogEntry {
    external_identity: ExternalIdentity,
}

impl ExternalIdentityCatalogEntry {
    /// Reconstructs one current catalogue entry.
    pub const fn new(external_identity: ExternalIdentity) -> Self {
        Self { external_identity }
    }

    /// Returns the exact authority and opaque subject for an authorized caller.
    pub const fn external_identity(&self) -> &ExternalIdentity {
        &self.external_identity
    }
}

impl fmt::Debug for ExternalIdentityCatalogEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalIdentityCatalogEntry")
            .field("authority_id", self.external_identity.authority_id())
            .field("subject", &"<redacted>")
            .finish()
    }
}

/// Stable position after which another external-identity page is requested.
///
/// Catalogue order is ascending by `(authority_id, subject)`. The opaque
/// subject remains available for persistence but is redacted from `Debug`.
#[derive(Clone, Eq, PartialEq)]
pub struct ExternalIdentityCatalogCursor {
    authority_id: IdentityAuthorityId,
    subject: ExternalIdentitySubject,
}

impl ExternalIdentityCatalogCursor {
    /// Creates a cursor from the final entry returned by a preceding page.
    pub const fn new(authority_id: IdentityAuthorityId, subject: ExternalIdentitySubject) -> Self {
        Self {
            authority_id,
            subject,
        }
    }

    /// Returns the cursor's authority component.
    pub const fn authority_id(&self) -> &IdentityAuthorityId {
        &self.authority_id
    }

    /// Returns the cursor's exact opaque subject component.
    pub const fn subject(&self) -> &ExternalIdentitySubject {
        &self.subject
    }
}

impl fmt::Debug for ExternalIdentityCatalogCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalIdentityCatalogCursor")
            .field("authority_id", &self.authority_id)
            .field("subject", &"<redacted>")
            .finish()
    }
}

/// Validated maximum number of current external identities in one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalIdentityCatalogPageSize(u16);

impl ExternalIdentityCatalogPageSize {
    /// Creates a page size in the inclusive range `1..=100`.
    ///
    /// # Errors
    ///
    /// Returns [`ExternalIdentityCatalogPageSizeError`] when `value` is zero
    /// or exceeds the maximum supported page size.
    pub const fn new(value: u16) -> Result<Self, ExternalIdentityCatalogPageSizeError> {
        if value == 0 {
            Err(ExternalIdentityCatalogPageSizeError::Zero)
        } else if value > MAX_PAGE_SIZE {
            Err(ExternalIdentityCatalogPageSizeError::TooLarge {
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

/// Invalid requested current external-identity page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalIdentityCatalogPageSizeError {
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

impl fmt::Display for ExternalIdentityCatalogPageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => {
                formatter.write_str("external identity catalogue page size must be nonzero")
            }
            Self::TooLarge { maximum, actual } => write!(
                formatter,
                "external identity catalogue page size {actual} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for ExternalIdentityCatalogPageSizeError {}

/// Bounded request for one actor's current external identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalIdentityCatalogQuery {
    actor_id: ActorId,
    page_size: ExternalIdentityCatalogPageSize,
    after: Option<ExternalIdentityCatalogCursor>,
}

impl ExternalIdentityCatalogQuery {
    /// Creates an actor-scoped query, optionally continuing after a prior page.
    pub const fn new(
        actor_id: ActorId,
        page_size: ExternalIdentityCatalogPageSize,
        after: Option<ExternalIdentityCatalogCursor>,
    ) -> Self {
        Self {
            actor_id,
            page_size,
            after,
        }
    }

    /// Returns the stable actor whose current mappings are requested.
    pub const fn actor_id(&self) -> &ActorId {
        &self.actor_id
    }

    /// Returns the maximum number of entries requested.
    pub const fn page_size(&self) -> ExternalIdentityCatalogPageSize {
        self.page_size
    }

    /// Returns the exclusive cursor after which later identities are selected.
    pub const fn after(&self) -> Option<&ExternalIdentityCatalogCursor> {
        self.after.as_ref()
    }
}

/// One bounded page of an actor's current external identities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalIdentityCatalogPage {
    entries: Vec<ExternalIdentityCatalogEntry>,
    next: Option<ExternalIdentityCatalogCursor>,
}

impl ExternalIdentityCatalogPage {
    /// Constructs a page from repository-selected entries and continuation.
    pub fn new(
        entries: Vec<ExternalIdentityCatalogEntry>,
        next: Option<ExternalIdentityCatalogCursor>,
    ) -> Self {
        Self { entries, next }
    }

    /// Returns entries ordered by authority and exact opaque subject.
    pub fn entries(&self) -> &[ExternalIdentityCatalogEntry] {
        &self.entries
    }

    /// Returns the cursor for a later page when more mappings exist.
    pub const fn next(&self) -> Option<&ExternalIdentityCatalogCursor> {
        self.next.as_ref()
    }
}

/// Persistence boundary for bounded current external-identity queries.
pub trait ExternalIdentityCatalogRepository: Send + Sync {
    /// Storage or reconstruction failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Finds one deterministic page scoped to the requested actor.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the page cannot be loaded or a
    /// durable identity cannot be reconstructed.
    fn find_page<'a>(
        &'a self,
        query: &'a ExternalIdentityCatalogQuery,
    ) -> impl Future<Output = Result<ExternalIdentityCatalogPage, Self::Error>> + Send + 'a;
}

/// Coordinates authorized reads of current external-identity mappings.
#[derive(Clone, Debug)]
pub struct ExternalIdentityCatalog<R> {
    repository: R,
}

impl<R> ExternalIdentityCatalog<R> {
    /// Creates the catalogue service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> ExternalIdentityCatalog<R>
where
    R: ExternalIdentityCatalogRepository,
{
    /// Finds one actor-scoped page with exact `ManageIdentity` authorization.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the operation fails.
    pub async fn find_page(
        &self,
        _authorization: AuthorizedPlatform<ManageIdentity>,
        query: &ExternalIdentityCatalogQuery,
    ) -> Result<ExternalIdentityCatalogPage, ExternalIdentityCatalogError<R::Error>> {
        self.repository
            .find_page(query)
            .await
            .map_err(ExternalIdentityCatalogError::Repository)
    }
}

/// Failure to execute an authorized current external-identity query.
#[derive(Debug)]
pub enum ExternalIdentityCatalogError<E> {
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for ExternalIdentityCatalogError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => {
                formatter.write_str("external identity catalogue could not be loaded")
            }
        }
    }
}

impl<E> Error for ExternalIdentityCatalogError<E>
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
