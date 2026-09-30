use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, PlatformAccessChangeId, UnixTimeMillis};

use crate::{
    AuthorizedPlatform, ManagePlatformAccess, PlatformAccessChangeEffect, PlatformAccessChangeKind,
    PlatformAction,
};

const MAX_PAGE_SIZE: u16 = 100;

/// One immutable platform access administration audit record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformAccessAuditEntry {
    change_id: PlatformAccessChangeId,
    administrator_actor_id: ActorId,
    target_actor_id: ActorId,
    action: PlatformAction,
    kind: PlatformAccessChangeKind,
    recorded_at: UnixTimeMillis,
    effect: PlatformAccessChangeEffect,
}

impl PlatformAccessAuditEntry {
    /// Reconstructs one complete durable platform audit record.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        change_id: PlatformAccessChangeId,
        administrator_actor_id: ActorId,
        target_actor_id: ActorId,
        action: PlatformAction,
        kind: PlatformAccessChangeKind,
        recorded_at: UnixTimeMillis,
        effect: PlatformAccessChangeEffect,
    ) -> Self {
        Self {
            change_id,
            administrator_actor_id,
            target_actor_id,
            action,
            kind,
            recorded_at,
            effect,
        }
    }

    /// Returns the platform-scoped administration command identity.
    pub const fn change_id(&self) -> &PlatformAccessChangeId {
        &self.change_id
    }

    /// Returns the actor that authorized the command.
    pub const fn administrator_actor_id(&self) -> &ActorId {
        &self.administrator_actor_id
    }

    /// Returns the actor whose exact platform grant was addressed.
    pub const fn target_actor_id(&self) -> &ActorId {
        &self.target_actor_id
    }

    /// Returns the exact platform action addressed by the command.
    pub const fn action(&self) -> PlatformAction {
        self.action
    }

    /// Returns whether the command granted or revoked the action.
    pub const fn kind(&self) -> PlatformAccessChangeKind {
        self.kind
    }

    /// Returns the trusted host time at which the command was recorded.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }

    /// Returns whether the command changed current grant state.
    pub const fn effect(&self) -> PlatformAccessChangeEffect {
        self.effect
    }
}

/// Stable position after which an older platform audit page is requested.
///
/// Audit order is descending by `(recorded_at, change_id)`. The identity
/// component makes the cursor deterministic when commands share recorded time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformAccessAuditCursor {
    recorded_at: UnixTimeMillis,
    change_id: PlatformAccessChangeId,
}

impl PlatformAccessAuditCursor {
    /// Creates a cursor from the last entry returned by a preceding page.
    pub const fn new(recorded_at: UnixTimeMillis, change_id: PlatformAccessChangeId) -> Self {
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
    pub const fn change_id(&self) -> &PlatformAccessChangeId {
        &self.change_id
    }
}

/// Validated maximum number of platform audit entries returned in one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformAccessAuditPageSize(u16);

impl PlatformAccessAuditPageSize {
    /// Creates a page size in the inclusive range `1..=100`.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformAccessAuditPageSizeError`] when `value` is zero or
    /// exceeds the maximum supported page size.
    pub const fn new(value: u16) -> Result<Self, PlatformAccessAuditPageSizeError> {
        if value == 0 {
            Err(PlatformAccessAuditPageSizeError::Zero)
        } else if value > MAX_PAGE_SIZE {
            Err(PlatformAccessAuditPageSizeError::TooLarge {
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

/// Invalid requested platform access audit page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformAccessAuditPageSizeError {
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

impl fmt::Display for PlatformAccessAuditPageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("platform access audit page size must be nonzero"),
            Self::TooLarge { maximum, actual } => write!(
                formatter,
                "platform access audit page size {actual} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for PlatformAccessAuditPageSizeError {}

/// Bounded request for platform access audit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformAccessAuditQuery {
    page_size: PlatformAccessAuditPageSize,
    after: Option<PlatformAccessAuditCursor>,
}

impl PlatformAccessAuditQuery {
    /// Creates an audit query, optionally continuing after a preceding page.
    pub const fn new(
        page_size: PlatformAccessAuditPageSize,
        after: Option<PlatformAccessAuditCursor>,
    ) -> Self {
        Self { page_size, after }
    }

    /// Returns the maximum number of entries requested.
    pub const fn page_size(&self) -> PlatformAccessAuditPageSize {
        self.page_size
    }

    /// Returns the exclusive cursor after which older entries are selected.
    pub const fn after(&self) -> Option<&PlatformAccessAuditCursor> {
        self.after.as_ref()
    }
}

/// One bounded page of platform access audit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformAccessAuditPage {
    entries: Vec<PlatformAccessAuditEntry>,
    next: Option<PlatformAccessAuditCursor>,
}

impl PlatformAccessAuditPage {
    /// Constructs a page from repository-selected entries and continuation.
    pub fn new(
        entries: Vec<PlatformAccessAuditEntry>,
        next: Option<PlatformAccessAuditCursor>,
    ) -> Self {
        Self { entries, next }
    }

    /// Returns newest-first audit entries in this page.
    pub fn entries(&self) -> &[PlatformAccessAuditEntry] {
        &self.entries
    }

    /// Returns the cursor for an older page when more entries exist.
    pub const fn next(&self) -> Option<&PlatformAccessAuditCursor> {
        self.next.as_ref()
    }
}

/// Persistence boundary for bounded platform access audit queries.
pub trait PlatformAccessAuditRepository: Send + Sync {
    /// Storage or reconstruction failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Finds one deterministic newest-first page of the global audit history.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the page cannot be loaded or a
    /// durable record cannot be reconstructed.
    fn find_page<'a>(
        &'a self,
        query: &'a PlatformAccessAuditQuery,
    ) -> impl Future<Output = Result<PlatformAccessAuditPage, Self::Error>> + Send + 'a;
}

/// Coordinates authorized reads of platform access audit history.
#[derive(Clone, Debug)]
pub struct PlatformAccessAudit<R> {
    repository: R,
}

impl<R> PlatformAccessAudit<R> {
    /// Creates the audit service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> PlatformAccessAudit<R>
where
    R: PlatformAccessAuditRepository,
{
    /// Finds one audit page with exact `ManagePlatformAccess` authorization.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the operation fails.
    pub async fn find_page(
        &self,
        _authorization: AuthorizedPlatform<ManagePlatformAccess>,
        query: &PlatformAccessAuditQuery,
    ) -> Result<PlatformAccessAuditPage, PlatformAccessAuditError<R::Error>> {
        self.repository
            .find_page(query)
            .await
            .map_err(PlatformAccessAuditError::Repository)
    }
}

/// Failure to execute an authorized platform access audit query.
#[derive(Debug)]
pub enum PlatformAccessAuditError<E> {
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for PlatformAccessAuditError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => formatter.write_str("platform access audit could not be loaded"),
        }
    }
}

impl<E> Error for PlatformAccessAuditError<E>
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
