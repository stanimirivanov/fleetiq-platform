//! Semantic attribution and normalization for `FleetIQ` telemetry.
//!
//! This crate connects protocol-neutral decoded observations to tenant-owned
//! asset properties. Infrastructure remains behind application-owned ports.

mod signal_binding;

pub use signal_binding::{
    BindingCandidate, BoundObservation, PropertyDefinitionView, ResolutionContext, ResolutionError,
    ResolutionOutcome, SignalBindingProvenance, SignalBindingResolver, SignalBindingView,
    UnresolvedObservation, UnresolvedReason,
};
