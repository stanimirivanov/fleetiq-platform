use fleetiq_domain::{RecordRevision, SignalBindingId};
use fleetiq_semantics::EventTimeBasis;
use fleetiq_telemetry::{
    AcceptedMessageId, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};

use super::*;

fn observation(message: &str) -> DecodedObservationId {
    DecodedObservationId::new(
        AcceptedMessageId::new(message).unwrap(),
        ProtocolPackageRef::new(
            ProtocolPackageId::new("vendor.protocol").unwrap(),
            ProtocolPackageVersion::new("1").unwrap(),
        ),
        0,
    )
}

#[test]
fn page_limit_is_bounded_and_cursor_belongs_to_exact_observation() {
    let source = observation("message-1");
    let cursor = AssetPropertyChangeId::from_parts(
        source.clone(),
        SignalBindingId::new("binding-1").unwrap(),
        RecordRevision::new(1).unwrap(),
        EventTimeBasis::DeviceRecorded,
    );
    assert_eq!(
        ObservationInterpretationPageQuery::new(source.clone(), 0, None),
        Err(InvalidObservationInterpretationPageQuery::Limit)
    );
    assert_eq!(
        ObservationInterpretationPageQuery::new(source.clone(), 101, None),
        Err(InvalidObservationInterpretationPageQuery::Limit)
    );
    let query = ObservationInterpretationPageQuery::new(
        source.clone(),
        ObservationInterpretationPageQuery::MAX_LIMIT,
        Some(cursor.clone()),
    )
    .unwrap();
    assert_eq!(query.observation_id(), &source);
    assert_eq!(query.after(), Some(&cursor));
    assert_eq!(
        ObservationInterpretationPageQuery::new(observation("message-2"), 1, Some(cursor)),
        Err(InvalidObservationInterpretationPageQuery::CursorObservation)
    );
}
