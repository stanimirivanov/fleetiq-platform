//! Ports are consumed by projection, with no SQL, task scheduler or transport types.
use crate::CurrentPropertySnapshot;
use fleetiq_domain::{SignalTarget, TenantId};
use std::{error::Error, future::Future};
/// One atomic bounded batch result; absence of work is not a health assertion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionTick {
    /// No eligible tenant had committed pending work when inspected.
    Idle,
    /// Applied this many queue entries and committed one tenant revision.
    Applied(u16),
}
/// Durable projector and operator-controlled atomic rebuild boundary.
///
/// Implementations serialize per tenant and atomically consume at most 100
/// queued immutable facts with their current rows and checkpoint. There is no
/// sequence watermark: a later commit with an older queue ID remains pending.
/// Rebuild uses one consistent history snapshot, advances generation/revision,
/// and cannot discard facts committed outside that snapshot. Cancellation rolls
/// back the complete operation; retries may return Idle after an ambiguous commit.
pub trait ProjectionMaintenance: Send + Sync {
    /// Infrastructure or invalid durable-state failure.
    type Error: Error + Send + Sync + 'static;
    /// Processes one bounded transaction, rechecking active tenant state.
    fn tick(&self) -> impl Future<Output = Result<ProjectionTick, Self::Error>> + Send;
    /// Recomputes one active tenant atomically under its projector lock.
    /// A timed-out rebuild leaves the previous generation intact.
    fn rebuild(&self, tenant: &TenantId) -> impl Future<Output = Result<(), Self::Error>> + Send;
}
/// Reads one complete exact-property projection in a repeatable database view.
pub trait CurrentStateRepository: Send + Sync {
    /// Infrastructure or invalid immutable-reference failure.
    type Error: Error + Send + Sync + 'static;
    /// Returns None only when this exact installed property does not exist.
    /// Known properties with no materialized value return an empty snapshot.
    fn snapshot(
        &self,
        tenant: &TenantId,
        target: &SignalTarget,
    ) -> impl Future<Output = Result<Option<CurrentPropertySnapshot>, Self::Error>> + Send;
}
