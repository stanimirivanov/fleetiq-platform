use super::*;
use crate::{
    CalculatorEvaluationId, CalculatorEvidenceId, CalculatorId, CalculatorInput, CalculatorRef,
    CalculatorVersion,
};
use fleetiq_domain::{
    DefinitionName, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef,
    PropertyValueKind, TenantId,
};
fn prepared(values: &[&str], minimum: u16) -> PreparedCalculatorEvaluation {
    let time = UnixTimeMillis::new;
    let window = CalculatorWindow::new(time(0), time(100), time(100)).unwrap();
    let request = CalculatorRunRequest {
        evaluation: CalculatorEvaluation::new(
            CalculatorEvaluationId::new("e").unwrap(),
            TenantId::new("t").unwrap(),
            AssetId::new("a").unwrap(),
            CalculatorRef::new(
                CalculatorId::new("mean").unwrap(),
                CalculatorVersion::new("1").unwrap(),
            ),
            time(100),
            time(101),
        ),
        configuration: CalculatorConfigurationId::new("c").unwrap(),
        window,
        known_at: time(100),
        minimum_samples: minimum,
        target: AssetId::new("a").unwrap(),
    };
    PreparedCalculatorEvaluation {
        request,
        manifest: CalculatorInputManifest::new(
            window,
            time(100),
            values
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    CalculatorInput::new(
                        CalculatorEvidenceId::new(format!("fact-{i}")).unwrap(),
                        time(i64::try_from(i).unwrap()),
                    )
                })
                .collect(),
        )
        .unwrap(),
        samples: values
            .iter()
            .map(|v| Some(PropertyValue::Decimal(v.parse().unwrap())))
            .collect(),
        output: PropertyDefinition::new(
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("mean").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
            DefinitionName::new("Mean").unwrap(),
            PropertyValueKind::Decimal,
            None,
        )
        .unwrap(),
    }
}
#[test]
fn mean_is_exact_until_explicit_signed_truncation() {
    for (input, expected) in [
        (vec!["1", "2"], "1.5"),
        (vec!["1", "0", "0"], "0.333333333333333333"),
        (vec!["-1", "0", "0"], "-0.333333333333333333"),
    ] {
        let record = calculate_mean(prepared(&input, 1)).unwrap();
        assert_eq!(record.outcome(), CalculatorExecutionOutcome::Complete);
        assert_eq!(
            record.outputs()[0].property().value(),
            &PropertyValue::Decimal(expected.parse().unwrap())
        );
        assert_eq!(record.outputs()[0].id().output_index(), 0);
    }
}
#[test]
fn missing_unusable_and_resource_limited_inputs_never_become_healthy_values() {
    for selection in [prepared(&[], 1), prepared(&["1"], 2)] {
        let record = calculate_mean(selection).unwrap();
        assert_eq!(
            record.outcome(),
            CalculatorExecutionOutcome::InsufficientEvidence
        );
        assert_eq!(record.outputs(), []);
    }
    let mut unusable = prepared(&["1"], 1);
    unusable.samples[0] = None;
    assert_eq!(
        calculate_mean(unusable).unwrap().outcome(),
        CalculatorExecutionOutcome::InsufficientEvidence
    );
    assert_eq!(
        calculate_mean(prepared(&["1e1000"], 1)).unwrap().outcome(),
        CalculatorExecutionOutcome::Failed(CalculatorExecutionFailure::ResourceLimit)
    );
    let mut mismatch = prepared(&["1"], 1);
    mismatch.samples.clear();
    assert_eq!(calculate_mean(mismatch), Err(CalculatorRunError::Context));
}
