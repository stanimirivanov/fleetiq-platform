use std::{error::Error, fmt};

use crate::{PropertyDefinitionId, UnitId};

use super::{DefinitionName, DefinitionVersion, PropertyDefinitionRef};

/// The storage-level value category of an asset property.
///
/// Domain meaning belongs to [`PropertyDefinition`]. For example, a numeric
/// temperature and voltage are both decimals but have different definition IDs
/// and canonical units.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PropertyValueKind {
    /// A true/false state.
    Boolean,
    /// A signed whole number.
    Integer,
    /// A real-valued number.
    Decimal,
    /// Human-readable or protocol-defined text.
    Text,
}

impl PropertyValueKind {
    fn is_numeric(self) -> bool {
        matches!(self, Self::Integer | Self::Decimal)
    }
}

/// Defines the stable meaning and representation of an observable asset property.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyDefinition {
    reference: PropertyDefinitionRef,
    name: DefinitionName,
    value_kind: PropertyValueKind,
    canonical_unit: Option<UnitId>,
}

impl PropertyDefinition {
    /// Constructs an immutable property-definition version.
    ///
    /// # Errors
    ///
    /// Returns [`PropertyDefinitionError::UnitRequiresNumericValue`] when a unit
    /// is assigned to a Boolean or text property.
    pub fn new(
        reference: PropertyDefinitionRef,
        name: DefinitionName,
        value_kind: PropertyValueKind,
        canonical_unit: Option<UnitId>,
    ) -> Result<Self, PropertyDefinitionError> {
        if canonical_unit.is_some() && !value_kind.is_numeric() {
            return Err(PropertyDefinitionError::UnitRequiresNumericValue);
        }

        Ok(Self {
            reference,
            name,
            value_kind,
            canonical_unit,
        })
    }

    /// Returns the stable property-definition identity.
    pub fn id(&self) -> &PropertyDefinitionId {
        self.reference.id()
    }

    /// Returns this immutable definition's version.
    pub fn version(&self) -> DefinitionVersion {
        self.reference.version()
    }

    /// Returns the identity and version of this immutable definition.
    pub fn reference(&self) -> &PropertyDefinitionRef {
        &self.reference
    }

    /// Returns the operator-facing property name.
    pub fn name(&self) -> &DefinitionName {
        &self.name
    }

    /// Returns the property's storage-level value category.
    pub fn value_kind(&self) -> PropertyValueKind {
        self.value_kind
    }

    /// Returns the canonical unit for a numeric property, when one is defined.
    pub fn canonical_unit(&self) -> Option<&UnitId> {
        self.canonical_unit.as_ref()
    }
}

/// Describes why a property definition could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PropertyDefinitionError {
    /// Units apply only to integer and decimal property values.
    UnitRequiresNumericValue,
}

impl fmt::Display for PropertyDefinitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a canonical unit requires a numeric property value")
    }
}

impl Error for PropertyDefinitionError {}
