//! Exact arithmetic and fixed bucket semantics precede persistence.
use fleetiq_domain::{PropertyValue, PropertyValueKind, UnitId};
use fleetiq_history::downsampling::{NumericSample, NumericSpec, SummaryFailure, summarize};
use fleetiq_telemetry::ObservationQuality;
fn sample(id: &str, event: i64, value: PropertyValue) -> NumericSample {
    NumericSample {
        identity: id.into(),
        event_at_ms: event,
        value,
        unit: Some(UnitId::new("volt").unwrap()),
        quality: ObservationQuality::Good,
    }
}
#[test]
fn signed_buckets_preserve_exact_extremes_and_exclude_bad_quality() {
    let spec = NumericSpec::new(-10, 20, 10).unwrap();
    let a = sample("a", -10, PropertyValue::Integer(i64::MAX));
    let b = sample("b", -1, PropertyValue::Integer(i64::MAX));
    let mut excluded = sample("c", 0, PropertyValue::Integer(i64::MIN));
    excluded.quality = ObservationQuality::Invalid;
    let result = summarize(
        spec,
        PropertyValueKind::Integer,
        Some(&UnitId::new("volt").unwrap()),
        &[a.clone(), a, b, excluded],
    )
    .unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].count, 2);
    assert_eq!(result[0].sum.to_string(), "18446744073709551614");
    assert_eq!(result[1].count, 0);
    assert_eq!(result[1].excluded, 1);
    assert_eq!(result[2].total, 0);
    assert!(result[2].minimum.is_none());
}
#[test]
fn decimal_sums_remain_exact_and_invalid_context_fails_closed() {
    let spec = NumericSpec::new(0, 10, 10).unwrap();
    let a = sample(
        "a",
        0,
        PropertyValue::Decimal("9007199254740993.00000000000000000001".parse().unwrap()),
    );
    let b = sample(
        "b",
        9,
        PropertyValue::Decimal("0.00000000000000000002".parse().unwrap()),
    );
    let unit = UnitId::new("volt").unwrap();
    let result = summarize(
        spec,
        PropertyValueKind::Decimal,
        Some(&unit),
        &[a.clone(), b],
    )
    .unwrap();
    assert_eq!(
        result[0].sum.to_string(),
        "9007199254740993.00000000000000000003"
    );
    let mut foreign = a.clone();
    foreign.unit = None;
    assert_eq!(
        summarize(spec, PropertyValueKind::Decimal, Some(&unit), &[foreign]),
        Err(SummaryFailure::InvalidEvidence)
    );
    let mut collision = a.clone();
    collision.event_at_ms = 1;
    assert_eq!(
        summarize(
            spec,
            PropertyValueKind::Decimal,
            Some(&unit),
            &[a, collision]
        ),
        Err(SummaryFailure::InvalidEvidence)
    );
    assert_eq!(
        summarize(
            spec,
            PropertyValueKind::Decimal,
            Some(&unit),
            &[sample("mixed", 0, PropertyValue::Integer(1))]
        ),
        Err(SummaryFailure::InvalidEvidence)
    );
    assert!(summarize(spec, PropertyValueKind::Boolean, None, &[]).is_err());
    assert!(
        summarize(
            spec,
            PropertyValueKind::Decimal,
            Some(&unit),
            &[sample(
                "huge",
                0,
                PropertyValue::Decimal("1e1000".parse().unwrap())
            )]
        )
        .is_err()
    );
}
#[test]
fn windows_bucket_counts_and_input_counts_are_bounded() {
    for args in [
        (0, 0, 1),
        (1, 0, 1),
        (0, 10, 0),
        (-1, 10, 10),
        (0, 65, 1),
        (i64::MIN, i64::MAX, 1),
        (0, 604_800_001, 1),
    ] {
        assert!(NumericSpec::new(args.0, args.1, args.2).is_err());
    }
    let spec = NumericSpec::new(0, 10, 1).unwrap();
    let rows = (0..101)
        .map(|i| sample(&i.to_string(), 0, PropertyValue::Integer(1)))
        .collect::<Vec<_>>();
    assert_eq!(
        summarize(
            spec,
            PropertyValueKind::Integer,
            Some(&UnitId::new("volt").unwrap()),
            &rows
        ),
        Err(SummaryFailure::InputLimit)
    );
    assert!(
        summarize(
            spec,
            PropertyValueKind::Integer,
            Some(&UnitId::new("volt").unwrap()),
            &[sample("end", 10, PropertyValue::Integer(1))]
        )
        .is_err()
    );
}

#[test]
fn full_valid_unicode_identity_is_not_narrowed_to_a_byte_count() {
    let message = fleetiq_telemetry::AcceptedMessageId::new("🚚".repeat(128)).unwrap();
    let binding = fleetiq_domain::SignalBindingId::new("🚚".repeat(128)).unwrap();
    let identity = format!(
        "[\"{}\",\"protocol\",\"1\",0,\"{}\",\"1\",\"received_at\"]",
        message.as_str(),
        binding.as_str()
    );
    assert!(identity.len() > 1024);
    let mut row = sample("placeholder", 0, PropertyValue::Integer(1));
    row.identity = identity;
    assert!(
        summarize(
            NumericSpec::new(0, 1, 1).unwrap(),
            PropertyValueKind::Integer,
            Some(&UnitId::new("volt").unwrap()),
            &[row]
        )
        .is_ok()
    );
}
#[test]
fn minima_maxima_and_opposite_integer_extremes_are_exact() {
    let rows = [
        sample("min", 0, PropertyValue::Integer(i64::MIN)),
        sample("max", 0, PropertyValue::Integer(i64::MAX)),
    ];
    let result = summarize(
        NumericSpec::new(0, 1, 1).unwrap(),
        PropertyValueKind::Integer,
        Some(&UnitId::new("volt").unwrap()),
        &rows,
    )
    .unwrap();
    assert_eq!(result[0].sum.to_string(), "-1");
    assert_eq!(
        result[0].minimum.as_ref().unwrap().to_string(),
        i64::MIN.to_string()
    );
    assert_eq!(
        result[0].maximum.as_ref().unwrap().to_string(),
        i64::MAX.to_string()
    );
}
