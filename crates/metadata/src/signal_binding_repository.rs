use std::{error::Error, future::Future};

use fleetiq_domain::{RecordRevision, SignalBinding, SignalBindingId, SignalSource, TenantId};

// Preserve the application-facing query name while semantic policy owns it.
pub use fleetiq_semantics::SignalBindingSnapshotQuery;

/// A bounded, exclusive-keyset request for one decoded source's bindings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignalBindingSnapshotPageQuery {
    snapshot: SignalBindingSnapshotQuery,
    limit: u8,
    after: Option<SignalBindingId>,
}

impl SignalBindingSnapshotPageQuery {
    /// Maximum number of bindings returned by one page.
    pub const MAX_LIMIT: u8 = 100;

    /// Creates a page request with a limit between 1 and 100.
    ///
    /// # Errors
    ///
    /// Returns an error when the limit is outside the supported range.
    pub fn new(
        snapshot: SignalBindingSnapshotQuery,
        limit: u8,
        after: Option<SignalBindingId>,
    ) -> Result<Self, InvalidSignalBindingPageLimit> {
        if limit == 0 || limit > Self::MAX_LIMIT {
            return Err(InvalidSignalBindingPageLimit);
        }
        Ok(Self {
            snapshot,
            limit,
            after,
        })
    }

    /// Returns the explicit physical and recorded-time cutoffs.
    pub const fn snapshot(&self) -> SignalBindingSnapshotQuery {
        self.snapshot
    }

    /// Returns the maximum result count.
    pub const fn limit(&self) -> u8 {
        self.limit
    }

    /// Returns the exclusive stable-identity cursor.
    pub fn after(&self) -> Option<&SignalBindingId> {
        self.after.as_ref()
    }
}

/// A signal-binding page limit outside 1 through 100.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSignalBindingPageLimit;

impl std::fmt::Display for InvalidSignalBindingPageLimit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("signal-binding page limit must be between 1 and 100")
    }
}

impl Error for InvalidSignalBindingPageLimit {}

/// One ordered page of recorded bindings selecting a decoded source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignalBindingSnapshotPage {
    bindings: Vec<SignalBinding>,
    next_after: Option<SignalBindingId>,
}

impl SignalBindingSnapshotPage {
    /// Creates a page assembled by a repository adapter.
    pub fn new(bindings: Vec<SignalBinding>, next_after: Option<SignalBindingId>) -> Self {
        Self {
            bindings,
            next_after,
        }
    }

    /// Returns binding revisions in ascending stable-identity order.
    pub fn bindings(&self) -> &[SignalBinding] {
        &self.bindings
    }

    /// Returns the exclusive cursor for the next page, if one exists.
    pub fn next_after(&self) -> Option<&SignalBindingId> {
        self.next_after.as_ref()
    }
}

/// Persists and resolves revisioned decoded-signal attributions.
///
/// Each inserted value is a complete immutable revision. Implementations must
/// serialize concurrent changes to one stable binding identity, reject gaps in
/// its recorded revision sequence, and preserve tenant boundaries.
pub trait SignalBindingRepository: Send + Sync {
    /// The infrastructure-specific failure returned by this repository.
    type Error: Error + Send + Sync + 'static;

    /// Appends one immutable signal-binding revision.
    fn insert_revision(
        &self,
        binding: &SignalBinding,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;

    /// Finds one exact recorded revision within a tenant.
    fn find_revision(
        &self,
        tenant_id: &TenantId,
        binding_id: &SignalBindingId,
        revision: RecordRevision,
    ) -> impl Future<Output = Result<Option<SignalBinding>, Self::Error>> + Send;

    /// Resolves bindings for a decoded source in a bitemporal snapshot.
    ///
    /// The result contains the latest revision of each candidate binding known
    /// by `query.known_at()` when that revision still selects `source` and is
    /// effective at `query.effective_at()`.
    fn find_for_source(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send;

    /// Returns a bounded page of latest-known bindings for a decoded source.
    ///
    /// Implementations must choose each candidate's latest known revision
    /// before filtering its source and physical effective interval.
    fn find_for_source_page(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: &SignalBindingSnapshotPageQuery,
    ) -> impl Future<Output = Result<SignalBindingSnapshotPage, Self::Error>> + Send;
}
