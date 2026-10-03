use std::{
    future::{Future, ready},
    io,
};

use fleetiq_domain::{
    AssetId, DeviceEndpointId, PropertyDefinition, PropertyDefinitionRef, PropertyValue,
    SignalBinding, SignalSource, SourceSignalId,
};
use fleetiq_semantics::{AssetPropertyChange, SignalBindingSnapshotQuery};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedMeasurement, ObservationQuality, ObservationTimes, ProtocolPackageId,
    ProtocolPackageRef, ProtocolPackageVersion,
};

use super::*;
use crate::PropertyChangeAppendOutcome;

#[derive(Clone, Copy)]
enum ObservationView {
    Missing,
    Failed,
    WrongTenant,
    WrongOutput,
}

impl DecodedObservationView for ObservationView {
    type Error = io::Error;

    fn find<'a>(
        &'a self,
        tenant: &'a TenantId,
        id: &'a DecodedObservationId,
    ) -> impl Future<Output = Result<Option<DecodedObservation>, Self::Error>> + Send + 'a {
        assert_eq!(tenant.as_str(), "tenant-a");
        assert_eq!(id, request().id());
        ready(match self {
            Self::Missing => Ok(None),
            Self::Failed => Err(io::Error::other("test lookup failure")),
            Self::WrongTenant => Ok(Some(observation(
                TenantId::new("tenant-b").unwrap(),
                id.clone(),
            ))),
            Self::WrongOutput => Ok(Some(observation(
                tenant.clone(),
                DecodedObservationId::new(id.message_id().clone(), id.decoded_by().clone(), 1),
            ))),
        })
    }
}

#[derive(Clone, Copy)]
struct NeverRead;

impl SignalBindingView for NeverRead {
    type Error = io::Error;

    fn find_for_source(
        &self,
        _: &TenantId,
        _: &SignalSource,
        _: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send {
        ready(Err(io::Error::other("unexpected semantic lookup")))
    }
}

impl PropertyDefinitionView for NeverRead {
    type Error = io::Error;

    fn find_property<'a>(
        &'a self,
        _: &'a TenantId,
        _: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        ready(Err(io::Error::other("unexpected definition lookup")))
    }
}

impl AssetPropertyChangeLog for NeverRead {
    type Error = io::Error;

    fn append<'a>(
        &'a self,
        _: &'a AssetPropertyChange,
    ) -> impl Future<Output = Result<PropertyChangeAppendOutcome, Self::Error>> + Send + 'a {
        ready(Err(io::Error::other("unexpected history append")))
    }
}

fn request() -> StoredObservationRequest {
    StoredObservationRequest::new(
        TenantId::new("tenant-a").unwrap(),
        DecodedObservationId::new(
            AcceptedMessageId::new("message-1").unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("vendor.protocol").unwrap(),
                ProtocolPackageVersion::new("1").unwrap(),
            ),
            0,
        ),
        UnixTimeMillis::new(150),
        EventTimeBasis::DeviceRecorded,
    )
}

fn observation(tenant_id: TenantId, id: DecodedObservationId) -> DecodedObservation {
    DecodedObservation::new(
        id,
        tenant_id,
        SignalSource::new(
            AssetId::new("gateway-1").unwrap(),
            DeviceEndpointId::new("can-primary").unwrap(),
            SourceSignalId::new("bus.voltage").unwrap(),
        ),
        DecodedMeasurement::new(PropertyValue::Integer(48), None, ObservationQuality::Good)
            .unwrap(),
        ObservationTimes::new(Some(UnixTimeMillis::new(1000)), UnixTimeMillis::new(1100)),
    )
}

fn service(
    view: ObservationView,
) -> StoredSemanticHistoryProcessor<ObservationView, NeverRead, NeverRead, NeverRead> {
    StoredSemanticHistoryProcessor::new(
        view,
        SemanticHistoryProcessor::new(
            fleetiq_semantics::SignalBindingResolver::new(NeverRead, NeverRead),
            NeverRead,
        ),
    )
}

#[tokio::test]
async fn exact_absence_does_not_invoke_semantic_reads_or_append() {
    let request = request();
    assert!(
        service(ObservationView::Missing)
            .process(&request)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(request.known_at(), UnixTimeMillis::new(150));
    assert_eq!(request.basis(), EventTimeBasis::DeviceRecorded);
}

#[tokio::test]
async fn source_failure_preserves_the_borrowed_request_and_cause() {
    let request = request();
    let error = service(ObservationView::Failed)
        .process(&request)
        .await
        .err()
        .expect("source lookup should fail");
    assert!(matches!(error, StoredSemanticHistoryError::Lookup(_)));
    assert_eq!(request, self::request());
    assert!(std::error::Error::source(&error).is_some());
}

#[tokio::test]
async fn mismatched_source_identity_fails_before_semantic_resolution() {
    for view in [ObservationView::WrongTenant, ObservationView::WrongOutput] {
        let error = service(view)
            .process(&request())
            .await
            .err()
            .expect("a mismatched source must fail closed");
        assert!(matches!(error, StoredSemanticHistoryError::SourceMismatch));
    }
}
