//! Exact property snapshots require fresh telemetry-read evidence at entry.
use crate::CurrentStateRepository;
use fleetiq_access::{AuthorizedTenant, ReadTelemetry};
use fleetiq_domain::SignalTarget;
use fleetiq_history::StoredPropertyChange;
use fleetiq_live::{ProjectionRevision, ProjectionSnapshot};
/// One exact property version at a committed tenant projection revision.
///
/// Generation changes on rebuild. The revision never resets within a tenant.
/// `pending` refers only to committed pending interpretation work for this tenant;
/// it does not assert decoded/semantic queue emptiness or device connectivity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentPropertySnapshot {
    generation: u64,
    processed_count: u64,
    pending: bool,
    state: ProjectionSnapshot<StoredPropertyChange>,
}
impl CurrentPropertySnapshot {
    /// Assembles a validated exact-property read: at most one materialized item.
    pub fn new(
        generation: u64,
        revision: u64,
        processed_count: u64,
        pending: bool,
        item: Option<StoredPropertyChange>,
    ) -> Self {
        Self {
            generation,
            processed_count,
            pending,
            state: ProjectionSnapshot::new(
                ProjectionRevision::new(revision),
                item.into_iter().collect(),
            ),
        }
    }
    /// Returns the rebuild generation; zero precedes the first history append.
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    /// Returns the number of consumed interpretations since the last rebuild.
    /// Rebuild sets this to the visible authoritative history count.
    pub const fn processed_count(&self) -> u64 {
        self.processed_count
    }
    /// Reports committed tenant work not yet included in current state.
    pub const fn pending(&self) -> bool {
        self.pending
    }
    /// Returns the complete exact-property projection envelope.
    pub const fn state(&self) -> &ProjectionSnapshot<StoredPropertyChange> {
        &self.state
    }
}
/// Application boundary preventing metadata authority from granting telemetry access.
pub struct CurrentStateService<R> {
    repository: R,
}
impl<R> CurrentStateService<R> {
    /// Composes the consistent read port.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}
impl<R: CurrentStateRepository> CurrentStateService<R> {
    /// Reads one exact installed property without interpreting metadata again.
    ///
    /// # Errors
    /// Returns storage failure, never a fabricated missing or empty value.
    pub async fn snapshot(
        &self,
        grant: AuthorizedTenant<ReadTelemetry>,
        target: &SignalTarget,
    ) -> Result<Option<CurrentPropertySnapshot>, R::Error> {
        self.repository.snapshot(grant.tenant_id(), target).await
    }
}
