use std::{
    error::Error,
    fmt,
    future::{Future, ready},
    sync::{Arc, Mutex},
};

use fleetiq_domain::{
    AssetId, DefinitionName, DefinitionVersion, DeviceEndpointId, EffectiveInterval,
    PropertyDefinition, PropertyDefinitionId, PropertyDefinitionRef, PropertyValue,
    PropertyValueKind, RecordRevision, RecordedRevision, SignalBinding, SignalBindingId,
    SignalSource, SignalTarget, SourceSignalId, TenantId, UnitId, UnixTimeMillis,
};
use fleetiq_semantics::{SignalBindingSnapshotQuery, UnresolvedReason};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedMeasurement, DecodedObservationId, ObservationQuality,
    ObservationTimes, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};

use super::*;

#[derive(Clone, Copy, Debug)]
struct TestError;

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("test failure")
    }
}
impl Error for TestError {}

#[derive(Clone)]
struct Bindings {
    result: Result<Vec<SignalBinding>, TestError>,
    expected_at: UnixTimeMillis,
}

impl SignalBindingView for Bindings {
    type Error = TestError;

    fn find_for_source(
        &self,
        tenant: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send {
        assert_eq!(tenant.as_str(), "tenant-a");
        assert_eq!(source, &signal_source());
        assert_eq!(query.effective_at(), self.expected_at);
        assert_eq!(query.known_at(), UnixTimeMillis::new(150));
        ready(self.result.clone())
    }
}

#[derive(Clone)]
struct Definitions;

impl PropertyDefinitionView for Definitions {
    type Error = TestError;

    fn find_property<'a>(
        &'a self,
        _tenant: &'a TenantId,
        _reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        ready(Ok(Some(
            PropertyDefinition::new(
                property_ref(),
                DefinitionName::new("Voltage").unwrap(),
                PropertyValueKind::Integer,
                Some(UnitId::new("volt").unwrap()),
            )
            .unwrap(),
        )))
    }
}

#[derive(Clone, Default)]
struct Changes(Arc<Mutex<Vec<AssetPropertyChange>>>);

impl AssetPropertyChangeLog for Changes {
    type Error = TestError;

    fn append<'a>(
        &'a self,
        change: &'a AssetPropertyChange,
    ) -> impl Future<Output = Result<PropertyChangeAppendOutcome, Self::Error>> + Send + 'a {
        let mut changes = self.0.lock().unwrap();
        let outcome = if changes.contains(change) {
            PropertyChangeAppendOutcome::AlreadyPresent
        } else {
            changes.push(change.clone());
            PropertyChangeAppendOutcome::Appended
        };
        ready(Ok(outcome))
    }
}

struct LostAppendResponse;

impl AssetPropertyChangeLog for LostAppendResponse {
    type Error = TestError;

    fn append<'a>(
        &'a self,
        _change: &'a AssetPropertyChange,
    ) -> impl Future<Output = Result<PropertyChangeAppendOutcome, Self::Error>> + Send + 'a {
        ready(Err(TestError))
    }
}

fn signal_source() -> SignalSource {
    SignalSource::new(
        AssetId::new("gateway-1").unwrap(),
        DeviceEndpointId::new("can-primary").unwrap(),
        SourceSignalId::new("bus.voltage").unwrap(),
    )
}

fn property_ref() -> PropertyDefinitionRef {
    PropertyDefinitionRef::new(
        PropertyDefinitionId::new("electrical.voltage").unwrap(),
        DefinitionVersion::new(1).unwrap(),
    )
}

fn binding() -> SignalBinding {
    SignalBinding::new(
        TenantId::new("tenant-a").unwrap(),
        SignalBindingId::new("binding-1").unwrap(),
        RecordedRevision::new(RecordRevision::new(1).unwrap(), UnixTimeMillis::new(100)),
        signal_source(),
        SignalTarget::new(AssetId::new("power-system-1").unwrap(), property_ref()),
        EffectiveInterval::new(UnixTimeMillis::new(0), None).unwrap(),
    )
}

fn observation(device_at: Option<i64>) -> DecodedObservation {
    DecodedObservation::new(
        DecodedObservationId::new(
            AcceptedMessageId::new("message-1").unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("vendor.protocol").unwrap(),
                ProtocolPackageVersion::new("1").unwrap(),
            ),
            0,
        ),
        TenantId::new("tenant-a").unwrap(),
        signal_source(),
        DecodedMeasurement::new(
            PropertyValue::Integer(48),
            Some(UnitId::new("volt").unwrap()),
            ObservationQuality::Good,
        )
        .unwrap(),
        ObservationTimes::new(
            device_at.map(UnixTimeMillis::new),
            UnixTimeMillis::new(1100),
        ),
    )
}

fn processor(
    result: Result<Vec<SignalBinding>, TestError>,
    expected_at: i64,
    log: Changes,
) -> SemanticHistoryProcessor<Bindings, Definitions, Changes> {
    SemanticHistoryProcessor::new(
        SignalBindingResolver::new(
            Bindings {
                result,
                expected_at: UnixTimeMillis::new(expected_at),
            },
            Definitions,
        ),
        log,
    )
}

#[tokio::test]
async fn selected_device_time_is_binding_time_and_exact_retry_is_idempotent() {
    let log = Changes::default();
    let processor = processor(Ok(vec![binding()]), 1000, log.clone());
    let decoded = observation(Some(1000));
    let first = processor
        .process(
            decoded.clone(),
            UnixTimeMillis::new(150),
            EventTimeBasis::DeviceRecorded,
        )
        .await
        .unwrap();
    let SemanticHistoryOutcome::Recorded { change, append } = first else {
        panic!("binding should resolve")
    };
    assert_eq!(append, PropertyChangeAppendOutcome::Appended);
    assert_eq!(change.event_at(), UnixTimeMillis::new(1000));
    assert_eq!(
        change.bound_observation().context().effective_at(),
        UnixTimeMillis::new(1000)
    );
    let second = processor
        .process(
            decoded,
            UnixTimeMillis::new(150),
            EventTimeBasis::DeviceRecorded,
        )
        .await
        .unwrap();
    assert!(matches!(
        second,
        SemanticHistoryOutcome::Recorded {
            append: PropertyChangeAppendOutcome::AlreadyPresent,
            ..
        }
    ));
    assert_eq!(log.0.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn receipt_time_can_attribute_an_observation_without_device_time() {
    let processor = processor(Ok(vec![binding()]), 1100, Changes::default());
    let result = processor
        .process(
            observation(None),
            UnixTimeMillis::new(150),
            EventTimeBasis::ReceivedAt,
        )
        .await
        .unwrap();
    let SemanticHistoryOutcome::Recorded { change, .. } = result else {
        panic!("binding should resolve")
    };
    assert_eq!(change.event_at(), UnixTimeMillis::new(1100));
    assert_eq!(
        change.bound_observation().context().effective_at(),
        UnixTimeMillis::new(1100)
    );
}

#[tokio::test]
async fn absent_device_time_preserves_observation_before_any_lookup() {
    let decoded = observation(None);
    let log = Changes::default();
    let error = processor(Err(TestError), 1000, log.clone())
        .process(
            decoded.clone(),
            UnixTimeMillis::new(150),
            EventTimeBasis::DeviceRecorded,
        )
        .await
        .err()
        .expect("missing device time must fail");
    assert!(matches!(
        error,
        SemanticHistoryError::MissingDeviceRecordedTime { .. }
    ));
    let SemanticHistoryRetry::Observation {
        observation: retained,
        known_at,
        basis,
    } = error.into_retry()
    else {
        panic!("observation must be retained")
    };
    assert_eq!(*retained, decoded);
    assert_eq!(known_at, UnixTimeMillis::new(150));
    assert_eq!(basis, EventTimeBasis::DeviceRecorded);
    assert!(log.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unresolved_and_read_failure_never_append() {
    let decoded = observation(Some(1000));
    let log = Changes::default();
    let unresolved = processor(Ok(vec![]), 1000, log.clone())
        .process(
            decoded.clone(),
            UnixTimeMillis::new(150),
            EventTimeBasis::DeviceRecorded,
        )
        .await
        .unwrap();
    let SemanticHistoryOutcome::Unresolved(unresolved) = unresolved else {
        panic!("binding is absent")
    };
    assert_eq!(unresolved.observation(), &decoded);
    assert_eq!(unresolved.reason(), &UnresolvedReason::NoEffectiveBinding);
    let error = processor(Err(TestError), 1000, log.clone())
        .process(
            decoded.clone(),
            UnixTimeMillis::new(150),
            EventTimeBasis::DeviceRecorded,
        )
        .await
        .err()
        .expect("binding read must fail");
    let SemanticHistoryRetry::Observation {
        observation: retained,
        known_at,
        basis,
    } = error.into_retry()
    else {
        panic!("observation must be retained")
    };
    assert_eq!(*retained, decoded);
    assert_eq!(known_at, UnixTimeMillis::new(150));
    assert_eq!(basis, EventTimeBasis::DeviceRecorded);
    assert!(log.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn append_failure_retains_exact_change_for_retry_without_resolution() {
    let failing = SemanticHistoryProcessor::new(
        SignalBindingResolver::new(
            Bindings {
                result: Ok(vec![binding()]),
                expected_at: UnixTimeMillis::new(1000),
            },
            Definitions,
        ),
        LostAppendResponse,
    );
    let error = failing
        .process(
            observation(Some(1000)),
            UnixTimeMillis::new(150),
            EventTimeBasis::DeviceRecorded,
        )
        .await
        .err()
        .expect("append response should fail");
    assert!(matches!(error, SemanticHistoryError::Append { .. }));
    let SemanticHistoryRetry::Change(change) = error.into_retry() else {
        panic!("exact change must be retained")
    };
    let expected_id = change.id().clone();
    // A read adapter that fails must never be called by append_change.
    let retry = processor(Err(TestError), 1000, Changes::default())
        .append_change(*change)
        .await
        .expect("exact append retry should succeed");
    let SemanticHistoryOutcome::Recorded { change, append } = retry else {
        panic!("change should be recorded")
    };
    assert_eq!(append, PropertyChangeAppendOutcome::Appended);
    assert_eq!(change.id(), &expected_id);
}
