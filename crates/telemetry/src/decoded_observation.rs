use std::{error::Error, fmt};

use bigdecimal::BigDecimal;
use fleetiq_domain::{PropertyValueKind, SignalSource, TenantId, UnitId, UnixTimeMillis};

use crate::{AcceptedMessageId, ProtocolPackageId, ProtocolPackageVersion};

/// Identifies one decoder output deterministically within an accepted message.
///
/// A protocol package can emit several observations from one wire message. The
/// zero-based output index makes each identity stable when the same accepted
/// message is replayed through the same immutable package release.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DecodedObservationId {
    message_id: AcceptedMessageId,
    decoded_by: ProtocolPackageRef,
    output_index: u32,
}

impl DecodedObservationId {
    /// Constructs a stable decoded-observation identity.
    pub const fn new(
        message_id: AcceptedMessageId,
        decoded_by: ProtocolPackageRef,
        output_index: u32,
    ) -> Self {
        Self {
            message_id,
            decoded_by,
            output_index,
        }
    }

    /// Returns the durably accepted raw message that produced this output.
    pub const fn message_id(&self) -> &AcceptedMessageId {
        &self.message_id
    }

    /// Returns the exact package release that produced this interpretation.
    pub const fn decoded_by(&self) -> &ProtocolPackageRef {
        &self.decoded_by
    }

    /// Returns the zero-based position in the package's decoded output.
    pub const fn output_index(&self) -> u32 {
        self.output_index
    }
}

/// Pins decoder provenance to one immutable protocol package release.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProtocolPackageRef {
    id: ProtocolPackageId,
    version: ProtocolPackageVersion,
}

impl ProtocolPackageRef {
    /// Constructs an exact protocol package reference.
    pub const fn new(id: ProtocolPackageId, version: ProtocolPackageVersion) -> Self {
        Self { id, version }
    }

    /// Returns the stable protocol package identity.
    pub const fn id(&self) -> &ProtocolPackageId {
        &self.id
    }

    /// Returns the immutable protocol package release.
    pub const fn version(&self) -> &ProtocolPackageVersion {
        &self.version
    }
}

/// A value decoded from a device message before semantic attribution.
///
/// Values mirror the property kinds understood by the semantic model. Protocol
/// packages must retain the raw accepted message separately when a payload
/// cannot be represented by one of these typed values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodedValue {
    /// A Boolean value.
    Boolean(bool),
    /// A signed whole number.
    Integer(i64),
    /// An arbitrary-precision decimal number.
    Decimal(BigDecimal),
    /// Protocol-defined text preserved without normalization.
    Text(String),
}

impl DecodedValue {
    /// Returns the storage-level kind of this value.
    pub const fn kind(&self) -> PropertyValueKind {
        match self {
            Self::Boolean(_) => PropertyValueKind::Boolean,
            Self::Integer(_) => PropertyValueKind::Integer,
            Self::Decimal(_) => PropertyValueKind::Decimal,
            Self::Text(_) => PropertyValueKind::Text,
        }
    }

    const fn is_numeric(&self) -> bool {
        matches!(self, Self::Integer(_) | Self::Decimal(_))
    }
}

/// Source-reported confidence in a decoded value.
///
/// This status records protocol or device evidence only. Semantic validation,
/// timestamp policy, freshness, and calculator confidence remain later stages.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObservationQuality {
    /// The decoder found no source-level reason to distrust the value.
    Good,
    /// The source supplied a value with a warning or reduced confidence.
    Uncertain,
    /// The source explicitly marked the decoded value invalid.
    Invalid,
}

/// A decoded value with the source metadata needed before semantic binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedMeasurement {
    value: DecodedValue,
    source_unit: Option<UnitId>,
    quality: ObservationQuality,
}

impl DecodedMeasurement {
    /// Constructs decoded measurement data after validating its unit.
    ///
    /// # Errors
    ///
    /// Returns [`DecodedMeasurementError::UnitRequiresNumericValue`] when a
    /// source unit is attached to Boolean or text output.
    pub fn new(
        value: DecodedValue,
        source_unit: Option<UnitId>,
        quality: ObservationQuality,
    ) -> Result<Self, DecodedMeasurementError> {
        if source_unit.is_some() && !value.is_numeric() {
            return Err(DecodedMeasurementError::UnitRequiresNumericValue);
        }

        Ok(Self {
            value,
            source_unit,
            quality,
        })
    }

    /// Returns the typed value before conversion or semantic binding.
    pub const fn value(&self) -> &DecodedValue {
        &self.value
    }

    /// Returns the source unit declared by the protocol package, when present.
    pub const fn source_unit(&self) -> Option<&UnitId> {
        self.source_unit.as_ref()
    }

    /// Returns the source-reported quality.
    pub const fn quality(&self) -> ObservationQuality {
        self.quality
    }
}

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

/// Describes why decoded measurement metadata is structurally invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodedMeasurementError {
    /// Units are meaningful only for integer and decimal values.
    UnitRequiresNumericValue,
}

impl fmt::Display for DecodedMeasurementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a decoded source unit requires a numeric value")
    }
}

impl Error for DecodedMeasurementError {}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use fleetiq_domain::{AssetId, DeviceEndpointId, SourceSignalId};

    use super::*;

    fn observation(
        value: DecodedValue,
        source_unit: Option<UnitId>,
    ) -> Result<DecodedObservation, DecodedMeasurementError> {
        let measurement =
            DecodedMeasurement::new(value, source_unit, ObservationQuality::Uncertain)?;
        Ok(DecodedObservation::new(
            DecodedObservationId::new(
                AcceptedMessageId::new("message-100").expect("valid message ID"),
                ProtocolPackageRef::new(
                    ProtocolPackageId::new("example.binary").expect("valid package ID"),
                    ProtocolPackageVersion::new("2.1.0").expect("valid package version"),
                ),
                3,
            ),
            TenantId::new("tenant-a").expect("valid tenant ID"),
            SignalSource::new(
                AssetId::new("gateway-1").expect("valid asset ID"),
                DeviceEndpointId::new("can-primary").expect("valid endpoint ID"),
                SourceSignalId::new("bus.voltage").expect("valid signal ID"),
            ),
            measurement,
            ObservationTimes::new(Some(UnixTimeMillis::new(1_000)), UnixTimeMillis::new(1_200)),
        ))
    }

    #[test]
    fn preserves_source_value_times_quality_and_provenance() {
        let decoded = observation(
            DecodedValue::Decimal(BigDecimal::from_str("48.125").expect("valid decimal")),
            Some(UnitId::new("volt").expect("valid unit ID")),
        )
        .expect("observation should be valid");

        assert_eq!(decoded.id().message_id().as_str(), "message-100");
        assert_eq!(decoded.id().output_index(), 3);
        assert_eq!(decoded.tenant_id().as_str(), "tenant-a");
        assert_eq!(decoded.source().device_asset_id().as_str(), "gateway-1");
        assert_eq!(decoded.source().endpoint_id().as_str(), "can-primary");
        assert_eq!(decoded.source().signal_id().as_str(), "bus.voltage");
        assert_eq!(decoded.value().kind(), PropertyValueKind::Decimal);
        assert_eq!(decoded.source_unit().map(UnitId::as_str), Some("volt"));
        assert_eq!(decoded.quality(), ObservationQuality::Uncertain);
        assert_eq!(
            decoded.device_recorded_at(),
            Some(UnixTimeMillis::new(1_000))
        );
        assert_eq!(decoded.received_at(), UnixTimeMillis::new(1_200));
        assert_eq!(decoded.decoded_by().id().as_str(), "example.binary");
        assert_eq!(decoded.decoded_by().version().as_str(), "2.1.0");
    }

    #[test]
    fn stable_identity_distinguishes_outputs_from_one_message() {
        let message_id = AcceptedMessageId::new("message-100").expect("valid message ID");

        assert_ne!(
            DecodedObservationId::new(
                message_id.clone(),
                ProtocolPackageRef::new(
                    ProtocolPackageId::new("example.binary").expect("valid package ID"),
                    ProtocolPackageVersion::new("1").expect("valid package version"),
                ),
                0,
            ),
            DecodedObservationId::new(
                message_id,
                ProtocolPackageRef::new(
                    ProtocolPackageId::new("example.binary").expect("valid package ID"),
                    ProtocolPackageVersion::new("1").expect("valid package version"),
                ),
                1,
            )
        );
    }

    #[test]
    fn reinterpreting_a_message_with_another_package_release_has_a_new_identity() {
        let message_id = AcceptedMessageId::new("message-100").expect("valid message ID");
        let package_id = ProtocolPackageId::new("example.binary").expect("valid package ID");

        assert_ne!(
            DecodedObservationId::new(
                message_id.clone(),
                ProtocolPackageRef::new(
                    package_id.clone(),
                    ProtocolPackageVersion::new("1").expect("valid package version"),
                ),
                0,
            ),
            DecodedObservationId::new(
                message_id,
                ProtocolPackageRef::new(
                    package_id,
                    ProtocolPackageVersion::new("2").expect("valid package version"),
                ),
                0,
            )
        );
    }

    #[test]
    fn accepts_units_only_for_numeric_values() {
        let unit = || UnitId::new("volt").expect("valid unit ID");

        assert!(observation(DecodedValue::Integer(48), Some(unit())).is_ok());
        assert_eq!(
            observation(DecodedValue::Boolean(true), Some(unit())),
            Err(DecodedMeasurementError::UnitRequiresNumericValue)
        );
        assert_eq!(
            observation(DecodedValue::Text("online".to_owned()), Some(unit())),
            Err(DecodedMeasurementError::UnitRequiresNumericValue)
        );
    }

    #[test]
    fn keeps_device_and_receipt_times_independent() {
        let decoded = DecodedObservation::new(
            DecodedObservationId::new(
                AcceptedMessageId::new("message-clock-skew").expect("valid message ID"),
                ProtocolPackageRef::new(
                    ProtocolPackageId::new("example.clock").expect("valid package ID"),
                    ProtocolPackageVersion::new("1").expect("valid package version"),
                ),
                0,
            ),
            TenantId::new("tenant-a").expect("valid tenant ID"),
            SignalSource::new(
                AssetId::new("gateway-1").expect("valid asset ID"),
                DeviceEndpointId::new("onboard").expect("valid endpoint ID"),
                SourceSignalId::new("clock.sample").expect("valid signal ID"),
            ),
            DecodedMeasurement::new(DecodedValue::Integer(1), None, ObservationQuality::Good)
                .expect("measurement should be valid"),
            ObservationTimes::new(Some(UnixTimeMillis::new(2_000)), UnixTimeMillis::new(1_000)),
        );

        assert_eq!(
            decoded.device_recorded_at(),
            Some(UnixTimeMillis::new(2_000))
        );
        assert_eq!(decoded.received_at(), UnixTimeMillis::new(1_000));
    }
}
