use super::*;
use fleetiq_domain::{
    AssetId, AssetName, AssetTypeId, AssetTypeRef, DefinitionVersion, DeviceEndpointId,
    PropertyDefinitionId, PropertyDefinitionRef, RecordRevision, SignalBindingId, SignalSource,
    SignalTarget, SourceSignalId, TenantId,
};
use fleetiq_history::StoredPropertyChange;
use fleetiq_semantics::{AssetPropertyChangeId, ResolutionContext};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedMeasurement, DecodedObservation, DecodedObservationId,
    ObservationTimes, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
fn property(id: &str) -> PropertyDefinitionRef {
    PropertyDefinitionRef::new(
        PropertyDefinitionId::new(id).unwrap(),
        DefinitionVersion::new(1).unwrap(),
    )
}
fn change(
    axis: &str,
    value: i64,
    time: Option<i64>,
    message: &str,
    quality: ObservationQuality,
) -> StoredPropertyChange {
    let source = DecodedObservationId::new(
        AcceptedMessageId::new(message).unwrap(),
        ProtocolPackageRef::new(
            ProtocolPackageId::new("test.protocol").unwrap(),
            ProtocolPackageVersion::new("1").unwrap(),
        ),
        u32::from(axis == "longitude"),
    );
    let basis = if time.is_some() {
        EventTimeBasis::DeviceRecorded
    } else {
        EventTimeBasis::ReceivedAt
    };
    let id = AssetPropertyChangeId::from_parts(
        source.clone(),
        SignalBindingId::new(format!("bind-{axis}")).unwrap(),
        RecordRevision::new(1).unwrap(),
        basis,
    );
    let observation = DecodedObservation::new(
        source,
        TenantId::new("tenant-a").unwrap(),
        SignalSource::new(
            AssetId::new("device").unwrap(),
            DeviceEndpointId::new("main").unwrap(),
            SourceSignalId::new(axis).unwrap(),
        ),
        DecodedMeasurement::new(PropertyValue::Integer(value), None, quality).unwrap(),
        ObservationTimes::new(time.map(UnixTimeMillis::new), UnixTimeMillis::new(2000)),
    );
    StoredPropertyChange::new(
        id,
        observation,
        SignalTarget::new(AssetId::new("asset-a").unwrap(), property(axis)),
        UnixTimeMillis::new(time.unwrap_or(2000)),
        ResolutionContext::new(UnixTimeMillis::new(1000), UnixTimeMillis::new(2000)),
    )
    .unwrap()
}
fn fixture() -> (FleetAsset, LocationMapping) {
    (
        FleetAsset {
            id: AssetId::new("asset-a").unwrap(),
            name: AssetName::new("Asset").unwrap(),
            asset_type: AssetTypeRef::new(
                AssetTypeId::new("equipment").unwrap(),
                DefinitionVersion::new(1).unwrap(),
            ),
            mapped: true,
            ambiguous: false,
            latitude: Some(change(
                "latitude",
                512_345_678,
                Some(1000),
                "message",
                ObservationQuality::Good,
            )),
            longitude: Some(change(
                "longitude",
                -123_456_789,
                Some(1000),
                "message",
                ObservationQuality::Uncertain,
            )),
        },
        LocationMapping::new(
            property("latitude"),
            property("longitude"),
            CoordinateEncoding::E7,
        )
        .unwrap(),
    )
}
#[test]
fn exact_e7_preserves_sign_and_quality_independently_of_age() {
    let (asset, mapping) = fixture();
    for (now, expected) in [
        (1000, Freshness::Fresh),
        (31001, Freshness::Stale),
        (999, Freshness::Future),
    ] {
        let LocationState::Observed(point) =
            classify_location(&asset, &mapping, true, UnixTimeMillis::new(now))
        else {
            panic!("expected coherent point")
        };
        assert_eq!(point.latitude, "51.2345678");
        assert_eq!(point.longitude, "-12.3456789");
        assert_eq!(point.quality, ObservationQuality::Uncertain);
        assert_eq!(point.freshness, expected);
    }
}
#[test]
fn missing_mapping_and_current_evidence_are_distinct() {
    let (mut asset, mapping) = fixture();
    assert_eq!(
        classify_location(&asset, &mapping, false, UnixTimeMillis::new(0)),
        LocationState::IncompatibleMapping
    );
    asset.latitude = None;
    assert_eq!(
        classify_location(&asset, &mapping, true, UnixTimeMillis::new(0)),
        LocationState::Missing
    );
    asset.mapped = false;
    assert_eq!(
        classify_location(&asset, &mapping, true, UnixTimeMillis::new(0)),
        LocationState::Unmapped
    );
}
#[test]
fn message_time_receipt_only_and_ambiguous_groups_do_not_make_a_fix() {
    for (time, message, ambiguous) in [
        (Some(1001), "message", false),
        (Some(1000), "another", false),
        (None, "message", false),
        (Some(1000), "message", true),
    ] {
        let (mut asset, mapping) = fixture();
        asset.ambiguous = ambiguous;
        asset.longitude = Some(change(
            "longitude",
            1,
            time,
            message,
            ObservationQuality::Good,
        ));
        assert_eq!(
            classify_location(&asset, &mapping, true, UnixTimeMillis::new(2000)),
            LocationState::Incoherent
        );
    }
}
#[test]
fn newest_invalid_quality_and_invalid_coordinates_never_fall_back() {
    let (mut asset, mapping) = fixture();
    asset.latitude = Some(change(
        "latitude",
        1,
        Some(1000),
        "message",
        ObservationQuality::Invalid,
    ));
    assert_eq!(
        classify_location(&asset, &mapping, true, UnixTimeMillis::new(2000)),
        LocationState::InvalidQuality
    );
    asset.latitude = Some(change(
        "latitude",
        900_000_001,
        Some(1000),
        "message",
        ObservationQuality::Good,
    ));
    assert_eq!(
        classify_location(&asset, &mapping, true, UnixTimeMillis::new(2000)),
        LocationState::InvalidCoordinates
    );
}
#[test]
fn exact_decimal_limits_do_not_round_out_of_range_values_into_validity() {
    for text in ["90.0000000000000000000001", "-90.0000000000000000000001"] {
        assert_eq!(
            coordinate(
                &PropertyValue::Decimal(text.parse().unwrap()),
                CoordinateEncoding::Degrees,
                90
            ),
            None
        );
    }
    assert_eq!(
        coordinate(
            &PropertyValue::Decimal("89.9999999999999999999999".parse().unwrap()),
            CoordinateEncoding::Degrees,
            90
        )
        .as_deref(),
        Some("89.9999999999999999999999")
    );
    assert_eq!(
        coordinate(
            &PropertyValue::Integer(i64::MIN),
            CoordinateEncoding::E7,
            90
        ),
        None
    );
    assert_eq!(
        coordinate(
            &PropertyValue::Integer(-1_800_000_000),
            CoordinateEncoding::E7,
            180
        )
        .as_deref(),
        Some("-180.0000000")
    );
}
#[test]
fn query_limits_and_distinct_axis_identity_are_enforced() {
    use crate::FleetQuery;
    let (_, mapping) = fixture();
    assert!(FleetQuery::new(mapping.clone(), 0, None).is_err());
    assert!(FleetQuery::new(mapping.clone(), 101, None).is_err());
    assert!(FleetQuery::new(mapping, 100, None).is_ok());
    assert!(
        LocationMapping::new(
            property("latitude"),
            property("latitude"),
            CoordinateEncoding::E7
        )
        .is_err()
    );
}
