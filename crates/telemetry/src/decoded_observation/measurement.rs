use std::{error::Error, fmt};

use bigdecimal::BigDecimal;
use fleetiq_domain::{PropertyValueKind, UnitId};

/// A value decoded from a device message before semantic attribution.
///
/// Values mirror the property kinds understood by the semantic model. Protocol
/// packages must retain the raw accepted message separately when a payload
/// cannot be represented by one of these typed values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedValue {
    /// A Boolean value.
    Boolean(bool),
    /// A signed whole number.
    Integer(i64),
    /// An arbitrary-precision decimal number.
    Decimal(BigDecimal),
    /// Protocol-defined text preserved without normalization.
    Text(String),
}

impl DecodedValue {
    /// Returns the storage-level kind of this value.
    pub const fn kind(&self) -> PropertyValueKind {
        match self {
            Self::Boolean(_) => PropertyValueKind::Boolean,
            Self::Integer(_) => PropertyValueKind::Integer,
            Self::Decimal(_) => PropertyValueKind::Decimal,
            Self::Text(_) => PropertyValueKind::Text,
        }
    }

    const fn is_numeric(&self) -> bool {
        matches!(self, Self::Integer(_) | Self::Decimal(_))
    }
}

/// Source-reported confidence in a decoded value.
///
/// This status records protocol or device evidence only. Semantic validation,
/// timestamp policy, freshness, and calculator confidence remain later stages.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObservationQuality {
    /// The decoder found no source-level reason to distrust the value.
    Good,
    /// The source supplied a value with a warning or reduced confidence.
    Uncertain,
    /// The source explicitly marked the decoded value invalid.
    Invalid,
}

/// A decoded value with the source metadata needed before semantic binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedMeasurement {
    value: DecodedValue,
    source_unit: Option<UnitId>,
    quality: ObservationQuality,
}

impl DecodedMeasurement {
    /// Constructs decoded measurement data after validating its unit.
    ///
    /// # Errors
    ///
    /// Returns [`DecodedMeasurementError::UnitRequiresNumericValue`] when a
    /// source unit is attached to Boolean or text output.
    pub fn new(
        value: DecodedValue,
        source_unit: Option<UnitId>,
        quality: ObservationQuality,
    ) -> Result<Self, DecodedMeasurementError> {
        if source_unit.is_some() && !value.is_numeric() {
            return Err(DecodedMeasurementError::UnitRequiresNumericValue);
        }

        Ok(Self {
            value,
            source_unit,
            quality,
        })
    }

    /// Returns the typed value before conversion or semantic binding.
    pub const fn value(&self) -> &DecodedValue {
        &self.value
    }

    /// Returns the source unit declared by the protocol package, when present.
    pub const fn source_unit(&self) -> Option<&UnitId> {
        self.source_unit.as_ref()
    }

    /// Returns the source-reported quality.
    pub const fn quality(&self) -> ObservationQuality {
        self.quality
    }
}

/// Describes why decoded measurement metadata is structurally invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodedMeasurementError {
    /// Units are meaningful only for integer and decimal values.
    UnitRequiresNumericValue,
}

impl fmt::Display for DecodedMeasurementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a decoded source unit requires a numeric value")
    }
}

impl Error for DecodedMeasurementError {}
