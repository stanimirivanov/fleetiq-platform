use crate::{
    AssetId, DeviceEndpointId, EffectiveInterval, PropertyDefinitionRef, RecordRevision,
    RecordedRevision, SignalBindingId, SourceSignalId, TenantId, UnixTimeMillis,
};

/// Identifies one decoded signal at a communication-device endpoint.
///
/// The device and endpoint locate the source in the physical communication
/// topology. The signal ID is emitted by a versioned protocol package or device
/// profile and remains independent of the semantic asset receiving the value.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SignalSource {
    device: AssetId,
    endpoint: DeviceEndpointId,
    signal: SourceSignalId,
}

impl SignalSource {
    /// Constructs a protocol-neutral signal source.
    pub fn new(
        device_asset_id: AssetId,
        endpoint_id: DeviceEndpointId,
        signal_id: SourceSignalId,
    ) -> Self {
        Self {
            device: device_asset_id,
            endpoint: endpoint_id,
            signal: signal_id,
        }
    }

    /// Returns the asset carrying communication-device capability.
    pub fn device_asset_id(&self) -> &AssetId {
        &self.device
    }

    /// Returns the endpoint where the decoded signal originated.
    pub fn endpoint_id(&self) -> &DeviceEndpointId {
        &self.endpoint
    }

    /// Returns the stable signal identity emitted by the decoder or profile.
    pub fn signal_id(&self) -> &SourceSignalId {
        &self.signal
    }
}

/// Identifies the semantic asset property receiving a source signal.
///
/// The exact property-definition version preserves the meaning used for
/// attribution even after a newer semantic definition is published.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SignalTarget {
    asset_id: AssetId,
    property: PropertyDefinitionRef,
}

impl SignalTarget {
    /// Constructs a semantic signal target.
    pub fn new(asset_id: AssetId, property: PropertyDefinitionRef) -> Self {
        Self { asset_id, property }
    }

    /// Returns the asset whose property receives the observation.
    pub fn asset_id(&self) -> &AssetId {
        &self.asset_id
    }

    /// Returns the exact semantic property definition used by the binding.
    pub fn property(&self) -> &PropertyDefinitionRef {
        &self.property
    }
}

/// A recorded revision of signal attribution within one tenant.
///
/// A binding maps a protocol-neutral source signal to one exact semantic asset
/// property for a half-open effective interval. Recorded revisions preserve what
/// `FleetIQ` knew before a delayed commissioning update or correction arrived.
///
/// This value validates only local structure. Repository and application
/// boundaries must verify that the source device, endpoint, target asset, and
/// property definition all exist in this tenant and are mutually compatible.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignalBinding {
    tenant_id: TenantId,
    id: SignalBindingId,
    recorded_revision: RecordedRevision,
    source: SignalSource,
    target: SignalTarget,
    effective_interval: EffectiveInterval,
}

impl SignalBinding {
    /// Constructs a complete recorded signal-binding revision.
    pub fn new(
        tenant_id: TenantId,
        id: SignalBindingId,
        recorded_revision: RecordedRevision,
        source: SignalSource,
        target: SignalTarget,
        effective_interval: EffectiveInterval,
    ) -> Self {
        Self {
            tenant_id,
            id,
            recorded_revision,
            source,
            target,
            effective_interval,
        }
    }

    /// Returns the tenant that owns this attribution.
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the stable binding identity shared by its revisions.
    pub fn id(&self) -> &SignalBindingId {
        &self.id
    }

    /// Returns this recorded fact's revision number.
    pub fn revision(&self) -> RecordRevision {
        self.recorded_revision.number()
    }

    /// Returns when `FleetIQ` recorded this revision.
    pub fn recorded_at(&self) -> UnixTimeMillis {
        self.recorded_revision.at()
    }

    /// Returns the decoded source signal selected by this binding.
    pub fn source(&self) -> &SignalSource {
        &self.source
    }

    /// Returns the semantic asset property selected by this binding.
    pub fn target(&self) -> &SignalTarget {
        &self.target
    }

    /// Returns the interval in which this attribution applies physically.
    pub fn effective_interval(&self) -> EffectiveInterval {
        self.effective_interval
    }

    /// Reports whether this attribution applies at an effective-time instant.
    pub fn is_effective_at(&self, instant: UnixTimeMillis) -> bool {
        self.effective_interval.contains(instant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DefinitionVersion, PropertyDefinitionId};

    fn instant(value: i64) -> UnixTimeMillis {
        UnixTimeMillis::new(value)
    }

    fn binding(interval: EffectiveInterval) -> SignalBinding {
        SignalBinding::new(
            TenantId::new("tenant-a").expect("valid tenant ID"),
            SignalBindingId::new("binding-1").expect("valid binding ID"),
            RecordedRevision::new(
                RecordRevision::new(2).expect("valid record revision"),
                instant(2_500),
            ),
            SignalSource::new(
                AssetId::new("gateway-1").expect("valid device asset ID"),
                DeviceEndpointId::new("can-primary").expect("valid endpoint ID"),
                SourceSignalId::new("electrical.voltage").expect("valid source signal ID"),
            ),
            SignalTarget::new(
                AssetId::new("power-system-1").expect("valid target asset ID"),
                PropertyDefinitionRef::new(
                    PropertyDefinitionId::new("electrical.voltage")
                        .expect("valid property definition ID"),
                    DefinitionVersion::new(3).expect("valid definition version"),
                ),
            ),
            interval,
        )
    }

    #[test]
    fn preserves_source_target_and_bitemporal_metadata() {
        let interval = EffectiveInterval::new(instant(1_000), Some(instant(4_000)))
            .expect("valid effective interval");
        let binding = binding(interval);

        assert_eq!(binding.tenant_id().as_str(), "tenant-a");
        assert_eq!(binding.id().as_str(), "binding-1");
        assert_eq!(binding.revision().get(), 2);
        assert_eq!(binding.recorded_at(), instant(2_500));
        assert_eq!(binding.source().device_asset_id().as_str(), "gateway-1");
        assert_eq!(binding.source().endpoint_id().as_str(), "can-primary");
        assert_eq!(binding.source().signal_id().as_str(), "electrical.voltage");
        assert_eq!(binding.target().asset_id().as_str(), "power-system-1");
        assert_eq!(
            binding.target().property().id().as_str(),
            "electrical.voltage"
        );
        assert_eq!(binding.target().property().version().get(), 3);
        assert_eq!(binding.effective_interval(), interval);
    }

    #[test]
    fn effective_lookup_uses_half_open_interval_semantics() {
        let binding = binding(
            EffectiveInterval::new(instant(1_000), Some(instant(4_000)))
                .expect("valid effective interval"),
        );

        assert!(!binding.is_effective_at(instant(999)));
        assert!(binding.is_effective_at(instant(1_000)));
        assert!(binding.is_effective_at(instant(3_999)));
        assert!(!binding.is_effective_at(instant(4_000)));
    }

    #[test]
    fn permits_a_device_to_observe_its_own_semantic_property() {
        let asset_id = AssetId::new("gateway-1").expect("valid asset ID");
        let source = SignalSource::new(
            asset_id.clone(),
            DeviceEndpointId::new("onboard").expect("valid endpoint ID"),
            SourceSignalId::new("device.battery.voltage").expect("valid signal ID"),
        );
        let target = SignalTarget::new(
            asset_id,
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("device.battery.voltage").expect("valid property ID"),
                DefinitionVersion::new(1).expect("valid definition version"),
            ),
        );

        assert_eq!(source.device_asset_id(), target.asset_id());
    }
}
