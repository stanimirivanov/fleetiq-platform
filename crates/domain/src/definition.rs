//! Versioned semantic definitions and their shared value objects.

mod asset_type;
mod common;
mod property;
mod relationship_type;

pub use asset_type::{AssetTypeDefinition, AssetTypeDefinitionError};
pub use common::{
    AssetTypeRef, DefinitionName, DefinitionNameError, DefinitionVersion, DefinitionVersionError,
    PropertyDefinitionRef, RelationshipTypeRef,
};
pub use property::{PropertyDefinition, PropertyDefinitionError, PropertyValueKind};
pub use relationship_type::RelationshipTypeDefinition;

#[cfg(test)]
mod tests;
