use std::{error::Error, fmt, future::Future};

use fleetiq_application::{
    DefinitionRepository, SignalBindingRepository, SignalBindingSnapshotQuery,
};
use fleetiq_domain::{
    PropertyDefinition, PropertyDefinitionRef, PropertyValueKind, RecordRevision, SignalBinding,
    SignalBindingId, SignalSource, SignalTarget, TenantId, UnitId, UnixTimeMillis,
};
use fleetiq_telemetry::{DecodedObservation, DecodedObservationId};

/// Selects the physical and recorded-time view used for semantic attribution.
///
/// The caller supplies effective time because choosing between device and
/// receipt time is an operational policy. Knowledge time remains explicit so
/// replay can reproduce either an original or corrected historical view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolutionContext {
    effective_at: UnixTimeMillis,
    known_at: UnixTimeMillis,
}

impl ResolutionContext {
    /// Constructs a bitemporal semantic-resolution context.
    pub const fn new(effective_at: UnixTimeMillis, known_at: UnixTimeMillis) -> Self {
        Self {
            effective_at,
            known_at,
        }
    }

    /// Returns the physical-time instant used to select a binding.
    pub const fn effective_at(self) -> UnixTimeMillis {
        self.effective_at
    }

    /// Returns the inclusive recorded-time cutoff used to select a binding.
    pub const fn known_at(self) -> UnixTimeMillis {
        self.known_at
    }
}

/// Provides the bitemporal binding read required by semantic resolution.
///
/// The blanket implementation adapts any application [`SignalBindingRepository`].
/// A worker-local projection can implement this smaller consumer-owned view
/// without exposing persistence write operations.
pub trait SignalBindingView: Send + Sync {
    /// The infrastructure-specific lookup failure.
    type Error: Error + Send + Sync + 'static;

    /// Finds bindings selecting a source in one bitemporal snapshot.
    fn find_for_source(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send;
}

impl<T: SignalBindingRepository> SignalBindingView for T {
    type Error = T::Error;

    fn find_for_source(
        &self,
        tenant_id: &TenantId,
        source: &SignalSource,
        query: SignalBindingSnapshotQuery,
    ) -> impl Future<Output = Result<Vec<SignalBinding>, Self::Error>> + Send {
        SignalBindingRepository::find_for_source(self, tenant_id, source, query)
    }
}

/// Provides exact property definitions required by semantic resolution.
///
/// The blanket implementation adapts any application [`DefinitionRepository`].
pub trait PropertyDefinitionView: Send + Sync {
    /// The infrastructure-specific lookup failure.
    type Error: Error + Send + Sync + 'static;

    /// Finds one exact tenant-owned property-definition version.
    fn find_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a;
}

impl<T: DefinitionRepository> PropertyDefinitionView for T {
    type Error = T::Error;

    fn find_property<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        reference: &'a PropertyDefinitionRef,
    ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a {
        DefinitionRepository::find_property(self, tenant_id, reference)
    }
}

/// Identifies the exact signal-binding revision used for attribution.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SignalBindingProvenance {
    binding_id: SignalBindingId,
    revision: RecordRevision,
}

impl SignalBindingProvenance {
    fn new(binding_id: SignalBindingId, revision: RecordRevision) -> Self {
        Self {
            binding_id,
            revision,
        }
    }

    /// Returns the stable binding identity.
    pub const fn binding_id(&self) -> &SignalBindingId {
        &self.binding_id
    }

    /// Returns the immutable binding revision used for attribution.
    pub const fn revision(&self) -> RecordRevision {
        self.revision
    }
}

/// A decoded observation attributed to one exact semantic asset property.
///
/// The decoded observation remains embedded so source lineage, decoder
/// provenance, timestamps, quality, and value survive attribution. Exact kind
/// and unit compatibility has been verified.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundObservation {
    observation: DecodedObservation,
    target: SignalTarget,
    binding: SignalBindingProvenance,
    context: ResolutionContext,
}

impl BoundObservation {
    fn new(
        observation: DecodedObservation,
        target: SignalTarget,
        binding: SignalBindingProvenance,
        context: ResolutionContext,
    ) -> Self {
        Self {
            observation,
            target,
            binding,
            context,
        }
    }

    /// Returns the decoded observation identity for downstream idempotency.
    pub const fn id(&self) -> &DecodedObservationId {
        self.observation.id()
    }

    /// Returns the decoded observation and its complete source provenance.
    pub const fn observation(&self) -> &DecodedObservation {
        &self.observation
    }

    /// Returns the semantic asset and property receiving this observation.
    pub const fn target(&self) -> &SignalTarget {
        &self.target
    }

    /// Returns the exact binding revision used for attribution.
    pub const fn binding(&self) -> &SignalBindingProvenance {
        &self.binding
    }

    /// Returns the bitemporal context in which attribution was resolved.
    pub const fn context(&self) -> ResolutionContext {
        self.context
    }
}

/// Identifies one competing binding revision in an ambiguous resolution.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct BindingCandidate {
    binding_id: SignalBindingId,
    revision: RecordRevision,
}

impl BindingCandidate {
    fn new(binding_id: SignalBindingId, revision: RecordRevision) -> Self {
        Self {
            binding_id,
            revision,
        }
    }

    /// Returns the stable candidate binding identity.
    pub const fn binding_id(&self) -> &SignalBindingId {
        &self.binding_id
    }

    /// Returns the candidate's immutable revision.
    pub const fn revision(&self) -> RecordRevision {
        self.revision
    }
}

/// Explains why a decoded observation could not be attributed confidently.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnresolvedReason {
    /// No binding selected this source in the requested bitemporal snapshot.
    NoEffectiveBinding,
    /// More than one binding selected this source and no authority policy exists.
    AmbiguousBindings {
        /// Deterministically ordered competing binding revisions.
        candidates: Vec<BindingCandidate>,
    },
    /// The binding referenced a property unavailable from the definition view.
    MissingPropertyDefinition {
        /// Exact property definition required by the binding.
        property: PropertyDefinitionRef,
    },
    /// The decoded value kind differs from the target property's required kind.
    ValueKindMismatch {
        /// Kind emitted by the decoder.
        actual: PropertyValueKind,
        /// Kind required by the exact target property definition.
        expected: PropertyValueKind,
    },
    /// Source and canonical target units differ and no conversion was selected.
    UnitConversionRequired {
        /// Unit emitted by the decoder, or `None` when absent or unitless.
        source: Option<UnitId>,
        /// Canonical target unit, or `None` for a unitless property.
        target: Option<UnitId>,
    },
}

/// Preserves a decoded observation that requires configuration or policy work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedObservation {
    observation: DecodedObservation,
    reason: UnresolvedReason,
    context: ResolutionContext,
}

impl UnresolvedObservation {
    fn new(
        observation: DecodedObservation,
        reason: UnresolvedReason,
        context: ResolutionContext,
    ) -> Self {
        Self {
            observation,
            reason,
            context,
        }
    }

    /// Returns the decoded observation retained for diagnosis or retry.
    pub const fn observation(&self) -> &DecodedObservation {
        &self.observation
    }

    /// Returns the configuration or compatibility reason blocking attribution.
    pub const fn reason(&self) -> &UnresolvedReason {
        &self.reason
    }

    /// Returns the bitemporal context of the failed resolution attempt.
    pub const fn context(&self) -> ResolutionContext {
        self.context
    }
}

/// Result of semantic attribution for one decoded observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolutionOutcome {
    /// Exactly one compatible binding produced a semantic observation.
    Bound(BoundObservation),
    /// Attribution requires missing configuration or an explicit later policy.
    Unresolved(UnresolvedObservation),
}

/// Resolves decoded sources through bitemporal binding and definition views.
#[derive(Clone, Debug)]
pub struct SignalBindingResolver<B, D> {
    bindings: B,
    definitions: D,
}

impl<B, D> SignalBindingResolver<B, D> {
    /// Constructs a resolver from application-owned repository ports.
    pub const fn new(bindings: B, definitions: D) -> Self {
        Self {
            bindings,
            definitions,
        }
    }
}

impl<B, D> SignalBindingResolver<B, D>
where
    B: SignalBindingView,
    D: PropertyDefinitionView,
{
    /// Attributes one decoded observation without guessing missing policy.
    ///
    /// Exactly one effective binding must exist. Value kind and source unit must
    /// equal the exact property's declared kind and canonical unit. Expected
    /// configuration problems return [`ResolutionOutcome::Unresolved`] so the
    /// observation can be diagnosed and retried. Repository failures remain
    /// operational errors.
    ///
    /// # Errors
    ///
    /// Returns [`ResolutionError`] when either repository cannot serve the
    /// requested view.
    pub async fn resolve(
        &self,
        observation: DecodedObservation,
        context: ResolutionContext,
    ) -> Result<ResolutionOutcome, ResolutionError<B::Error, D::Error>> {
        let mut bindings = self
            .bindings
            .find_for_source(
                observation.tenant_id(),
                observation.source(),
                SignalBindingSnapshotQuery::new(context.effective_at(), context.known_at()),
            )
            .await
            .map_err(ResolutionError::BindingRepository)?;

        if bindings.is_empty() {
            return Ok(unresolved(
                observation,
                UnresolvedReason::NoEffectiveBinding,
                context,
            ));
        }

        if bindings.len() > 1 {
            let mut candidates: Vec<_> = bindings
                .into_iter()
                .map(|binding| BindingCandidate::new(binding.id().clone(), binding.revision()))
                .collect();
            candidates.sort_unstable();
            return Ok(unresolved(
                observation,
                UnresolvedReason::AmbiguousBindings { candidates },
                context,
            ));
        }

        let Some(binding) = bindings.pop() else {
            return Ok(unresolved(
                observation,
                UnresolvedReason::NoEffectiveBinding,
                context,
            ));
        };
        let property_reference = binding.target().property();
        let property = PropertyDefinitionView::find_property(
            &self.definitions,
            observation.tenant_id(),
            property_reference,
        )
        .await
        .map_err(ResolutionError::DefinitionRepository)?;
        let Some(property) = property else {
            return Ok(unresolved(
                observation,
                UnresolvedReason::MissingPropertyDefinition {
                    property: property_reference.clone(),
                },
                context,
            ));
        };

        let actual_kind = observation.value().kind();
        let expected_kind = property.value_kind();
        if actual_kind != expected_kind {
            return Ok(unresolved(
                observation,
                UnresolvedReason::ValueKindMismatch {
                    actual: actual_kind,
                    expected: expected_kind,
                },
                context,
            ));
        }

        if observation.source_unit() != property.canonical_unit() {
            let source_unit = observation.source_unit().cloned();
            let target_unit = property.canonical_unit().cloned();
            return Ok(unresolved(
                observation,
                UnresolvedReason::UnitConversionRequired {
                    source: source_unit,
                    target: target_unit,
                },
                context,
            ));
        }

        let provenance = SignalBindingProvenance::new(binding.id().clone(), binding.revision());
        let target = binding.target().clone();
        Ok(ResolutionOutcome::Bound(BoundObservation::new(
            observation,
            target,
            provenance,
            context,
        )))
    }
}

fn unresolved(
    observation: DecodedObservation,
    reason: UnresolvedReason,
    context: ResolutionContext,
) -> ResolutionOutcome {
    ResolutionOutcome::Unresolved(UnresolvedObservation::new(observation, reason, context))
}

/// Operational failure while loading semantic resolution state.
#[derive(Debug)]
pub enum ResolutionError<B, D> {
    /// The signal-binding repository could not serve the requested snapshot.
    BindingRepository(B),
    /// The semantic-definition repository could not load the target property.
    DefinitionRepository(D),
}

impl<B: fmt::Display, D: fmt::Display> fmt::Display for ResolutionError<B, D> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindingRepository(error) => {
                write!(formatter, "signal-binding lookup failed: {error}")
            }
            Self::DefinitionRepository(error) => {
                write!(formatter, "property-definition lookup failed: {error}")
            }
        }
    }
}

impl<B: Error + 'static, D: Error + 'static> Error for ResolutionError<B, D> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::BindingRepository(error) => Some(error),
            Self::DefinitionRepository(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::{Future, ready};

    use fleetiq_domain::{
        AssetId, DefinitionName, DefinitionVersion, DeviceEndpointId, EffectiveInterval,
        PropertyDefinitionId, SourceSignalId,
    };
    use fleetiq_telemetry::{
        AcceptedMessageId, DecodedMeasurement, DecodedObservationId, DecodedValue,
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
        ) -> impl Future<Output = Result<Option<PropertyDefinition>, Self::Error>> + Send + 'a
        {
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
}
