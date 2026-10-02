//! Tenant-owned asset graph use cases and consumer-owned persistence ports.
//!
//! Definitions, assets, communication devices, relationships, and signal
//! bindings share the metadata authorization boundary. PostgreSQL adapts these
//! ports without entering the application policy.

use fleetiq_access::{AuthorizedTenant, ManageMetadata, ReadMetadata};

#[cfg(test)]
use fleetiq_access::{
    AccessDecision, ActorIdentityResolver, AuthenticatedActor, CallerAuthenticationService,
    ExternalIdentityAuthenticator, TenantAccessPolicy, TenantAccessRequest, TenantActionMarker,
    TenantAuthorizationService,
};

mod asset_service;
mod definition_repository;
mod definition_service;
mod device_repository;
mod device_service;
mod relationship_repository;
mod relationship_service;
mod repository;
mod signal_binding_repository;
mod signal_binding_service;

#[cfg(test)]
mod test_support;

pub use asset_service::{AssetApplicationError, AssetApplicationService};
pub use definition_repository::DefinitionRepository;
pub use definition_service::{DefinitionApplicationError, DefinitionApplicationService};
pub use device_repository::DeviceRepository;
pub use device_service::{DeviceApplicationError, DeviceApplicationService};
pub use relationship_repository::{
    InvalidRelationshipPageLimit, RelationshipRepository, RelationshipSnapshotPage,
    RelationshipSnapshotPageQuery, RelationshipSnapshotQuery,
};
pub use relationship_service::{RelationshipApplicationError, RelationshipApplicationService};
pub use repository::{
    AssetCatalogueEntry, AssetPage, AssetPageQuery, AssetRepository, InvalidAssetPageLimit,
};
pub use signal_binding_repository::{
    InvalidSignalBindingPageLimit, SignalBindingRepository, SignalBindingSnapshotPage,
    SignalBindingSnapshotPageQuery, SignalBindingSnapshotQuery,
};
pub use signal_binding_service::{SignalBindingApplicationError, SignalBindingApplicationService};
