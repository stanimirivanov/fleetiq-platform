//! Framework-independent `FleetIQ` domain types and invariants.
//!
//! The domain crate has no infrastructure dependencies. Transport and persistence
//! adapters must construct these types through their validating APIs rather than
//! duplicating domain rules.

mod asset;
mod definition;
mod identifier;

pub use asset::{Asset, AssetError, AssetName, ExternalIdentifier};
pub use definition::{
    AssetTypeDefinition, AssetTypeDefinitionError, AssetTypeRef, DefinitionName,
    DefinitionNameError, DefinitionVersion, DefinitionVersionError, PropertyDefinition,
    PropertyDefinitionError, PropertyDefinitionRef, PropertyValueKind,
};
pub use identifier::{
    AssetId, AssetTypeId, ExternalIdentifierAuthority, ExternalIdentifierKind,
    ExternalIdentifierValue, IdentifierError, PropertyDefinitionId, TenantId, UnitId,
};
