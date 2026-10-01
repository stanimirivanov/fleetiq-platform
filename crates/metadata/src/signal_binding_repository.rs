use std::{error::Error, future::Future};

use fleetiq_domain::{RecordRevision, SignalBinding, SignalBindingId, SignalSource, TenantId};

// Preserve the application-facing query name while semantic policy owns it.
pub use fleetiq_semantics::SignalBindingSnapshotQuery;

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
}
