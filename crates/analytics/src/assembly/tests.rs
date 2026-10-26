use fleetiq_domain::{
    AssetId, DefinitionName, DefinitionVersion, PropertyDefinition, PropertyDefinitionId,
    PropertyDefinitionRef, PropertyValue, PropertyValueKind, TenantId, UnitId, UnixTimeMillis,
};

use super::{assemble_output, assemble_outputs};
use crate::{
    CalculatedProperty, CalculatedPropertyError, CalculatorEvaluation, CalculatorEvaluationId,
    CalculatorId, CalculatorRef, CalculatorVersion,
};

#[test]
fn assembly_adds_host_owned_context_and_deterministic_indices() {
    let evaluation = evaluation();
    let outputs = assemble_outputs(
        &evaluation,
        [property("anomaly.score", 17), property("health.index", 92)],
    )
    .expect("two outputs should fit the output index");

    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].id().output_index(), 0);
    assert_eq!(outputs[1].id().output_index(), 1);
    for output in outputs {
        assert_eq!(output.id().evaluation_id(), evaluation.id());
        assert_eq!(output.id().produced_by(), evaluation.calculator());
        assert_eq!(output.evaluation(), &evaluation);
    }
}

#[test]
fn output_preserves_scope_times_target_value_and_unit() {
    let evaluation = evaluation();
    let output = assemble_output(&evaluation, 4, property("health.index", 92));

    assert_eq!(output.evaluation().tenant_id().as_str(), "tenant-a");
    assert_eq!(
        output.evaluation().scope_asset_id().as_str(),
        "locomotive-1"
    );
    assert_eq!(output.evaluation().event_at(), instant(2_000));
    assert_eq!(output.evaluation().evaluated_at(), instant(2_050));
    assert_eq!(output.property().asset_id().as_str(), "motor-1");
    assert_eq!(output.property().property().id().as_str(), "health.index");
    assert_eq!(output.property().value(), &PropertyValue::Integer(92));
    assert_eq!(
        output.property().unit().map(UnitId::as_str),
        Some("percent")
    );
    assert_eq!(output.id().output_index(), 4);
}

#[test]
fn empty_evaluation_can_produce_no_outputs() {
    let outputs = assemble_outputs(&evaluation(), []).expect("empty output should be valid");

    assert_eq!(outputs.len(), 0);
}

#[test]
fn calculated_values_must_match_the_exact_property_definition() {
    let definition = definition("operating.state", PropertyValueKind::Text, None);
    let result = CalculatedProperty::new(
        AssetId::new("motor-1").expect("valid asset ID"),
        &definition,
        PropertyValue::Text("ready".to_owned()),
        Some(UnitId::new("percent").expect("valid unit ID")),
    );

    assert_eq!(
        result,
        Err(CalculatedPropertyError::CanonicalUnitMismatch {
            actual: Some(UnitId::new("percent").expect("valid unit ID")),
            expected: None,
        })
    );
}

#[test]
fn calculated_value_kind_must_match_the_exact_property_definition() {
    let definition = definition("health.index", PropertyValueKind::Integer, None);
    let result = CalculatedProperty::new(
        AssetId::new("motor-1").expect("valid asset ID"),
        &definition,
        PropertyValue::Text("healthy".to_owned()),
        None,
    );

    assert_eq!(
        result,
        Err(CalculatedPropertyError::ValueKindMismatch {
            actual: PropertyValueKind::Text,
            expected: PropertyValueKind::Integer,
        })
    );
}

fn evaluation() -> CalculatorEvaluation {
    CalculatorEvaluation::new(
        CalculatorEvaluationId::new("evaluation-1").expect("valid evaluation ID"),
        TenantId::new("tenant-a").expect("valid tenant ID"),
        AssetId::new("locomotive-1").expect("valid scope asset ID"),
        CalculatorRef::new(
            CalculatorId::new("equipment.health").expect("valid calculator ID"),
            CalculatorVersion::new("2").expect("valid calculator version"),
        ),
        instant(2_000),
        instant(2_050),
    )
}

fn property(property_id: &str, value: i64) -> CalculatedProperty {
    let definition = definition(
        property_id,
        PropertyValueKind::Integer,
        Some(UnitId::new("percent").expect("valid unit ID")),
    );
    CalculatedProperty::new(
        AssetId::new("motor-1").expect("valid asset ID"),
        &definition,
        PropertyValue::Integer(value),
        Some(UnitId::new("percent").expect("valid unit ID")),
    )
    .expect("numeric calculated property should be valid")
}

fn definition(
    id: &str,
    value_kind: PropertyValueKind,
    canonical_unit: Option<UnitId>,
) -> PropertyDefinition {
    PropertyDefinition::new(
        property_reference(id),
        DefinitionName::new(id).expect("valid property name"),
        value_kind,
        canonical_unit,
    )
    .expect("test property definition should be valid")
}

fn property_reference(id: &str) -> PropertyDefinitionRef {
    PropertyDefinitionRef::new(
        PropertyDefinitionId::new(id).expect("valid property ID"),
        DefinitionVersion::new(1).expect("valid definition version"),
    )
}

const fn instant(value: i64) -> UnixTimeMillis {
    UnixTimeMillis::new(value)
}
