//! Rebuildable current-property policy, consistent snapshot ports and worker boundary.
//!
//! History remains authoritative. Projection revisions describe committed batches,
//! not event-time watermarks; a durable pending queue closes commit-order gaps.
//! `fleetiq-live` remains a transport-independent envelope contract.
mod freshness;
mod ports;
mod precedence;
pub use precedence::compare_precedence;
mod snapshot;
pub use freshness::{Freshness, freshness};
pub use ports::{CurrentStateRepository, ProjectionMaintenance, ProjectionTick};
pub use snapshot::{CurrentPropertySnapshot, CurrentStateService};
