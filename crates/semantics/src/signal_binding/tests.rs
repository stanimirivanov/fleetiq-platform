use std::{
    error::Error,
    fmt,
    future::{Future, ready},
};

use fleetiq_domain::{
    AssetId, DefinitionName, DefinitionVersion, DeviceEndpointId, EffectiveInterval,
    PropertyDefinition, PropertyDefinitionId, PropertyDefinitionRef, PropertyValueKind,
    RecordRevision, SignalBinding, SignalBindingId, SignalSource, SignalTarget, SourceSignalId,
    TenantId, UnitId, UnixTimeMillis,
};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedMeasurement, DecodedObservation, DecodedObservationId, DecodedValue,
    ObservationQuality, ObservationTimes, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestRepositoryError;

impl fmt::Display for TestRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("test repository unavailable")
    }
}

impl Error for TestRepositoryError {}

#[derive(Clone)]
struct BindingView {
    result: Result<Vec<SignalBinding>, TestRepositoryError>,
    expected_context: ResolutionContext,
}

impl SignalBindingView for BindingView {
    type Error = TestRepositoryError;

    fn find_for_source(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send {
        assert_eq!(tenant_id.as_str(), "tenant-a");
        assert_eq!(source, &signal_source());
        assert_eq!(query.effective_at(), self.expected_context.effective_at());
        assert_eq!(query.known_at(), self.expected_context.known_at());
        ready(self.result.clone())
    }
}

#[derive(Clone)]
struct DefinitionView {
    result: Result<Option<PropertyDefinition>, TestRepositoryError>,
}

impl PropertyDefinitionView for DefinitionView {
    type Error = TestRepositoryError;

    fn find_property<'a>(
        &'a self,
        _tenant_id: &'a TenantId,
        _reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        ready(self.result.clone())
    }
}

fn version(value: u32) -> DefinitionVersion {
    DefinitionVersion::new(value).expect("test definition version should be valid")
}

fn property_reference() -> PropertyDefinitionRef {
    PropertyDefinitionRef::new(
        PropertyDefinitionId::new("electrical.voltage").expect("valid property ID"),
        version(1),
    )
}

fn signal_source() -> SignalSource {
    SignalSource::new(
        AssetId::new("gateway-1").expect("valid device asset ID"),
        DeviceEndpointId::new("can-primary").expect("valid endpoint ID"),
        SourceSignalId::new("bus.voltage").expect("valid signal ID"),
    )
}

fn binding(id: &str) -> SignalBinding {
    SignalBinding::new(
        TenantId::new("tenant-a").expect("valid tenant ID"),
        SignalBindingId::new(id).expect("valid binding ID"),
        fleetiq_domain::RecordedRevision::new(
            RecordRevision::new(2).expect("valid revision"),
            UnixTimeMillis::new(150),
        ),
        signal_source(),
        SignalTarget::new(
            AssetId::new("power-system-1").expect("valid target asset ID"),
            property_reference(),
        ),
        EffectiveInterval::new(UnixTimeMillis::new(0), None).expect("valid interval"),
    )
}

fn property(kind: PropertyValueKind, unit: Option<&str>) -> PropertyDefinition {
    PropertyDefinition::new(
        property_reference(),
        DefinitionName::new("Electrical voltage").expect("valid definition name"),
        kind,
        unit.map(|value| UnitId::new(value).expect("valid unit ID")),
    )
    .expect("test property definition should be valid")
}

fn observation(value: DecodedValue, unit: Option<&str>) -> DecodedObservation {
    DecodedObservation::new(
        DecodedObservationId::new(
            AcceptedMessageId::new("message-1").expect("valid message ID"),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("example.binary").expect("valid package ID"),
                ProtocolPackageVersion::new("1").expect("valid package version"),
            ),
            0,
        ),
        TenantId::new("tenant-a").expect("valid tenant ID"),
        signal_source(),
        DecodedMeasurement::new(
            value,
            unit.map(|value| UnitId::new(value).expect("valid unit ID")),
            ObservationQuality::Good,
        )
        .expect("test measurement should be valid"),
        ObservationTimes::new(Some(UnixTimeMillis::new(1_000)), UnixTimeMillis::new(1_100)),
    )
}

fn context() -> ResolutionContext {
    ResolutionContext::new(UnixTimeMillis::new(1_000), UnixTimeMillis::new(2_000))
}

fn resolver(
    bindings: Result<Vec<SignalBinding>, TestRepositoryError>,
    property: Result<Option<PropertyDefinition>, TestRepositoryError>,
) -> SignalBindingResolver<BindingView, DefinitionView> {
    SignalBindingResolver::new(
        BindingView {
            result: bindings,
            expected_context: context(),
        },
        DefinitionView { result: property },
    )
}

#[tokio::test]
async fn binds_one_exactly_compatible_observation_with_full_provenance() {
    let decoded = observation(DecodedValue::Integer(48), Some("volt"));
    let outcome = resolver(
        Ok(vec![binding("binding-1")]),
        Ok(Some(property(PropertyValueKind::Integer, Some("volt")))),
    )
    .resolve(decoded.clone(), context())
    .await
    .expect("repository views should be available");

    let ResolutionOutcome::Bound(bound) = outcome else {
        panic!("exactly compatible observation should be bound");
    };
    assert_eq!(bound.observation(), &decoded);
    assert_eq!(bound.id(), decoded.id());
    assert_eq!(bound.target().asset_id().as_str(), "power-system-1");
    assert_eq!(bound.target().property(), &property_reference());
    assert_eq!(bound.binding().binding_id().as_str(), "binding-1");
    assert_eq!(bound.binding().revision().get(), 2);
    assert_eq!(bound.context(), context());
}

#[tokio::test]
async fn retains_an_observation_when_no_effective_binding_exists() {
    let decoded = observation(DecodedValue::Integer(48), Some("volt"));
    let outcome = resolver(Ok(vec![]), Ok(None))
        .resolve(decoded.clone(), context())
        .await
        .expect("repository view should be available");

    let ResolutionOutcome::Unresolved(unresolved) = outcome else {
        panic!("an unconfigured source must remain unresolved");
    };
    assert_eq!(unresolved.observation(), &decoded);
    assert_eq!(unresolved.reason(), &UnresolvedReason::NoEffectiveBinding);
    assert_eq!(unresolved.context(), context());
}

#[tokio::test]
async fn reports_all_ambiguous_bindings_in_deterministic_order() {
    let outcome = resolver(
        Ok(vec![binding("binding-z"), binding("binding-a")]),
        Err(TestRepositoryError),
    )
    .resolve(
        observation(DecodedValue::Integer(48), Some("volt")),
        context(),
    )
    .await
    .expect("ambiguity is resolved before property lookup");

    let ResolutionOutcome::Unresolved(unresolved) = outcome else {
        panic!("competing bindings must remain unresolved");
    };
    let UnresolvedReason::AmbiguousBindings { candidates } = unresolved.reason() else {
        panic!("ambiguity should identify its candidates");
    };
    assert_eq!(candidates[0].binding_id().as_str(), "binding-a");
    assert_eq!(candidates[1].binding_id().as_str(), "binding-z");
}

#[tokio::test]
async fn reports_a_missing_exact_property_definition() {
    let outcome = resolver(Ok(vec![binding("binding-1")]), Ok(None))
        .resolve(
            observation(DecodedValue::Integer(48), Some("volt")),
            context(),
        )
        .await
        .expect("repository views should be available");

    let ResolutionOutcome::Unresolved(unresolved) = outcome else {
        panic!("missing semantic metadata must remain unresolved");
    };
    assert_eq!(
        unresolved.reason(),
        &UnresolvedReason::MissingPropertyDefinition {
            property: property_reference()
        }
    );
}

#[tokio::test]
async fn reports_value_kind_and_unit_incompatibility_separately() {
    let kind_outcome = resolver(
        Ok(vec![binding("binding-1")]),
        Ok(Some(property(PropertyValueKind::Decimal, Some("volt")))),
    )
    .resolve(
        observation(DecodedValue::Integer(48), Some("volt")),
        context(),
    )
    .await
    .expect("repository views should be available");
    let ResolutionOutcome::Unresolved(kind_failure) = kind_outcome else {
        panic!("a kind mismatch must remain unresolved");
    };
    assert_eq!(
        kind_failure.reason(),
        &UnresolvedReason::ValueKindMismatch {
            actual: PropertyValueKind::Integer,
            expected: PropertyValueKind::Decimal,
        }
    );

    let unit_outcome = resolver(
        Ok(vec![binding("binding-1")]),
        Ok(Some(property(PropertyValueKind::Integer, Some("volt")))),
    )
    .resolve(
        observation(DecodedValue::Integer(48), Some("millivolt")),
        context(),
    )
    .await
    .expect("repository views should be available");
    let ResolutionOutcome::Unresolved(unit_failure) = unit_outcome else {
        panic!("a unit mismatch must remain unresolved");
    };
    assert_eq!(
        unit_failure.reason(),
        &UnresolvedReason::UnitConversionRequired {
            source: Some(UnitId::new("millivolt").expect("valid unit ID")),
            target: Some(UnitId::new("volt").expect("valid unit ID")),
        }
    );
}

#[tokio::test]
async fn distinguishes_operational_repository_failures() {
    let binding_error = resolver(Err(TestRepositoryError), Ok(None))
        .resolve(
            observation(DecodedValue::Integer(48), Some("volt")),
            context(),
        )
        .await
        .expect_err("binding repository failure should be operational");
    assert!(matches!(
        binding_error,
        ResolutionError::BindingRepository(TestRepositoryError)
    ));

    let definition_error = resolver(Ok(vec![binding("binding-1")]), Err(TestRepositoryError))
        .resolve(
            observation(DecodedValue::Integer(48), Some("volt")),
            context(),
        )
        .await
        .expect_err("definition repository failure should be operational");
    assert!(matches!(
        definition_error,
        ResolutionError::DefinitionRepository(TestRepositoryError)
    ));
}
