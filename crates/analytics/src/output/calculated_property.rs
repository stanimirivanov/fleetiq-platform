use std::{error::Error, fmt};

use fleetiq_domain::{
    AssetId, PropertyDefinition, PropertyDefinitionRef, PropertyValue, PropertyValueKind, UnitId,
};

/// One semantic property value emitted by calculator logic.
///
/// Calculator code selects an exact target property and emits a value in that
/// property's canonical unit. The analytics host supplies evaluation and
/// version provenance when assembling the final output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalculatedProperty {
    asset_id: AssetId,
    property: PropertyDefinitionRef,
    value: PropertyValue,
    unit: Option<UnitId>,
}

impl CalculatedProperty {
    /// Constructs a calculated property against its exact semantic definition.
    ///
    /// # Errors
    ///
    /// Returns [`CalculatedPropertyError::ValueKindMismatch`] when the value
    /// kind differs from the definition, or
    /// [`CalculatedPropertyError::CanonicalUnitMismatch`] when the unit differs
    /// from the definition's canonical unit.
    pub fn new(
        asset_id: AssetId,
        definition: &PropertyDefinition,
        value: PropertyValue,
        unit: Option<UnitId>,
    ) -> Result<Self, CalculatedPropertyError> {
        if value.kind() != definition.value_kind() {
            return Err(CalculatedPropertyError::ValueKindMismatch {
                actual: value.kind(),
                expected: definition.value_kind(),
            });
        }
        if unit.as_ref() != definition.canonical_unit() {
            return Err(CalculatedPropertyError::CanonicalUnitMismatch {
                actual: unit,
                expected: definition.canonical_unit().cloned(),
            });
        }

        Ok(Self {
            asset_id,
            property: definition.reference().clone(),
            value,
            unit: definition.canonical_unit().cloned(),
        })
    }

    /// Returns the asset receiving this derived value.
    pub const fn asset_id(&self) -> &AssetId {
        &self.asset_id
    }

    /// Returns the exact semantic property definition receiving the value.
    pub const fn property(&self) -> &PropertyDefinitionRef {
        &self.property
    }

    /// Returns the calculated property value.
    pub const fn value(&self) -> &PropertyValue {
        &self.value
    }

    /// Returns the canonical unit supplied for the calculated value.
    pub const fn unit(&self) -> Option<&UnitId> {
        self.unit.as_ref()
    }
}

/// Describes why a calculated property conflicts with its semantic definition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CalculatedPropertyError {
    /// The emitted value kind differs from the target definition.
    ValueKindMismatch {
        /// Kind emitted by calculator logic.
        actual: PropertyValueKind,
        /// Kind required by the exact target property definition.
        expected: PropertyValueKind,
    },
    /// The emitted unit differs from the target definition's canonical unit.
    CanonicalUnitMismatch {
        /// Unit emitted by calculator logic.
        actual: Option<UnitId>,
        /// Canonical unit required by the exact target property definition.
        expected: Option<UnitId>,
    },
}

impl fmt::Display for CalculatedPropertyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ValueKindMismatch { actual, expected } => write!(
                formatter,
                "calculated value kind {actual:?} does not match target kind {expected:?}"
            ),
            Self::CanonicalUnitMismatch { actual, expected } => write!(
                formatter,
                "calculated unit {actual:?} does not match canonical unit {expected:?}"
            ),
        }
    }
}

impl Error for CalculatedPropertyError {}
