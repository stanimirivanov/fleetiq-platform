use super::*;
use fleetiq_domain::{AssetId, DefinitionVersion, PropertyDefinitionId, PropertyDefinitionRef};
fn target() -> SignalTarget {
    SignalTarget::new(
        AssetId::new("asset-a").unwrap(),
        PropertyDefinitionRef::new(
            PropertyDefinitionId::new("voltage").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
    )
}
#[test]
fn windows_and_page_sizes_are_bounded_before_storage() {
    for (start, end, limit) in [
        (0, 0, 1),
        (1, 0, 1),
        (0, 604_800_001, 1),
        (i64::MIN, i64::MAX, 1),
        (0, 1, 0),
        (0, 1, 101),
    ] {
        assert!(
            PropertyHistoryQuery::new(
                target(),
                UnixTimeMillis::new(start),
                UnixTimeMillis::new(end),
                limit,
                None
            )
            .is_err()
        );
    }
    assert!(
        PropertyHistoryQuery::new(
            target(),
            UnixTimeMillis::new(-1),
            UnixTimeMillis::new(604_799_999),
            100,
            None
        )
        .is_ok()
    );
}

#[test]
fn cursors_must_remain_inside_the_half_open_window() {
    use fleetiq_domain::{RecordRevision, SignalBindingId};
    use fleetiq_semantics::{AssetPropertyChangeId, EventTimeBasis};
    use fleetiq_telemetry::{
        AcceptedMessageId, DecodedObservationId, ProtocolPackageId, ProtocolPackageRef,
        ProtocolPackageVersion,
    };
    let id = AssetPropertyChangeId::from_parts(
        DecodedObservationId::new(
            AcceptedMessageId::new("message").unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("protocol").unwrap(),
                ProtocolPackageVersion::new("1").unwrap(),
            ),
            0,
        ),
        SignalBindingId::new("binding").unwrap(),
        RecordRevision::new(1).unwrap(),
        EventTimeBasis::ReceivedAt,
    );
    for time in [-1, 10] {
        assert!(
            PropertyHistoryQuery::new(
                target(),
                UnixTimeMillis::new(0),
                UnixTimeMillis::new(10),
                1,
                Some(PropertyHistoryCursor::new(
                    UnixTimeMillis::new(time),
                    id.clone()
                ))
            )
            .is_err()
        );
    }
}
