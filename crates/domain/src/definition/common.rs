use std::{error::Error, fmt, num::NonZeroU32};

use crate::{AssetTypeId, PropertyDefinitionId, RelationshipTypeId};

const MAX_DEFINITION_NAME_LENGTH: usize = 200;

/// A positive version of a semantic definition.
///
/// Versions make interpretation reproducible: an asset or observation can point
/// to the exact definition that was active instead of inheriting later edits.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DefinitionVersion(NonZeroU32);

impl DefinitionVersion {
    /// Constructs a definition version.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionVersionError`] when `value` is zero. Version numbering
    /// starts at one.
    pub fn new(value: u32) -> Result<Self, DefinitionVersionError> {
        NonZeroU32::new(value)
            .map(Self)
            .ok_or(DefinitionVersionError::Zero)
    }

    /// Returns the numeric version.
    pub fn get(self) -> u32 {
        self.0.get()
    }
}

/// Describes why a definition version could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefinitionVersionError {
    /// Version zero is reserved as an invalid/uninitialized value.
    Zero,
}

impl fmt::Display for DefinitionVersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("definition version must be greater than zero")
    }
}

impl Error for DefinitionVersionError {}

/// References one immutable version of an asset-type definition.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AssetTypeRef {
    id: AssetTypeId,
    version: DefinitionVersion,
}

impl AssetTypeRef {
    /// Constructs a reference to an asset-type definition.
    pub fn new(id: AssetTypeId, version: DefinitionVersion) -> Self {
        Self { id, version }
    }

    /// Returns the stable asset-type identity.
    pub fn id(&self) -> &AssetTypeId {
        &self.id
    }

    /// Returns the referenced immutable version.
    pub fn version(&self) -> DefinitionVersion {
        self.version
    }
}

/// References one immutable version of a property definition.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PropertyDefinitionRef {
    id: PropertyDefinitionId,
    version: DefinitionVersion,
}

impl PropertyDefinitionRef {
    /// Constructs a reference to a property definition.
    pub fn new(id: PropertyDefinitionId, version: DefinitionVersion) -> Self {
        Self { id, version }
    }

    /// Returns the stable property-definition identity.
    pub fn id(&self) -> &PropertyDefinitionId {
        &self.id
    }

    /// Returns the referenced immutable version.
    pub fn version(&self) -> DefinitionVersion {
        self.version
    }
}

/// References one immutable version of a relationship-type definition.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RelationshipTypeRef {
    id: RelationshipTypeId,
    version: DefinitionVersion,
}

impl RelationshipTypeRef {
    /// Constructs a reference to a relationship-type definition.
    pub fn new(id: RelationshipTypeId, version: DefinitionVersion) -> Self {
        Self { id, version }
    }

    /// Returns the stable relationship-type identity.
    pub fn id(&self) -> &RelationshipTypeId {
        &self.id
    }

    /// Returns the referenced immutable version.
    pub fn version(&self) -> DefinitionVersion {
        self.version
    }
}

/// A human-readable name for an asset type or property definition.
///
/// Definition names are labels and are not stable identities. The corresponding
/// strongly typed ID supplies identity across renames.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DefinitionName(String);

impl DefinitionName {
    /// Constructs a validated definition name.
    ///
    /// # Errors
    ///
    /// Returns [`DefinitionNameError`] when the name is empty, has surrounding
    /// whitespace, contains a control character, or exceeds 200 Unicode scalar
    /// values.
    pub fn new(value: impl Into<String>) -> Result<Self, DefinitionNameError> {
        let value = value.into();
        validate_name(&value)?;
        Ok(Self(value))
    }

    /// Returns the definition name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the name and returns its text representation.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl AsRef<str> for DefinitionName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for DefinitionName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Describes why a definition name could not be constructed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DefinitionNameError {
    /// The supplied name is empty or contains only whitespace.
    Empty,
    /// The supplied name begins or ends with whitespace.
    SurroundingWhitespace,
    /// The supplied name exceeds the supported size.
    TooLong {
        /// The maximum accepted Unicode scalar-value count.
        maximum: usize,
        /// The Unicode scalar-value count in the rejected name.
        actual: usize,
    },
    /// The supplied name contains a control character.
    ControlCharacter,
}

impl fmt::Display for DefinitionNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("definition name must not be empty"),
            Self::SurroundingWhitespace => {
                formatter.write_str("definition name must not have surrounding whitespace")
            }
            Self::TooLong { maximum, actual } => write!(
                formatter,
                "definition name contains {actual} characters; the maximum is {maximum}"
            ),
            Self::ControlCharacter => {
                formatter.write_str("definition name must not contain control characters")
            }
        }
    }
}

impl Error for DefinitionNameError {}

fn validate_name(value: &str) -> Result<(), DefinitionNameError> {
    if value.trim().is_empty() {
        return Err(DefinitionNameError::Empty);
    }
    if value.trim() != value {
        return Err(DefinitionNameError::SurroundingWhitespace);
    }
    if value.chars().any(char::is_control) {
        return Err(DefinitionNameError::ControlCharacter);
    }

    let actual = value.chars().count();
    if actual > MAX_DEFINITION_NAME_LENGTH {
        return Err(DefinitionNameError::TooLong {
            maximum: MAX_DEFINITION_NAME_LENGTH,
            actual,
        });
    }

    Ok(())
}
