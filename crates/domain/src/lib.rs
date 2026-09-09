//! Framework-independent `FleetIQ` domain types and invariants.
//!
//! The domain crate has no infrastructure dependencies. Transport and persistence
//! adapters must construct these types through their validating APIs rather than
//! duplicating domain rules.

mod asset;
mod definition;
mod identifier;
mod relationship;

pub use asset::{Asset, AssetError, AssetName, ExternalIdentifier};
pub use definition::{
    AssetTypeDefinition, AssetTypeDefinitionError, AssetTypeRef, DefinitionName,
    DefinitionNameError, DefinitionVersion, DefinitionVersionError, PropertyDefinition,
    PropertyDefinitionError, PropertyDefinitionRef, PropertyValueKind, RelationshipTypeDefinition,
    RelationshipTypeRef,
};
pub use identifier::{
    AssetId, AssetRelationshipId, AssetTypeId, ExternalIdentifierAuthority, ExternalIdentifierKind,
    ExternalIdentifierValue, IdentifierError, PropertyDefinitionId, RelationshipTypeId, TenantId,
    UnitId,
};
pub use relationship::{
    AssetRelationship, EffectiveInterval, EffectiveIntervalError, RecordRevision,
    RecordRevisionError, RecordedRevision, RelationshipEndpoints, UnixTimeMillis,
};
