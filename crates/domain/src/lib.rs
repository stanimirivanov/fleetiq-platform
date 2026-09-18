//! Framework-independent `FleetIQ` domain types and invariants.
//!
//! The domain crate has no infrastructure dependencies. Transport and persistence
//! adapters must construct these types through their validating APIs rather than
//! duplicating domain rules.

mod asset;
mod definition;
mod device;
mod identifier;
mod property_value;
mod relationship;
mod signal_binding;

pub use asset::{Asset, AssetError, AssetName, ExternalIdentifier};
pub use definition::{
    AssetTypeDefinition, AssetTypeDefinitionError, AssetTypeRef, DefinitionName,
    DefinitionNameError, DefinitionVersion, DefinitionVersionError, PropertyDefinition,
    PropertyDefinitionError, PropertyDefinitionRef, PropertyValueKind, RelationshipTypeDefinition,
    RelationshipTypeRef,
};
pub use device::{
    CommunicationDevice, CommunicationDeviceError, DeviceEndpoint, DeviceEndpointName,
    DeviceEndpointNameError,
};
pub use identifier::{
    ActorId, AssetId, AssetRelationshipId, AssetTypeId, DeviceEndpointId, DeviceEndpointKindId,
    ExternalIdentifierAuthority, ExternalIdentifierKind, ExternalIdentifierValue, IdentifierError,
    PropertyDefinitionId, RelationshipTypeId, SignalBindingId, SourceSignalId, TenantId, UnitId,
};
pub use property_value::PropertyValue;
pub use relationship::{
    AssetRelationship, EffectiveInterval, EffectiveIntervalError, RecordRevision,
    RecordRevisionError, RecordedRevision, RelationshipEndpoints, UnixTimeMillis,
};
pub use signal_binding::{SignalBinding, SignalSource, SignalTarget};
