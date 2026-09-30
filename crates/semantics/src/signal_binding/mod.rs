//! Signal-to-property attribution contracts and resolution.

mod model;
mod ports;
mod resolver;

pub use model::{
    BindingCandidate, BoundObservation, ResolutionContext, ResolutionOutcome,
    SignalBindingProvenance, UnresolvedObservation, UnresolvedReason,
};
pub use ports::{PropertyDefinitionView, SignalBindingSnapshotQuery, SignalBindingView};
pub use resolver::{ResolutionError, SignalBindingResolver};

#[cfg(test)]
mod tests;
