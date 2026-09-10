use std::{error::Error, fmt, str::FromStr};

const MAX_PIPELINE_IDENTIFIER_LENGTH: usize = 128;

/// Identifies one raw message after the host has accepted it durably.
///
/// The host assigns this identity before decoding and preserves it across
/// retries. Decoder output uses it as the stable basis for deduplication.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AcceptedMessageId(String);

/// Identifies a protocol package independently of any release of that package.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProtocolPackageId(String);

/// Identifies one immutable release of a protocol package.
///
/// The value is deliberately opaque at this boundary. Package distribution can
/// adopt semantic versions or content-derived versions without changing the
/// decoded-observation contract.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProtocolPackageVersion(String);

/// Describes why a telemetry pipeline identifier is invalid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PipelineIdentifierError {
    /// The supplied value is empty or contains only whitespace.
    Empty,
    /// The supplied value begins or ends with whitespace.
    SurroundingWhitespace,
    /// The supplied value exceeds the pipeline identifier size limit.
    TooLong {
        /// The maximum accepted Unicode scalar-value count.
        maximum: usize,
        /// The Unicode scalar-value count in the rejected value.
        actual: usize,
    },
    /// The supplied value contains a control character.
    ControlCharacter,
}

impl fmt::Display for PipelineIdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("pipeline identifier must not be empty"),
            Self::SurroundingWhitespace => {
                formatter.write_str("pipeline identifier must not have surrounding whitespace")
            }
            Self::TooLong { maximum, actual } => write!(
                formatter,
                "pipeline identifier contains {actual} characters; the maximum is {maximum}"
            ),
            Self::ControlCharacter => {
                formatter.write_str("pipeline identifier must not contain control characters")
            }
        }
    }
}

impl Error for PipelineIdentifierError {}

macro_rules! pipeline_identifier {
    ($type:ident) => {
        impl $type {
            /// Constructs an identifier after validating its pipeline invariants.
            ///
            /// # Errors
            ///
            /// Returns [`PipelineIdentifierError`] when `value` is empty, has
            /// surrounding whitespace, contains a control character, or exceeds
            /// 128 Unicode scalar values.
            pub fn new(value: impl Into<String>) -> Result<Self, PipelineIdentifierError> {
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
            type Err = PipelineIdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

pipeline_identifier!(AcceptedMessageId);
pipeline_identifier!(ProtocolPackageId);
pipeline_identifier!(ProtocolPackageVersion);

fn validate(value: &str) -> Result<(), PipelineIdentifierError> {
    if value.trim().is_empty() {
        return Err(PipelineIdentifierError::Empty);
    }
    if value.trim() != value {
        return Err(PipelineIdentifierError::SurroundingWhitespace);
    }
    if value.chars().any(char::is_control) {
        return Err(PipelineIdentifierError::ControlCharacter);
    }

    let actual = value.chars().count();
    if actual > MAX_PIPELINE_IDENTIFIER_LENGTH {
        return Err(PipelineIdentifierError::TooLong {
            maximum: MAX_PIPELINE_IDENTIFIER_LENGTH,
            actual,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_opaque_package_versions() {
        let version = ProtocolPackageVersion::new("2026.09-vendor.3")
            .expect("package version should be valid");

        assert_eq!(version.as_str(), "2026.09-vendor.3");
    }

    #[test]
    fn rejects_ambiguous_pipeline_identifiers() {
        assert_eq!(
            AcceptedMessageId::new(" message-1"),
            Err(PipelineIdentifierError::SurroundingWhitespace)
        );
        assert_eq!(
            ProtocolPackageId::new("protocol\npackage"),
            Err(PipelineIdentifierError::ControlCharacter)
        );
    }
}
