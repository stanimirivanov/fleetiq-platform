use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, TenantId, TenantLifecycleChangeId, UnixTimeMillis};

use crate::pagination::{KeysetPage, PageSize, PageSizeError};
use crate::{
    AuthorizedPlatform, ManageTenantLifecycle, TenantLifecycleChangeEffect,
    TenantLifecycleChangeKind,
};

/// One immutable tenant lifecycle administration audit record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantLifecycleAuditEntry {
    change_id: TenantLifecycleChangeId,
    administrator_actor_id: ActorId,
    tenant_id: TenantId,
    kind: TenantLifecycleChangeKind,
    recorded_at: UnixTimeMillis,
    effect: TenantLifecycleChangeEffect,
}

impl TenantLifecycleAuditEntry {
    /// Reconstructs one complete durable tenant lifecycle audit record.
    pub const fn new(
        change_id: TenantLifecycleChangeId,
        administrator_actor_id: ActorId,
        tenant_id: TenantId,
        kind: TenantLifecycleChangeKind,
        recorded_at: UnixTimeMillis,
        effect: TenantLifecycleChangeEffect,
    ) -> Self {
        Self {
            change_id,
            administrator_actor_id,
            tenant_id,
            kind,
            recorded_at,
            effect,
        }
    }

    /// Returns the platform-scoped administration command identity.
    pub const fn change_id(&self) -> &TenantLifecycleChangeId {
        &self.change_id
    }

    /// Returns the actor that authorized the command.
    pub const fn administrator_actor_id(&self) -> &ActorId {
        &self.administrator_actor_id
    }

    /// Returns the tenant whose lifecycle state was addressed.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns whether the command activated or suspended the tenant.
    pub const fn kind(&self) -> TenantLifecycleChangeKind {
        self.kind
    }

    /// Returns the trusted host time at which the command was recorded.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }

    /// Returns whether the command changed current tenant state.
    pub const fn effect(&self) -> TenantLifecycleChangeEffect {
        self.effect
    }
}

/// Stable position after which an older tenant lifecycle audit page is requested.
///
/// Audit order is descending by `(recorded_at, change_id)`. The change identity
/// makes the cursor deterministic when commands share one recorded time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantLifecycleAuditCursor {
    recorded_at: UnixTimeMillis,
    change_id: TenantLifecycleChangeId,
}

impl TenantLifecycleAuditCursor {
    /// Creates a cursor from the last entry returned by a preceding page.
    pub const fn new(recorded_at: UnixTimeMillis, change_id: TenantLifecycleChangeId) -> Self {
        Self {
            recorded_at,
            change_id,
        }
    }

    /// Returns the cursor's recorded-time component.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }

    /// Returns the cursor's change-identity component.
    pub const fn change_id(&self) -> &TenantLifecycleChangeId {
        &self.change_id
    }
}

/// Validated maximum number of tenant lifecycle audit entries in one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantLifecycleAuditPageSize(PageSize);

impl TenantLifecycleAuditPageSize {
    /// Creates a page size in the inclusive range `1..=100`.
    ///
    /// # Errors
    ///
    /// Returns [`TenantLifecycleAuditPageSizeError`] when `value` is zero or
    /// exceeds the maximum supported page size.
    pub const fn new(value: u16) -> Result<Self, TenantLifecycleAuditPageSizeError> {
        match PageSize::new(value) {
            Ok(size) => Ok(Self(size)),
            Err(PageSizeError::Zero) => Err(TenantLifecycleAuditPageSizeError::Zero),
            Err(PageSizeError::TooLarge { maximum, actual }) => {
                Err(TenantLifecycleAuditPageSizeError::TooLarge { maximum, actual })
            }
        }
    }

    /// Returns the validated page size.
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

/// Invalid requested tenant lifecycle audit page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantLifecycleAuditPageSizeError {
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

impl fmt::Display for TenantLifecycleAuditPageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("tenant lifecycle audit page size must be nonzero"),
            Self::TooLarge { maximum, actual } => write!(
                formatter,
                "tenant lifecycle audit page size {actual} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for TenantLifecycleAuditPageSizeError {}

/// Bounded request for tenant lifecycle audit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantLifecycleAuditQuery {
    page_size: TenantLifecycleAuditPageSize,
    after: Option<TenantLifecycleAuditCursor>,
}

impl TenantLifecycleAuditQuery {
    /// Creates an audit query, optionally continuing after a preceding page.
    pub const fn new(
        page_size: TenantLifecycleAuditPageSize,
        after: Option<TenantLifecycleAuditCursor>,
    ) -> Self {
        Self { page_size, after }
    }

    /// Returns the maximum number of entries requested.
    pub const fn page_size(&self) -> TenantLifecycleAuditPageSize {
        self.page_size
    }

    /// Returns the exclusive cursor after which older entries are selected.
    pub const fn after(&self) -> Option<&TenantLifecycleAuditCursor> {
        self.after.as_ref()
    }
}

/// One bounded page of tenant lifecycle audit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantLifecycleAuditPage {
    page: KeysetPage<TenantLifecycleAuditEntry, TenantLifecycleAuditCursor>,
}

impl TenantLifecycleAuditPage {
    /// Constructs a page from repository-selected entries and continuation.
    pub fn new(
        entries: Vec<TenantLifecycleAuditEntry>,
        next: Option<TenantLifecycleAuditCursor>,
    ) -> Self {
        Self {
            page: KeysetPage::new(entries, next),
        }
    }

    /// Returns newest-first audit entries in this page.
    pub fn entries(&self) -> &[TenantLifecycleAuditEntry] {
        self.page.entries()
    }

    /// Returns the cursor for an older page when more entries exist.
    pub const fn next(&self) -> Option<&TenantLifecycleAuditCursor> {
        self.page.next()
    }
}

/// Persistence boundary for bounded tenant lifecycle audit queries.
pub trait TenantLifecycleAuditRepository: Send + Sync {
    /// Storage or reconstruction failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Finds one deterministic newest-first page of platform-wide history.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the page cannot be loaded or a
    /// durable record cannot be reconstructed.
    fn find_page<'a>(
        &'a self,
        query: &'a TenantLifecycleAuditQuery,
    ) -> impl Future<Output = Result<TenantLifecycleAuditPage, Self::Error>> + Send + 'a;
}

/// Coordinates authorized reads of tenant lifecycle audit history.
#[derive(Clone, Debug)]
pub struct TenantLifecycleAudit<R> {
    repository: R,
}

impl<R> TenantLifecycleAudit<R> {
    /// Creates the audit service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> TenantLifecycleAudit<R>
where
    R: TenantLifecycleAuditRepository,
{
    /// Finds one audit page with exact `ManageTenantLifecycle` authorization.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the operation fails.
    pub async fn find_page(
        &self,
        _authorization: AuthorizedPlatform<ManageTenantLifecycle>,
        query: &TenantLifecycleAuditQuery,
    ) -> Result<TenantLifecycleAuditPage, TenantLifecycleAuditError<R::Error>> {
        self.repository
            .find_page(query)
            .await
            .map_err(TenantLifecycleAuditError::Repository)
    }
}

/// Failure to execute an authorized tenant lifecycle audit query.
#[derive(Debug)]
pub enum TenantLifecycleAuditError<E> {
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for TenantLifecycleAuditError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => {
                formatter.write_str("tenant lifecycle audit could not be loaded")
            }
        }
    }
}

impl<E> Error for TenantLifecycleAuditError<E>
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
