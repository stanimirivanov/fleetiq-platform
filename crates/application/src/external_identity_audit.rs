use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, ExternalIdentity, ExternalIdentityChangeId, UnixTimeMillis};

use crate::{
    AuthorizedPlatformAction, ExternalIdentityChangeEffect, ExternalIdentityChangeKind,
    PlatformAction,
};

const MAX_PAGE_SIZE: u16 = 100;

/// One immutable external-identity administration audit record.
#[derive(Clone, Eq, PartialEq)]
pub struct ExternalIdentityAuditEntry {
    change_id: ExternalIdentityChangeId,
    administrator_actor_id: ActorId,
    target_actor_id: ActorId,
    external_identity: ExternalIdentity,
    kind: ExternalIdentityChangeKind,
    recorded_at: UnixTimeMillis,
    effect: ExternalIdentityChangeEffect,
}

impl ExternalIdentityAuditEntry {
    /// Reconstructs one complete durable external-identity audit record.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        change_id: ExternalIdentityChangeId,
        administrator_actor_id: ActorId,
        target_actor_id: ActorId,
        external_identity: ExternalIdentity,
        kind: ExternalIdentityChangeKind,
        recorded_at: UnixTimeMillis,
        effect: ExternalIdentityChangeEffect,
    ) -> Self {
        Self {
            change_id,
            administrator_actor_id,
            target_actor_id,
            external_identity,
            kind,
            recorded_at,
            effect,
        }
    }

    /// Returns the platform-scoped administration command identity.
    pub const fn change_id(&self) -> &ExternalIdentityChangeId {
        &self.change_id
    }

    /// Returns the actor that authorized the command.
    pub const fn administrator_actor_id(&self) -> &ActorId {
        &self.administrator_actor_id
    }

    /// Returns the actor whose exact identity mapping was addressed.
    pub const fn target_actor_id(&self) -> &ActorId {
        &self.target_actor_id
    }

    /// Returns the exact authority and opaque subject addressed by the command.
    pub const fn external_identity(&self) -> &ExternalIdentity {
        &self.external_identity
    }

    /// Returns whether the command attached or detached the identity.
    pub const fn kind(&self) -> ExternalIdentityChangeKind {
        self.kind
    }

    /// Returns the trusted host time at which the command was recorded.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }

    /// Returns whether the command changed current mapping state.
    pub const fn effect(&self) -> ExternalIdentityChangeEffect {
        self.effect
    }
}

impl fmt::Debug for ExternalIdentityAuditEntry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalIdentityAuditEntry")
            .field("change_id", &self.change_id)
            .field("administrator_actor_id", &self.administrator_actor_id)
            .field("target_actor_id", &self.target_actor_id)
            .field("authority_id", self.external_identity.authority_id())
            .field("subject", &"<redacted>")
            .field("kind", &self.kind)
            .field("recorded_at", &self.recorded_at)
            .field("effect", &self.effect)
            .finish()
    }
}

/// Stable position after which an older external-identity audit page is requested.
///
/// Audit order is descending by `(recorded_at, change_id)`. The identity
/// component makes the cursor deterministic when commands share recorded time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalIdentityAuditCursor {
    recorded_at: UnixTimeMillis,
    change_id: ExternalIdentityChangeId,
}

impl ExternalIdentityAuditCursor {
    /// Creates a cursor from the last entry returned by a preceding page.
    pub const fn new(recorded_at: UnixTimeMillis, change_id: ExternalIdentityChangeId) -> Self {
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
    pub const fn change_id(&self) -> &ExternalIdentityChangeId {
        &self.change_id
    }
}

/// Validated maximum number of external-identity audit entries in one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalIdentityAuditPageSize(u16);

impl ExternalIdentityAuditPageSize {
    /// Creates a page size in the inclusive range `1..=100`.
    ///
    /// # Errors
    ///
    /// Returns [`ExternalIdentityAuditPageSizeError`] when `value` is zero or
    /// exceeds the maximum supported page size.
    pub const fn new(value: u16) -> Result<Self, ExternalIdentityAuditPageSizeError> {
        if value == 0 {
            Err(ExternalIdentityAuditPageSizeError::Zero)
        } else if value > MAX_PAGE_SIZE {
            Err(ExternalIdentityAuditPageSizeError::TooLarge {
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

/// Invalid requested external-identity audit page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalIdentityAuditPageSizeError {
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

impl fmt::Display for ExternalIdentityAuditPageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("external identity audit page size must be nonzero"),
            Self::TooLarge { maximum, actual } => write!(
                formatter,
                "external identity audit page size {actual} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for ExternalIdentityAuditPageSizeError {}

/// Bounded request for external-identity audit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalIdentityAuditQuery {
    page_size: ExternalIdentityAuditPageSize,
    after: Option<ExternalIdentityAuditCursor>,
}

impl ExternalIdentityAuditQuery {
    /// Creates an audit query, optionally continuing after a preceding page.
    pub const fn new(
        page_size: ExternalIdentityAuditPageSize,
        after: Option<ExternalIdentityAuditCursor>,
    ) -> Self {
        Self { page_size, after }
    }

    /// Returns the maximum number of entries requested.
    pub const fn page_size(&self) -> ExternalIdentityAuditPageSize {
        self.page_size
    }

    /// Returns the exclusive cursor after which older entries are selected.
    pub const fn after(&self) -> Option<&ExternalIdentityAuditCursor> {
        self.after.as_ref()
    }
}

/// One bounded page of external-identity audit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalIdentityAuditPage {
    entries: Vec<ExternalIdentityAuditEntry>,
    next: Option<ExternalIdentityAuditCursor>,
}

impl ExternalIdentityAuditPage {
    /// Constructs a page from repository-selected entries and continuation.
    pub fn new(
        entries: Vec<ExternalIdentityAuditEntry>,
        next: Option<ExternalIdentityAuditCursor>,
    ) -> Self {
        Self { entries, next }
    }

    /// Returns newest-first audit entries in this page.
    pub fn entries(&self) -> &[ExternalIdentityAuditEntry] {
        &self.entries
    }

    /// Returns the cursor for an older page when more entries exist.
    pub const fn next(&self) -> Option<&ExternalIdentityAuditCursor> {
        self.next.as_ref()
    }
}

/// Persistence boundary for bounded external-identity audit queries.
pub trait ExternalIdentityAuditRepository: Send + Sync {
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
        query: &'a ExternalIdentityAuditQuery,
    ) -> impl Future<Output = Result<ExternalIdentityAuditPage, Self::Error>> + Send + 'a;
}

/// Coordinates authorized reads of external-identity audit history.
#[derive(Clone, Debug)]
pub struct ExternalIdentityAudit<R> {
    repository: R,
}

impl<R> ExternalIdentityAudit<R> {
    /// Creates the audit service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> ExternalIdentityAudit<R>
where
    R: ExternalIdentityAuditRepository,
{
    /// Finds one audit page with exact `ManageIdentity` authorization.
    ///
    /// # Errors
    ///
    /// Returns [`ExternalIdentityAuditError::WrongAction`] when the evidence
    /// authorizes another action, or wraps the repository error when loading
    /// fails.
    pub async fn find_page(
        &self,
        authorization: AuthorizedPlatformAction,
        query: &ExternalIdentityAuditQuery,
    ) -> Result<ExternalIdentityAuditPage, ExternalIdentityAuditError<R::Error>> {
        if authorization.action() != PlatformAction::ManageIdentity {
            return Err(ExternalIdentityAuditError::WrongAction {
                actual: authorization.action(),
            });
        }

        self.repository
            .find_page(query)
            .await
            .map_err(ExternalIdentityAuditError::Repository)
    }
}

/// Failure to execute an authorized external-identity audit query.
#[derive(Debug)]
pub enum ExternalIdentityAuditError<E> {
    /// Authorization evidence permits another platform operation.
    WrongAction {
        /// Action carried by the rejected evidence.
        actual: PlatformAction,
    },
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for ExternalIdentityAuditError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongAction { actual } => write!(
                formatter,
                "external identity audit requires ManageIdentity authorization, got {actual:?}"
            ),
            Self::Repository(_) => {
                formatter.write_str("external identity audit could not be loaded")
            }
        }
    }
}

impl<E> Error for ExternalIdentityAuditError<E>
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
