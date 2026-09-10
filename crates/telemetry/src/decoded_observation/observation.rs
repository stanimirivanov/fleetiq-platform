use fleetiq_domain::{SignalSource, TenantId, UnitId, UnixTimeMillis};

use super::{
    DecodedMeasurement, DecodedObservationId, DecodedValue, ObservationQuality, ProtocolPackageRef,
};

/// Preserves device and server time evidence without selecting an event time.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ObservationTimes {
    device_recorded_at: Option<UnixTimeMillis>,
    received_at: UnixTimeMillis,
}

impl ObservationTimes {
    /// Constructs source-message timestamp evidence.
    pub const fn new(
        device_recorded_at: Option<UnixTimeMillis>,
        received_at: UnixTimeMillis,
    ) -> Self {
        Self {
            device_recorded_at,
            received_at,
        }
    }

    /// Returns the timestamp reported by the device, when available.
    pub const fn device_recorded_at(self) -> Option<UnixTimeMillis> {
        self.device_recorded_at
    }

    /// Returns when the host received the source message.
    pub const fn received_at(self) -> UnixTimeMillis {
        self.received_at
    }
}

/// One typed output from a protocol package before asset-property binding.
///
/// `device_recorded_at` preserves an optional timestamp reported by the device.
/// `received_at` independently records when the host received the source
/// message. This contract does not choose an event time or infer a missing
/// device timestamp; those decisions require an explicit downstream policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedObservation {
    id: DecodedObservationId,
    tenant_id: TenantId,
    source: SignalSource,
    measurement: DecodedMeasurement,
    times: ObservationTimes,
}

impl DecodedObservation {
    /// Constructs one complete decoder output from validated components.
    pub const fn new(
        id: DecodedObservationId,
        tenant_id: TenantId,
        source: SignalSource,
        measurement: DecodedMeasurement,
        times: ObservationTimes,
    ) -> Self {
        Self {
            id,
            tenant_id,
            source,
            measurement,
            times,
        }
    }

    /// Returns the stable replay identity of this decoder output.
    pub const fn id(&self) -> &DecodedObservationId {
        &self.id
    }

    /// Returns the tenant established by device admission.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the device, endpoint, and signal selected by the decoder.
    pub const fn source(&self) -> &SignalSource {
        &self.source
    }

    /// Returns the typed source value before conversion or semantic binding.
    pub const fn value(&self) -> &DecodedValue {
        self.measurement.value()
    }

    /// Returns the unit declared by the protocol package, when present.
    pub const fn source_unit(&self) -> Option<&UnitId> {
        self.measurement.source_unit()
    }

    /// Returns the source-reported quality of the decoded value.
    pub const fn quality(&self) -> ObservationQuality {
        self.measurement.quality()
    }

    /// Returns the timestamp reported by the device, when available.
    pub const fn device_recorded_at(&self) -> Option<UnixTimeMillis> {
        self.times.device_recorded_at()
    }

    /// Returns when the host received the source message.
    pub const fn received_at(&self) -> UnixTimeMillis {
        self.times.received_at()
    }

    /// Returns the exact package release that produced this output.
    pub const fn decoded_by(&self) -> &ProtocolPackageRef {
        self.id.decoded_by()
    }
}
