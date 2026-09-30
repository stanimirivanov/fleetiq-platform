use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{ActorId, TenantAccessChangeId, TenantId, UnixTimeMillis};

use crate::{
    AuthorizedTenant, ManageAccess, TenantAccessChangeEffect, TenantAccessChangeKind, TenantAction,
};

const MAX_PAGE_SIZE: u16 = 100;

/// One immutable tenant access administration audit record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantAccessAuditEntry {
    change_id: TenantAccessChangeId,
    tenant_id: TenantId,
    administrator_actor_id: ActorId,
    target_actor_id: ActorId,
    action: TenantAction,
    kind: TenantAccessChangeKind,
    recorded_at: UnixTimeMillis,
    effect: TenantAccessChangeEffect,
}

impl TenantAccessAuditEntry {
    /// Reconstructs one complete durable audit record.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        change_id: TenantAccessChangeId,
        tenant_id: TenantId,
        administrator_actor_id: ActorId,
        target_actor_id: ActorId,
        action: TenantAction,
        kind: TenantAccessChangeKind,
        recorded_at: UnixTimeMillis,
        effect: TenantAccessChangeEffect,
    ) -> Self {
        Self {
            change_id,
            tenant_id,
            administrator_actor_id,
            target_actor_id,
            action,
            kind,
            recorded_at,
            effect,
        }
    }

    /// Returns the tenant-scoped administration command identity.
    pub const fn change_id(&self) -> &TenantAccessChangeId {
        &self.change_id
    }

    /// Returns the tenant whose authorization state was addressed.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the actor that authorized the command.
    pub const fn administrator_actor_id(&self) -> &ActorId {
        &self.administrator_actor_id
    }

    /// Returns the actor whose exact action grant was addressed.
    pub const fn target_actor_id(&self) -> &ActorId {
        &self.target_actor_id
    }

    /// Returns the exact tenant action addressed by the command.
    pub const fn action(&self) -> TenantAction {
        self.action
    }

    /// Returns whether the command granted or revoked the action.
    pub const fn kind(&self) -> TenantAccessChangeKind {
        self.kind
    }

    /// Returns the trusted host time at which the command was recorded.
    pub const fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_at
    }

    /// Returns whether the command changed current grant state.
    pub const fn effect(&self) -> TenantAccessChangeEffect {
        self.effect
    }
}

/// Stable position after which an older audit page is requested.
///
/// Audit order is descending by `(recorded_at, change_id)`. The identity
/// component makes the cursor deterministic when multiple commands share the
/// same recorded time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantAccessAuditCursor {
    recorded_at: UnixTimeMillis,
    change_id: TenantAccessChangeId,
}

impl TenantAccessAuditCursor {
    /// Creates a cursor from the last entry returned by a preceding page.
    pub const fn new(recorded_at: UnixTimeMillis, change_id: TenantAccessChangeId) -> Self {
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
    pub const fn change_id(&self) -> &TenantAccessChangeId {
        &self.change_id
    }
}

/// Validated maximum number of audit entries returned in one page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantAccessAuditPageSize(u16);

impl TenantAccessAuditPageSize {
    /// Creates a page size in the inclusive range `1..=100`.
    ///
    /// # Errors
    ///
    /// Returns [`TenantAccessAuditPageSizeError`] when `value` is zero or
    /// exceeds the maximum supported page size.
    pub const fn new(value: u16) -> Result<Self, TenantAccessAuditPageSizeError> {
        if value == 0 {
            Err(TenantAccessAuditPageSizeError::Zero)
        } else if value > MAX_PAGE_SIZE {
            Err(TenantAccessAuditPageSizeError::TooLarge {
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

/// Invalid requested tenant access audit page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TenantAccessAuditPageSizeError {
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

impl fmt::Display for TenantAccessAuditPageSizeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero => formatter.write_str("tenant access audit page size must be nonzero"),
            Self::TooLarge { maximum, actual } => write!(
                formatter,
                "tenant access audit page size {actual} exceeds maximum {maximum}"
            ),
        }
    }
}

impl Error for TenantAccessAuditPageSizeError {}

/// Bounded request for tenant access audit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantAccessAuditQuery {
    page_size: TenantAccessAuditPageSize,
    after: Option<TenantAccessAuditCursor>,
}

impl TenantAccessAuditQuery {
    /// Creates an audit query, optionally continuing after a preceding page.
    pub const fn new(
        page_size: TenantAccessAuditPageSize,
        after: Option<TenantAccessAuditCursor>,
    ) -> Self {
        Self { page_size, after }
    }

    /// Returns the maximum number of entries requested.
    pub const fn page_size(&self) -> TenantAccessAuditPageSize {
        self.page_size
    }

    /// Returns the exclusive cursor after which older entries are selected.
    pub const fn after(&self) -> Option<&TenantAccessAuditCursor> {
        self.after.as_ref()
    }
}

/// One bounded page of tenant access audit history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantAccessAuditPage {
    entries: Vec<TenantAccessAuditEntry>,
    next: Option<TenantAccessAuditCursor>,
}

impl TenantAccessAuditPage {
    /// Constructs a page from repository-selected entries and continuation.
    pub fn new(
        entries: Vec<TenantAccessAuditEntry>,
        next: Option<TenantAccessAuditCursor>,
    ) -> Self {
        Self { entries, next }
    }

    /// Returns newest-first audit entries in this page.
    pub fn entries(&self) -> &[TenantAccessAuditEntry] {
        &self.entries
    }

    /// Returns the cursor for an older page when more entries exist.
    pub const fn next(&self) -> Option<&TenantAccessAuditCursor> {
        self.next.as_ref()
    }
}

/// Persistence boundary for bounded tenant access audit queries.
pub trait TenantAccessAuditRepository: Send + Sync {
    /// Storage or reconstruction failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Finds one newest-first page within exactly one tenant.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when the page cannot be loaded or a
    /// durable record cannot be reconstructed.
    fn find_page<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        query: &'a TenantAccessAuditQuery,
    ) -> impl Future<Output = Result<TenantAccessAuditPage, Self::Error>> + Send + 'a;
}

/// Coordinates authorized reads of tenant access audit history.
#[derive(Clone, Debug)]
pub struct TenantAccessAudit<R> {
    repository: R,
}

impl<R> TenantAccessAudit<R> {
    /// Creates the audit service using the supplied repository.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> TenantAccessAudit<R>
where
    R: TenantAccessAuditRepository,
{
    /// Finds one audit page with exact `ManageAccess` authorization.
    ///
    /// Tenant scope is derived exclusively from the consumed authorization
    /// evidence.
    ///
    /// # Errors
    ///
    /// Wraps the repository error when the page cannot be loaded.
    pub async fn find_page(
        &self,
        authorization: AuthorizedTenant<ManageAccess>,
        query: &TenantAccessAuditQuery,
    ) -> Result<TenantAccessAuditPage, TenantAccessAuditError<R::Error>> {
        self.repository
            .find_page(authorization.tenant_id(), query)
            .await
            .map_err(TenantAccessAuditError::Repository)
    }
}

/// Failure to execute an authorized tenant access audit query.
#[derive(Debug)]
pub enum TenantAccessAuditError<E> {
    /// The persistence operation failed.
    Repository(E),
}

impl<E> fmt::Display for TenantAccessAuditError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Repository(_) => formatter.write_str("tenant access audit could not be loaded"),
        }
    }
}

impl<E> Error for TenantAccessAuditError<E>
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
