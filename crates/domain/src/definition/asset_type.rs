use std::{error::Error, fmt};

use crate::PropertyDefinitionId;

use super::{AssetTypeRef, DefinitionName, PropertyDefinitionRef};

/// Defines the properties supported by one immutable version of an asset type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetTypeDefinition {
    reference: AssetTypeRef,
    name: DefinitionName,
    supported_properties: Vec<PropertyDefinitionRef>,
}

impl AssetTypeDefinition {
    /// Constructs an asset-type definition without supported properties.
    pub fn new(reference: AssetTypeRef, name: DefinitionName) -> Self {
        Self {
            reference,
            name,
            supported_properties: Vec::new(),
        }
    }

    /// Returns the identity and version of this immutable definition.
    pub fn reference(&self) -> &AssetTypeRef {
        &self.reference
    }

    /// Returns the operator-facing asset-type name.
    pub fn name(&self) -> &DefinitionName {
        &self.name
    }

    /// Returns the property IDs supported by this asset type.
    pub fn supported_properties(&self) -> &[PropertyDefinitionRef] {
        &self.supported_properties
    }

    /// Reports whether the asset type supports a property definition.
    pub fn supports_property(&self, property_id: &PropertyDefinitionId) -> bool {
        self.supported_properties
            .iter()
            .any(|reference| reference.id() == property_id)
    }

    /// Adds a supported property to this definition version.
    ///
    /// # Errors
    ///
    /// Returns [`AssetTypeDefinitionError::DuplicateProperty`] when the property
    /// is already present.
    pub fn add_property(
        &mut self,
        property: PropertyDefinitionRef,
    ) -> Result<(), AssetTypeDefinitionError> {
        if self.supports_property(property.id()) {
            return Err(AssetTypeDefinitionError::DuplicateProperty);
        }

        self.supported_properties.push(property);
        Ok(())
    }
}

/// Describes a rejected asset-type definition operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetTypeDefinitionError {
    /// The property is already supported by this definition version.
    DuplicateProperty,
}

impl fmt::Display for AssetTypeDefinitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("property is already supported by this asset type")
    }
}

impl Error for AssetTypeDefinitionError {}
