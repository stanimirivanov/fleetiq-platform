use std::{error::Error, fmt, str::FromStr};

const MAX_ANALYTICS_IDENTIFIER_LENGTH: usize = 128;

/// Identifies a calculator independently of any implementation release.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CalculatorId(String);

/// Identifies one immutable release of a calculator.
///
/// The version is opaque so deployments may use semantic versions or
/// content-derived versions without changing the output contract.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CalculatorVersion(String);

/// Identifies one logical calculator evaluation across retries.
///
/// The orchestrating host must reuse this identity when replaying the same
/// calculator, scope, event-time trigger, configuration, and input selection.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CalculatorEvaluationId(String);

/// Identifies one immutable configuration including input roles and parameters.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CalculatorConfigurationId(String);

/// Identifies one exact immutable semantic evidence record.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CalculatorEvidenceId(String);

/// Describes why an analytics identifier is invalid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalyticsIdentifierError {
    /// The supplied value is empty or contains only whitespace.
    Empty,
    /// The supplied value begins or ends with whitespace.
    SurroundingWhitespace,
    /// The supplied value exceeds the analytics identifier size limit.
    TooLong {
        /// The maximum accepted Unicode scalar-value count.
        maximum: usize,
        /// The Unicode scalar-value count in the rejected value.
        actual: usize,
    },
    /// The supplied value contains a control character.
    ControlCharacter,
}

impl fmt::Display for AnalyticsIdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("analytics identifier must not be empty"),
            Self::SurroundingWhitespace => {
                formatter.write_str("analytics identifier must not have surrounding whitespace")
            }
            Self::TooLong { maximum, actual } => write!(
                formatter,
                "analytics identifier contains {actual} characters; the maximum is {maximum}"
            ),
            Self::ControlCharacter => {
                formatter.write_str("analytics identifier must not contain control characters")
            }
        }
    }
}

impl Error for AnalyticsIdentifierError {}

macro_rules! analytics_identifier {
    ($type:ident) => {
        impl $type {
            /// Constructs an identifier after validating its analytics invariants.
            ///
            /// # Errors
            ///
            /// Returns [`AnalyticsIdentifierError`] when `value` is empty, has
            /// surrounding whitespace, contains a control character, or exceeds
            /// 128 Unicode scalar values.
            pub fn new(value: impl Into<String>) -> Result<Self, AnalyticsIdentifierError> {
                let value = value.into();
                validate(&value)?;
                Ok(Self(value))
            }

            /// Returns the identifier as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the identifier and returns its text representation.
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl AsRef<str> for $type {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $type {
            type Err = AnalyticsIdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

analytics_identifier!(CalculatorConfigurationId);
analytics_identifier!(CalculatorEvidenceId);
analytics_identifier!(CalculatorId);
analytics_identifier!(CalculatorVersion);
analytics_identifier!(CalculatorEvaluationId);

fn validate(value: &str) -> Result<(), AnalyticsIdentifierError> {
    if value.trim().is_empty() {
        return Err(AnalyticsIdentifierError::Empty);
    }
    if value.trim() != value {
        return Err(AnalyticsIdentifierError::SurroundingWhitespace);
    }
    if value.chars().any(char::is_control) {
        return Err(AnalyticsIdentifierError::ControlCharacter);
    }

    let actual = value.chars().count();
    if actual > MAX_ANALYTICS_IDENTIFIER_LENGTH {
        return Err(AnalyticsIdentifierError::TooLong {
            maximum: MAX_ANALYTICS_IDENTIFIER_LENGTH,
            actual,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_opaque_calculator_versions() {
        let version = CalculatorVersion::new("2026.09-threshold.2")
            .expect("calculator version should be valid");

        assert_eq!(version.as_str(), "2026.09-threshold.2");
    }

    #[test]
    fn rejects_ambiguous_analytics_identifiers() {
        assert_eq!(
            CalculatorId::new(" calculator"),
            Err(AnalyticsIdentifierError::SurroundingWhitespace)
        );
        assert_eq!(
            CalculatorEvaluationId::new("evaluation\n1"),
            Err(AnalyticsIdentifierError::ControlCharacter)
        );
    }
}
