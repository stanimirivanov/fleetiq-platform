use bigdecimal::BigDecimal;

use crate::PropertyValueKind;

/// A typed value held by an asset property.
///
/// Protocol observations and calculated results share this representation once
/// their vendor-specific wire encoding has been interpreted. A property
/// definition supplies the value's semantic meaning and canonical unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PropertyValue {
    /// A Boolean value.
    Boolean(bool),
    /// A signed whole number.
    Integer(i64),
    /// An arbitrary-precision decimal number.
    Decimal(BigDecimal),
    /// Human-readable or protocol-defined text.
    Text(String),
}

impl PropertyValue {
    /// Returns the storage-level kind of this value.
    pub const fn kind(&self) -> PropertyValueKind {
        match self {
            Self::Boolean(_) => PropertyValueKind::Boolean,
            Self::Integer(_) => PropertyValueKind::Integer,
            Self::Decimal(_) => PropertyValueKind::Decimal,
            Self::Text(_) => PropertyValueKind::Text,
        }
    }

    /// Reports whether this value can carry a unit.
    pub const fn is_numeric(&self) -> bool {
        matches!(self, Self::Integer(_) | Self::Decimal(_))
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn reports_storage_kinds_and_numeric_capability() {
        let values = [
            (
                PropertyValue::Boolean(true),
                PropertyValueKind::Boolean,
                false,
            ),
            (PropertyValue::Integer(7), PropertyValueKind::Integer, true),
            (
                PropertyValue::Decimal(BigDecimal::from_str("7.25").expect("valid decimal")),
                PropertyValueKind::Decimal,
                true,
            ),
            (
                PropertyValue::Text("ready".to_owned()),
                PropertyValueKind::Text,
                false,
            ),
        ];

        for (value, expected_kind, expected_numeric) in values {
            assert_eq!(value.kind(), expected_kind);
            assert_eq!(value.is_numeric(), expected_numeric);
        }
    }
}
