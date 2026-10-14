//! Actor-scoped, advisory tenant discovery; entries are never authorization evidence.

use std::{error::Error, fmt, future::Future, num::NonZeroU16};

use crate::{AuthenticatedActor, TenantAction};
use fleetiq_domain::{ActorId, TenantId};

/// Validated bounded discovery request, ordered by ascending tenant identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantDiscoveryQuery {
    limit: NonZeroU16,
    after: Option<TenantId>,
}

impl TenantDiscoveryQuery {
    /// Creates a page request with an inclusive limit of 1 through 100.
    ///
    /// The cursor is exclusive and need not identify a currently accessible tenant.
    ///
    /// # Errors
    ///
    /// Returns an error for zero or more than 100 entries.
    pub fn new(limit: u16, after: Option<TenantId>) -> Result<Self, TenantDiscoveryQueryError> {
        let limit = NonZeroU16::new(limit)
            .filter(|value| value.get() <= 100)
            .ok_or(TenantDiscoveryQueryError)?;
        Ok(Self { limit, after })
    }

    /// Returns the maximum number of tenants, excluding the lookahead row.
    pub const fn limit(&self) -> u16 {
        self.limit.get()
    }

    /// Returns the exclusive tenant-identity cursor.
    pub const fn after(&self) -> Option<&TenantId> {
        self.after.as_ref()
    }
}

/// Invalid discovery page size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantDiscoveryQueryError;

impl fmt::Display for TenantDiscoveryQueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("tenant discovery limit must be between 1 and 100")
    }
}
impl Error for TenantDiscoveryQueryError {}

/// One active tenant and its explicitly granted current actions.
///
/// This is a presentation hint, not reusable authority. No action implies any
/// other action, and platform grants are deliberately excluded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredTenant {
    tenant_id: TenantId,
    actions: Vec<TenantAction>,
}

impl DiscoveredTenant {
    /// Reconstructs one trusted repository result.
    pub const fn new(tenant_id: TenantId, actions: Vec<TenantAction>) -> Self {
        Self { tenant_id, actions }
    }
    /// Returns the stable tenant identity.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }
    /// Returns only exact tenant actions currently granted to this actor.
    pub fn actions(&self) -> &[TenantAction] {
        &self.actions
    }
}

/// Bounded current-policy results, without a cross-request snapshot guarantee.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TenantDiscoveryPage {
    tenants: Vec<DiscoveredTenant>,
    next_after: Option<TenantId>,
}
impl TenantDiscoveryPage {
    /// Reconstructs repository-selected entries and an optional continuation.
    pub const fn new(tenants: Vec<DiscoveredTenant>, next_after: Option<TenantId>) -> Self {
        Self {
            tenants,
            next_after,
        }
    }
    /// Returns tenants in ascending stable-identity order.
    pub fn tenants(&self) -> &[DiscoveredTenant] {
        &self.tenants
    }
    /// Returns the last emitted identity only when another tenant exists.
    pub const fn next_after(&self) -> Option<&TenantId> {
        self.next_after.as_ref()
    }
}

/// Read port for the current active actor's active, explicitly granted tenants.
///
/// Implementations must filter inactive actors and tenants and exclude all
/// other actors. A missing or inactive actor produces an empty page. Query
/// failures remain errors, never a successful empty discovery result.
pub trait TenantDiscoveryRepository: Send + Sync {
    /// Storage or reconstruction failure.
    type Error: Error + Send + Sync + 'static;

    /// Loads one bounded page including every exact action for each tenant.
    ///
    /// # Errors
    ///
    /// Returns an implementation error when policy cannot be read reliably.
    fn find_page<'a>(
        &'a self,
        actor: &'a ActorId,
        query: &'a TenantDiscoveryQuery,
    ) -> impl Future<Output = Result<TenantDiscoveryPage, Self::Error>> + Send + 'a;
}

/// Discovers access for the authenticated caller, without issuing grants.
#[derive(Clone, Debug)]
pub struct TenantDiscovery<R> {
    repository: R,
}
impl<R> TenantDiscovery<R> {
    /// Creates discovery with the consumer-owned read port.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}
impl<R: TenantDiscoveryRepository> TenantDiscovery<R> {
    /// Consumes fresh authentication evidence and discovers only its actor.
    ///
    /// Every subsequent protected operation must independently reauthorize.
    ///
    /// # Errors
    ///
    /// Preserves repository errors so transports can fail closed.
    pub async fn find_page(
        &self,
        actor: AuthenticatedActor,
        query: &TenantDiscoveryQuery,
    ) -> Result<TenantDiscoveryPage, R::Error> {
        self.repository.find_page(actor.actor_id(), query).await
    }
}

#[cfg(test)]
mod tests;
