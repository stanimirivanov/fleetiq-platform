//! Semantic attribution and normalization for `FleetIQ` telemetry.
//!
//! This crate connects protocol-neutral decoded observations to tenant-owned
//! asset properties. Infrastructure implements consumer-owned semantic read ports.

mod asset_change;
mod signal_binding;

pub use asset_change::{
    AssetPropertyChange, AssetPropertyChangeError, AssetPropertyChangeId, EventTimeBasis,
};
pub use signal_binding::{
    BindingCandidate, BoundObservation, PropertyDefinitionView, ResolutionContext, ResolutionError,
    ResolutionOutcome, SignalBindingProvenance, SignalBindingResolver, SignalBindingSnapshotQuery,
    SignalBindingView, UnresolvedObservation, UnresolvedReason,
};
