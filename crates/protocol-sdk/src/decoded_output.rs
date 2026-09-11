use fleetiq_domain::{DeviceEndpointId, SourceSignalId, UnitId, UnixTimeMillis};
use fleetiq_telemetry::{DecodedMeasurement, DecodedValue, ObservationQuality};

/// One device-scoped fact emitted by a protocol package.
///
/// A protocol package identifies where a value came from within the device
/// and describes the decoded measurement. It cannot choose tenant, device,
/// protocol-package, or host receipt-time provenance; the protocol host adds
/// those fields when it assembles a decoded observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedOutput {
    endpoint_id: DeviceEndpointId,
    signal_id: SourceSignalId,
    measurement: DecodedMeasurement,
    device_recorded_at: Option<UnixTimeMillis>,
}

impl DecodedOutput {
    /// Creates an output emitted by a protocol package.
    pub fn new(
        endpoint_id: DeviceEndpointId,
        signal_id: SourceSignalId,
        measurement: DecodedMeasurement,
        device_recorded_at: Option<UnixTimeMillis>,
    ) -> Self {
        Self {
            endpoint_id,
            signal_id,
            measurement,
            device_recorded_at,
        }
    }

    /// Returns the device endpoint that produced the value.
    pub fn endpoint_id(&self) -> &DeviceEndpointId {
        &self.endpoint_id
    }

    /// Returns the protocol package's stable signal identifier.
    pub fn signal_id(&self) -> &SourceSignalId {
        &self.signal_id
    }

    /// Returns the decoded measurement, including unit and quality.
    pub fn measurement(&self) -> &DecodedMeasurement {
        &self.measurement
    }

    /// Returns the decoded value.
    pub fn value(&self) -> &DecodedValue {
        self.measurement.value()
    }

    /// Returns the source unit reported or implied by the protocol.
    pub fn source_unit(&self) -> Option<&UnitId> {
        self.measurement.source_unit()
    }

    /// Returns the package-assigned observation quality.
    pub fn quality(&self) -> ObservationQuality {
        self.measurement.quality()
    }

    /// Returns the device timestamp when the payload carries one.
    pub fn device_recorded_at(&self) -> Option<UnixTimeMillis> {
        self.device_recorded_at
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        DeviceEndpointId,
        SourceSignalId,
        DecodedMeasurement,
        Option<UnixTimeMillis>,
    ) {
        (
            self.endpoint_id,
            self.signal_id,
            self.measurement,
            self.device_recorded_at,
        )
    }
}
