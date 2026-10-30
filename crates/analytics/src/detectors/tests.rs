use super::*;
fn samples(values: &[&str]) -> Vec<Option<PropertyValue>> {
    values
        .iter()
        .map(|value| Some(PropertyValue::Decimal(value.parse().unwrap())))
        .collect()
}
#[test]
fn normal_spike_flat_and_threshold_equality_are_explained() {
    for (series, anomalous, median, mad) in [
        (vec!["9", "10", "11", "10"], false, "10", "1"),
        (vec!["9", "10", "11", "17"], true, "10", "1"),
        (vec!["9", "10", "11", "16"], false, "10", "1"),
        (vec!["10", "10", "10", "10"], false, "10", "0"),
        (vec!["10", "10", "10", "11"], true, "10", "0"),
        (vec!["-1", "0", "1", "-7"], true, "0", "1"),
    ] {
        let evidence = detect_mad(&samples(&series), 1).unwrap().unwrap();
        assert_eq!(evidence.anomalous, anomalous);
        assert_eq!(evidence.median, median.parse::<BigDecimal>().unwrap());
        assert_eq!(evidence.mad, mad.parse::<BigDecimal>().unwrap());
        assert_eq!(evidence.threshold, &evidence.mad * BigDecimal::from(6));
    }
}
#[test]
fn exact_midpoints_and_large_integers_preserve_precision() {
    let input = samples(&["0.1", "0.2", "0.3", "0.4", "2"]);
    let evidence = detect_mad(&input, 5).unwrap().unwrap();
    assert_eq!(evidence.median, "0.25".parse::<BigDecimal>().unwrap());
    assert_eq!(evidence.mad, "0.1".parse::<BigDecimal>().unwrap());
    assert!(evidence.anomalous);
    assert_eq!(detect_mad(&input, 5).unwrap(), Some(evidence));
    let mut integers = vec![Some(PropertyValue::Integer(i64::MAX)); 4];
    integers[3] = Some(PropertyValue::Integer(i64::MAX - 1));
    assert_eq!(
        detect_mad(&integers, 4).unwrap().unwrap().deviation,
        BigDecimal::from(1)
    );
}
#[test]
fn sparse_unusable_and_budget_excess_do_not_produce_healthy_flags() {
    assert_eq!(detect_mad(&[], 1), Ok(None));
    assert_eq!(detect_mad(&samples(&["1", "2", "3"]), 1), Ok(None));
    assert_eq!(detect_mad(&samples(&["1", "2", "3", "4"]), 5), Ok(None));
    let mut input = samples(&["1", "2", "3", "4"]);
    input[0] = None;
    assert_eq!(detect_mad(&input, 4), Ok(None));
    input[0] = Some(PropertyValue::Boolean(true));
    assert_eq!(
        detect_mad(&input, 4),
        Err(CalculatorExecutionFailure::IncompatibleInput)
    );
    for oversized in ["1e257".to_owned(), "1e-257".to_owned(), "9".repeat(1025)] {
        input[0] = Some(PropertyValue::Decimal(oversized.parse().unwrap()));
        assert_eq!(
            detect_mad(&input, 4),
            Err(CalculatorExecutionFailure::ResourceLimit)
        );
    }
    assert_eq!(
        detect_mad(&vec![Some(PropertyValue::Integer(1)); 1025], 1),
        Err(CalculatorExecutionFailure::ResourceLimit)
    );
    assert!(
        detect_mad(&vec![Some(PropertyValue::Integer(1)); 1024], 1024)
            .unwrap()
            .is_some()
    );
}
