use std::{error::Error, future::Future};

use fleetiq_domain::{
    PropertyDefinition, PropertyDefinitionRef, SignalBinding, SignalSource, TenantId,
    UnixTimeMillis,
};

/// Selects signal attributions visible at two independent time dimensions.
///
/// `effective_at` identifies when the attribution applied in the physical
/// world. `known_at` is the inclusive cutoff for revisions recorded by
/// `FleetIQ`. Callers must choose both values so corrections cannot silently
/// change the historical view used by replay or analytics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalBindingSnapshotQuery {
    effective_at: UnixTimeMillis,
    known_at: UnixTimeMillis,
}

impl SignalBindingSnapshotQuery {
    /// Constructs a bitemporal source-resolution query.
    pub const fn new(effective_at: UnixTimeMillis, known_at: UnixTimeMillis) -> Self {
        Self {
            effective_at,
            known_at,
        }
    }

    /// Returns the physical-time instant selected by this query.
    pub const fn effective_at(self) -> UnixTimeMillis {
        self.effective_at
    }

    /// Returns the inclusive recorded-time cutoff selected by this query.
    pub const fn known_at(self) -> UnixTimeMillis {
        self.known_at
    }
}

/// Provides the bitemporal binding read required by semantic resolution.
///
/// An adapter or worker-local projection implements this consumer-owned view
/// without exposing persistence write operations.
pub trait SignalBindingView: Send + Sync {
    /// The infrastructure-specific lookup failure.
    type Error: Error + Send + Sync + 'static;

    /// Finds bindings selecting a source in one bitemporal snapshot.
    fn find_for_source(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send;
}

/// Provides exact property definitions required by semantic resolution.
pub trait PropertyDefinitionView: Send + Sync {
    /// The infrastructure-specific lookup failure.
    type Error: Error + Send + Sync + 'static;

    /// Finds one exact tenant-owned property-definition version.
    fn find_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a;
}
