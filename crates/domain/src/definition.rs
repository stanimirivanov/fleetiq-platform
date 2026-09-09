use std::{error::Error, fmt, num::NonZeroU32};

use crate::{AssetTypeId, PropertyDefinitionId, RelationshipTypeId, UnitId};

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

/// Defines the meaning of one immutable directed relationship type.
///
/// Endpoint type constraints, cardinality, and graph-shape rules require access
/// to definitions or relationships beyond this value. They belong to graph
/// validation rather than this definition aggregate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationshipTypeDefinition {
    reference: RelationshipTypeRef,
    name: DefinitionName,
}

impl RelationshipTypeDefinition {
    /// Constructs an immutable relationship-type definition.
    pub fn new(reference: RelationshipTypeRef, name: DefinitionName) -> Self {
        Self { reference, name }
    }

    /// Returns the identity and version of this immutable definition.
    pub fn reference(&self) -> &RelationshipTypeRef {
        &self.reference
    }

    /// Returns the operator-facing relationship-type name.
    pub fn name(&self) -> &DefinitionName {
        &self.name
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn version(value: u32) -> DefinitionVersion {
        DefinitionVersion::new(value).expect("positive definition version")
    }

    fn property_id(value: &str) -> PropertyDefinitionId {
        PropertyDefinitionId::new(value).expect("valid property definition ID")
    }

    fn property_reference(value: &str, version_number: u32) -> PropertyDefinitionRef {
        PropertyDefinitionRef::new(property_id(value), version(version_number))
    }

    fn type_reference(version_number: u32) -> AssetTypeRef {
        AssetTypeRef::new(
            AssetTypeId::new("generic.power-system").expect("valid asset type ID"),
            version(version_number),
        )
    }

    fn relationship_type_reference(version_number: u32) -> RelationshipTypeRef {
        RelationshipTypeRef::new(
            RelationshipTypeId::new("physical.contains").expect("valid relationship type ID"),
            version(version_number),
        )
    }

    #[test]
    fn definition_versions_start_at_one() {
        assert_eq!(DefinitionVersion::new(0), Err(DefinitionVersionError::Zero));
        assert_eq!(version(1).get(), 1);
    }

    #[test]
    fn defines_a_numeric_property_with_a_canonical_unit() {
        let definition = PropertyDefinition::new(
            property_reference("electrical.voltage", 1),
            DefinitionName::new("Voltage").expect("valid definition name"),
            PropertyValueKind::Decimal,
            Some(UnitId::new("V").expect("valid unit ID")),
        )
        .expect("numeric property may have a unit");

        assert_eq!(definition.id().as_str(), "electrical.voltage");
        assert_eq!(definition.version().get(), 1);
        assert_eq!(definition.name().as_str(), "Voltage");
        assert_eq!(definition.value_kind(), PropertyValueKind::Decimal);
        assert_eq!(definition.canonical_unit().map(AsRef::as_ref), Some("V"));
    }

    #[test]
    fn rejects_a_unit_on_a_non_numeric_property() {
        let result = PropertyDefinition::new(
            property_reference("equipment.running", 1),
            DefinitionName::new("Running").expect("valid definition name"),
            PropertyValueKind::Boolean,
            Some(UnitId::new("1").expect("valid unit ID")),
        );

        assert_eq!(
            result,
            Err(PropertyDefinitionError::UnitRequiresNumericValue)
        );
    }

    #[test]
    fn asset_type_declares_supported_properties_without_duplicates() {
        let mut definition = AssetTypeDefinition::new(
            type_reference(1),
            DefinitionName::new("Power system").expect("valid definition name"),
        );
        let voltage = property_reference("electrical.voltage", 1);

        definition
            .add_property(voltage.clone())
            .expect("new property should be accepted");

        assert!(definition.supports_property(voltage.id()));
        assert_eq!(
            definition.supported_properties(),
            std::slice::from_ref(&voltage)
        );
        assert_eq!(
            definition.add_property(voltage),
            Err(AssetTypeDefinitionError::DuplicateProperty)
        );
    }

    #[test]
    fn asset_type_versions_can_support_different_properties() {
        let mut first = AssetTypeDefinition::new(
            type_reference(1),
            DefinitionName::new("Power system").expect("valid definition name"),
        );
        let mut second = AssetTypeDefinition::new(
            type_reference(2),
            DefinitionName::new("Power system").expect("valid definition name"),
        );
        let voltage = property_reference("electrical.voltage", 1);
        let temperature = property_reference("thermal.temperature", 2);

        first
            .add_property(voltage.clone())
            .expect("new property should be accepted");
        second
            .add_property(voltage)
            .expect("new property should be accepted");
        second
            .add_property(temperature.clone())
            .expect("new property should be accepted");

        assert!(!first.supports_property(temperature.id()));
        assert!(second.supports_property(temperature.id()));
    }

    #[test]
    fn asset_type_pins_the_property_definition_version() {
        let mut definition = AssetTypeDefinition::new(
            type_reference(1),
            DefinitionName::new("Power system").expect("valid definition name"),
        );
        let voltage_v2 = property_reference("electrical.voltage", 2);

        definition
            .add_property(voltage_v2)
            .expect("new property should be accepted");

        assert_eq!(definition.supported_properties()[0].version().get(), 2);
    }

    #[test]
    fn rejects_invalid_definition_names() {
        assert_eq!(DefinitionName::new(""), Err(DefinitionNameError::Empty));
        assert_eq!(
            DefinitionName::new(" Temperature"),
            Err(DefinitionNameError::SurroundingWhitespace)
        );
        assert_eq!(
            DefinitionName::new("Temperature\nreading"),
            Err(DefinitionNameError::ControlCharacter)
        );
    }

    #[test]
    fn relationship_type_definitions_are_versioned() {
        let definition = RelationshipTypeDefinition::new(
            relationship_type_reference(2),
            DefinitionName::new("Contains").expect("valid definition name"),
        );

        assert_eq!(definition.reference().id().as_str(), "physical.contains");
        assert_eq!(definition.reference().version().get(), 2);
        assert_eq!(definition.name().as_str(), "Contains");
    }
}
