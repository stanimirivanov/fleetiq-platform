use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, ActorLifecycleChangeId, UnixTimeMillis};

use crate::{
    ActorLifecycleChangeEffect, ActorLifecycleChangeKind, AuthorizedPlatform, ManageIdentity,
};

const MAX_PAGE_SIZE: u16 = 100;

/// One immutable actor lifecycle administration audit record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorLifecycleAuditEntry {
    change_id: ActorLifecycleChangeId,
    administrator_actor_id: ActorId,
    target_actor_id: ActorId,
    kind: ActorLifecycleChangeKind,
    recorded_at: UnixTimeMillis,
    effect: ActorLifecycleChangeEffect,
}

impl ActorLifecycleAuditEntry {
    /// Reconstructs one complete durable actor lifecycle audit record.
    pub const fn new(
        change_id: ActorLifecycleChangeId,
        administrator_actor_id: ActorId,
        target_actor_id: ActorId,
        kind: ActorLifecycleChangeKind,
        recorded_at: UnixTimeMillis,
        effect: ActorLifecycleChangeEffect,
    ) -> Self {
        Self {
            change_id,
            administrator_actor_id,
            target_actor_id,
            kind,
            recorded_at,
            effect,
        }
    }

    /// Returns the platform-scoped administration command identity.
    pub const fn change_id(&self) -> &ActorLifecycleChangeId {
        &self.change_id
    }

    /// Returns the actor that authorized the command.
    pub const fn administrator_actor_id(&self) -> &ActorId {
        &self.administrator_actor_id
    }

    /// Returns the actor whose lifecycle state was addressed.
    pub const fn target_actor_id(&self) -> &ActorId {
        &self.target_actor_id
    }

    /// Returns whether the command activated or deactivated the actor.
    pub const fn kind(&self) -> ActorLifecycleChangeKind {
        self.kind
    }

    /// Returns the trusted host time at which the command was recorded.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }

    /// Returns whether the command changed current actor state.
    pub const fn effect(&self) -> ActorLifecycleChangeEffect {
        self.effect
    }
}

/// Stable position after which an older actor lifecycle audit page is requested.
///
/// Audit order is descending by `(recorded_at, change_id)`. The change identity
/// makes the cursor deterministic when commands share one recorded time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorLifecycleAuditCursor {
    recorded_at: UnixTimeMillis,
    change_id: ActorLifecycleChangeId,
}

impl ActorLifecycleAuditCursor {
    /// Creates a cursor from the last entry returned by a preceding page.
    pub const fn new(recorded_at: UnixTimeMillis, change_id: ActorLifecycleChangeId) -> Self {
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
    pub const fn change_id(&self) -> &ActorLifecycleChangeId {
        &self.change_id
    }
}

/// Validated maximum number of actor lifecycle audit entries in one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActorLifecycleAuditPageSize(u16);

impl ActorLifecycleAuditPageSize {
    /// Creates a page size in the inclusive range `1..=100`.
    ///
    /// # Errors
    ///
    /// Returns [`ActorLifecycleAuditPageSizeError`] when `value` is zero or
    /// exceeds the maximum supported page size.
    pub const fn new(value: u16) -> Result<Self, ActorLifecycleAuditPageSizeError> {
        if value == 0 {
            Err(ActorLifecycleAuditPageSizeError::Zero)
        } else if value > MAX_PAGE_SIZE {
            Err(ActorLifecycleAuditPageSizeError::TooLarge {
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

/// Invalid requested actor lifecycle audit page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActorLifecycleAuditPageSizeError {
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

impl fmt::Display for ActorLifecycleAuditPageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("actor lifecycle audit page size must be nonzero"),
            Self::TooLarge { maximum, actual } => write!(
                formatter,
                "actor lifecycle audit page size {actual} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for ActorLifecycleAuditPageSizeError {}

/// Bounded request for actor lifecycle audit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorLifecycleAuditQuery {
    page_size: ActorLifecycleAuditPageSize,
    after: Option<ActorLifecycleAuditCursor>,
}

impl ActorLifecycleAuditQuery {
    /// Creates an audit query, optionally continuing after a preceding page.
    pub const fn new(
        page_size: ActorLifecycleAuditPageSize,
        after: Option<ActorLifecycleAuditCursor>,
    ) -> Self {
        Self { page_size, after }
    }

    /// Returns the maximum number of entries requested.
    pub const fn page_size(&self) -> ActorLifecycleAuditPageSize {
        self.page_size
    }

    /// Returns the exclusive cursor after which older entries are selected.
    pub const fn after(&self) -> Option<&ActorLifecycleAuditCursor> {
        self.after.as_ref()
    }
}

/// One bounded page of actor lifecycle audit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActorLifecycleAuditPage {
    entries: Vec<ActorLifecycleAuditEntry>,
    next: Option<ActorLifecycleAuditCursor>,
}

impl ActorLifecycleAuditPage {
    /// Constructs a page from repository-selected entries and continuation.
    pub fn new(
        entries: Vec<ActorLifecycleAuditEntry>,
        next: Option<ActorLifecycleAuditCursor>,
    ) -> Self {
        Self { entries, next }
    }

    /// Returns newest-first audit entries in this page.
    pub fn entries(&self) -> &[ActorLifecycleAuditEntry] {
        &self.entries
    }

    /// Returns the cursor for an older page when more entries exist.
    pub const fn next(&self) -> Option<&ActorLifecycleAuditCursor> {
        self.next.as_ref()
    }
}

/// Persistence boundary for bounded actor lifecycle audit queries.
pub trait ActorLifecycleAuditRepository: Send + Sync {
    /// Storage or reconstruction failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Finds one deterministic newest-first page of global lifecycle history.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the page cannot be loaded or a
    /// durable record cannot be reconstructed.
    fn find_page<'a>(
        &'a self,
        query: &'a ActorLifecycleAuditQuery,
    ) -> impl Future<Output = Result<ActorLifecycleAuditPage, Self::Error>> + Send + 'a;
}

/// Coordinates authorized reads of actor lifecycle audit history.
#[derive(Clone, Debug)]
pub struct ActorLifecycleAudit<R> {
    repository: R,
}

impl<R> ActorLifecycleAudit<R> {
    /// Creates the audit service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> ActorLifecycleAudit<R>
where
    R: ActorLifecycleAuditRepository,
{
    /// Finds one audit page with exact `ManageIdentity` authorization.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the operation fails.
    pub async fn find_page(
        &self,
        _authorization: AuthorizedPlatform<ManageIdentity>,
        query: &ActorLifecycleAuditQuery,
    ) -> Result<ActorLifecycleAuditPage, ActorLifecycleAuditError<R::Error>> {
        self.repository
            .find_page(query)
            .await
            .map_err(ActorLifecycleAuditError::Repository)
    }
}

/// Failure to execute an authorized actor lifecycle audit query.
#[derive(Debug)]
pub enum ActorLifecycleAuditError<E> {
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for ActorLifecycleAuditError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => formatter.write_str("actor lifecycle audit could not be loaded"),
        }
    }
}

impl<E> Error for ActorLifecycleAuditError<E>
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
