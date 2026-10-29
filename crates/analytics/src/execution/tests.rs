use super::*;
use crate::{CalculatorId, CalculatorVersion};
fn t(value: i64) -> UnixTimeMillis {
    UnixTimeMillis::new(value)
}
fn window() -> CalculatorWindow {
    CalculatorWindow::new(t(0), t(100), t(120)).unwrap()
}
fn input(id: &str, time: i64) -> CalculatorInput {
    CalculatorInput::new(CalculatorEvidenceId::new(id).unwrap(), t(time))
}
fn manifest(inputs: Vec<CalculatorInput>) -> CalculatorInputManifest {
    CalculatorInputManifest::new(window(), t(120), inputs).unwrap()
}
fn key(manifest: CalculatorInputManifest) -> CalculatorExecutionKey {
    CalculatorExecutionKey::new(
        TenantId::new("tenant-a").unwrap(),
        AssetId::new("asset-a").unwrap(),
        CalculatorRef::new(
            CalculatorId::new("mean").unwrap(),
            CalculatorVersion::new("v1").unwrap(),
        ),
        CalculatorConfigurationId::new("config-v1").unwrap(),
        manifest,
    )
}
#[test]
fn window_bounds_and_arithmetic_do_not_wrap() {
    for (start, end, close) in [
        (0, 0, 0),
        (2, 1, 3),
        (0, 1, 0),
        (i64::MIN, i64::MAX, i64::MAX),
        (
            0,
            MAX_CALCULATOR_WINDOW_MILLIS + 1,
            MAX_CALCULATOR_WINDOW_MILLIS + 1,
        ),
        (0, 1, i64::MAX),
    ] {
        assert_eq!(
            CalculatorWindow::new(t(start), t(end), t(close)),
            Err(ExecutionContractError::InvalidWindow)
        );
    }
    let w = CalculatorWindow::new(t(-100), t(0), t(0)).unwrap();
    assert_eq!(w.start(), t(-100));
    assert_eq!(w.end(), t(0));
    assert_eq!(w.finalize_at(), t(0));
    assert!(
        CalculatorWindow::new(
            t(0),
            t(MAX_CALCULATOR_WINDOW_MILLIS),
            t(2 * MAX_CALCULATOR_WINDOW_MILLIS)
        )
        .is_ok()
    );
}
#[test]
fn manifests_reject_partial_or_ambiguous_selection() {
    assert_eq!(
        CalculatorInputManifest::new(window(), t(119), vec![]),
        Err(ExecutionContractError::PrematureSnapshot)
    );
    for time in [-1, 100] {
        assert_eq!(
            CalculatorInputManifest::new(window(), t(120), vec![input("a", time)]),
            Err(ExecutionContractError::OutsideWindow)
        );
    }
    assert_eq!(
        CalculatorInputManifest::new(window(), t(120), vec![input("a", 0), input("a", 1)]),
        Err(ExecutionContractError::DuplicateEvidence)
    );
    let inputs: Vec<_> = (0..MAX_CALCULATOR_INPUTS)
        .map(|i| input(&format!("fact-{i}"), 0))
        .collect();
    assert!(CalculatorInputManifest::new(window(), t(120), inputs.clone()).is_ok());
    let mut overflow = inputs;
    overflow.push(input("overflow", 0));
    assert_eq!(
        CalculatorInputManifest::new(window(), t(120), overflow),
        Err(ExecutionContractError::TooManyInputs)
    );
    assert_eq!(manifest(vec![]).inputs(), []);
}
#[test]
fn canonical_order_makes_retry_keys_independent_of_query_order() {
    let selected = manifest(vec![input("z", 20), input("b", 0), input("a", 0)]);
    let replay = manifest(vec![input("a", 0), input("z", 20), input("b", 0)]);
    assert_eq!(
        selected
            .inputs()
            .iter()
            .map(|i| i.id().as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b", "z"]
    );
    assert_eq!(selected.inputs()[0].event_at(), t(0));
    assert_eq!(selected.known_at(), t(120));
    assert_eq!(key(selected), key(replay));
}
#[test]
fn changes_to_any_logical_selection_dimension_require_new_identity() {
    let original = key(manifest(vec![input("a", 0)]));
    let mut variants = vec![];
    let mut changed = original.clone();
    changed.tenant = TenantId::new("tenant-b").unwrap();
    variants.push(changed);
    let mut changed = original.clone();
    changed.scope = AssetId::new("asset-b").unwrap();
    variants.push(changed);
    let mut changed = original.clone();
    changed.calculator = CalculatorRef::new(
        CalculatorId::new("mean").unwrap(),
        CalculatorVersion::new("v2").unwrap(),
    );
    variants.push(changed);
    let mut changed = original.clone();
    changed.configuration = CalculatorConfigurationId::new("config-v2").unwrap();
    variants.push(changed);
    variants.push(key(manifest(vec![input("b", 0)])));
    variants.push(key(manifest(vec![input("a", 1)])));
    variants.push(key(CalculatorInputManifest::new(
        window(),
        t(121),
        vec![input("a", 0)],
    )
    .unwrap()));
    variants.push(key(CalculatorInputManifest::new(
        CalculatorWindow::new(t(0), t(101), t(120)).unwrap(),
        t(120),
        vec![input("a", 0)],
    )
    .unwrap()));
    for changed in variants {
        assert_ne!(original, changed);
    }
    assert_eq!(original.tenant().as_str(), "tenant-a");
    assert_eq!(original.scope().as_str(), "asset-a");
    assert_eq!(original.calculator().version().as_str(), "v1");
    assert_eq!(original.configuration().as_str(), "config-v1");
    assert_eq!(original.manifest().window(), window());
}
#[test]
fn identity_validation_and_terminal_classifications_are_explicit() {
    assert!(CalculatorConfigurationId::new(" bad").is_err());
    assert!(CalculatorEvidenceId::new("x".repeat(129)).is_err());
    assert_ne!(
        CalculatorExecutionOutcome::Complete,
        CalculatorExecutionOutcome::InsufficientEvidence
    );
    assert_ne!(
        CalculatorExecutionOutcome::Failed(CalculatorExecutionFailure::ResourceLimit),
        CalculatorExecutionOutcome::InsufficientEvidence
    );
}
