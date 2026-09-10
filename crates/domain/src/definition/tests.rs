use super::*;
use crate::{AssetTypeId, PropertyDefinitionId, RelationshipTypeId, UnitId};

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
