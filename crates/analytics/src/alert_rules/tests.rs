use super::*;
use crate::{CalculatorId, CalculatorVersion};
use fleetiq_domain::{DefinitionVersion, PropertyDefinitionId};
fn definition() -> AlertRuleDefinition {
    AlertRuleDefinition {
        calculator: CalculatorRef::new(
            CalculatorId::new("mean").unwrap(),
            CalculatorVersion::new("1").unwrap(),
        ),
        scope: AssetId::new("a").unwrap(),
        target: AssetId::new("b").unwrap(),
        output: PropertyDefinitionRef::new(
            PropertyDefinitionId::new("mean").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
        output_index: 0,
        comparison: AlertComparison::Greater,
        threshold: "9007199254740993.001".parse().unwrap(),
        unit: None,
        hold_ms: 0,
        resolution: AlertResolution::OnFalse,
        resolve_after_ms: 0,
    }
}
#[test]
fn predicate_is_exact_and_missing_is_separate_from_false() {
    let mut rule = definition();
    assert!(rule.is_valid());
    let equal = rule.threshold.clone();
    let higher = "9007199254740993.002".parse().unwrap();
    assert_eq!(rule.condition(None), AlertCondition::Missing);
    for (name, equal_matches, higher_matches) in [
        ("gt", false, true),
        ("gte", true, true),
        ("lt", false, false),
        ("lte", true, false),
    ] {
        rule.comparison = AlertComparison::parse(name).unwrap();
        assert_eq!(rule.comparison.as_str(), name);
        assert_eq!(
            rule.condition(Some(&equal)),
            if equal_matches {
                AlertCondition::Matched
            } else {
                AlertCondition::NotMatched
            }
        );
        assert_eq!(
            rule.condition(Some(&higher)),
            if higher_matches {
                AlertCondition::Matched
            } else {
                AlertCondition::NotMatched
            }
        );
    }
    assert_eq!(AlertComparison::parse("value > threshold"), None);
}
#[test]
fn bounds_and_normalized_precision_are_checked_without_expansion() {
    let mut rule = definition();
    for bad in ["1e128", "1e-65", "1e999999"] {
        rule.threshold = bad.parse().unwrap();
        assert!(!rule.is_valid(), "{bad}");
    }
    for valid in ["1e127", "1e-64", "1.000", "-0"] {
        rule.threshold = valid.parse().unwrap();
        assert!(rule.is_valid(), "{valid}");
    }
    rule.hold_ms = 604_800_001;
    assert!(!rule.is_valid());
    rule.hold_ms = 0;
    rule.resolve_after_ms = 604_800_001;
    assert!(!rule.is_valid());
}
