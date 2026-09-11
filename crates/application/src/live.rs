//! Transport-independent contracts for revisioned projections and live updates.

mod message;
mod revision;

pub use message::{
    LiveMessage, ProjectionDelta, ProjectionDeltaError, ProjectionSnapshot, ResnapshotReason,
    ResnapshotRequired,
};
pub use revision::{
    ProjectionRevision, RevisionCursor, RevisionCursorError, RevisionOverflowError,
};

#[cfg(test)]
mod tests;
