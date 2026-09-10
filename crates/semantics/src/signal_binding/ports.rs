use std::{error::Error, future::Future};

use fleetiq_application::{
    DefinitionRepository, SignalBindingRepository, SignalBindingSnapshotQuery,
};
use fleetiq_domain::{
    PropertyDefinition, PropertyDefinitionRef, SignalBinding, SignalSource, TenantId,
};

/// Provides the bitemporal binding read required by semantic resolution.
///
/// The blanket implementation adapts any application [`SignalBindingRepository`].
/// A worker-local projection can implement this smaller consumer-owned view
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

impl<T: SignalBindingRepository> SignalBindingView for T {
    type Error = T::Error;

    fn find_for_source(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send {
        SignalBindingRepository::find_for_source(self, tenant_id, source, query)
    }
}

/// Provides exact property definitions required by semantic resolution.
///
/// The blanket implementation adapts any application [`DefinitionRepository`].
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

impl<T: DefinitionRepository> PropertyDefinitionView for T {
    type Error = T::Error;

    fn find_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        DefinitionRepository::find_property(self, tenant_id, reference)
    }
}
