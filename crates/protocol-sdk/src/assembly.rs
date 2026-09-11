use std::{error::Error, fmt};

use fleetiq_domain::SignalSource;
use fleetiq_telemetry::{AcceptedMessage, DecodedObservation, ObservationTimes};

use crate::DecodedOutput;

/// Combines one package output with trusted host provenance.
///
/// `output_index` must be stable for the same accepted message and protocol
/// package version. The host uses it to derive the observation identifier.
pub fn assemble_observation(
    message: &AcceptedMessage,
    output_index: u32,
    output: DecodedOutput,
) -> DecodedObservation {
    let (endpoint_id, signal_id, measurement, device_recorded_at) = output.into_parts();

    DecodedObservation::new(
        message.decoded_observation_id(output_index),
        message.tenant_id().clone(),
        SignalSource::new(message.device_asset_id().clone(), endpoint_id, signal_id),
        measurement,
        ObservationTimes::new(device_recorded_at, message.received_at()),
    )
}

/// Combines an ordered sequence of package outputs with trusted host provenance.
///
/// Sequence order is part of deterministic replay: the same accepted message
/// and protocol package version must emit outputs in the same order.
///
/// # Errors
///
/// Returns [`ObservationAssemblyError::OutputIndexOverflow`] if the sequence
/// contains more outputs than can be indexed by `u32`.
pub fn assemble_observations(
    message: &AcceptedMessage,
    outputs: impl IntoIterator<Item = DecodedOutput>,
) -> Result<Vec<DecodedObservation>, ObservationAssemblyError> {
    outputs
        .into_iter()
        .enumerate()
        .map(|(index, output)| {
            let output_index = u32::try_from(index)
                .map_err(|_| ObservationAssemblyError::OutputIndexOverflow { index })?;
            Ok(assemble_observation(message, output_index, output))
        })
        .collect()
}

/// Failure while the host assembles decoded observations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservationAssemblyError {
    /// The package emitted more outputs than the observation ID can index.
    OutputIndexOverflow {
        /// Zero-based output position that cannot be represented.
        index: usize,
    },
}

impl fmt::Display for ObservationAssemblyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputIndexOverflow { index } => {
                write!(formatter, "decoded output index {index} exceeds u32::MAX")
            }
        }
    }
}

impl Error for ObservationAssemblyError {}

#[cfg(test)]
mod tests {
    use fleetiq_domain::{AssetId, TenantId};
    use fleetiq_telemetry::{
        AcceptedMessageId, AcceptedPayload, ProtocolPackageId, ProtocolPackageRef,
        ProtocolPackageVersion,
    };

    use super::{assemble_observation, assemble_observations};
    use crate::{
        AcceptedMessage, DecodedMeasurement, DecodedOutput, DecodedValue, DeviceEndpointId,
        ObservationQuality, SourceSignalId, UnixTimeMillis,
    };

    #[test]
    fn assembly_adds_host_owned_context_and_deterministic_output_indices() {
        let message = accepted_message();
        let first = output("can.1", "engine.speed", 1_720);
        let second = output("can.2", "battery.voltage", 672);

        let observations = assemble_observations(&message, [first, second]).unwrap();

        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].id(), &message.decoded_observation_id(0));
        assert_eq!(observations[1].id(), &message.decoded_observation_id(1));
        for observation in observations {
            assert_eq!(observation.tenant_id(), message.tenant_id());
            assert_eq!(
                observation.source().device_asset_id(),
                message.device_asset_id()
            );
            assert_eq!(observation.decoded_by(), message.protocol_package());
            assert_eq!(observation.received_at(), message.received_at());
        }
    }

    #[test]
    fn single_output_preserves_package_owned_fields() {
        let message = accepted_message();
        let output = output("can.1", "engine.speed", 1_720);

        let observation = assemble_observation(&message, 7, output);

        assert_eq!(observation.source().endpoint_id().as_str(), "can.1");
        assert_eq!(observation.source().signal_id().as_str(), "engine.speed");
        assert_eq!(observation.value(), &DecodedValue::Integer(1_720));
        assert_eq!(
            observation.device_recorded_at(),
            Some(UnixTimeMillis::new(1_725_000_000_000))
        );
    }

    #[test]
    fn empty_output_sequence_produces_no_observations() {
        let observations = assemble_observations(&accepted_message(), []).unwrap();

        assert!(observations.is_empty());
    }

    fn accepted_message() -> AcceptedMessage {
        AcceptedMessage::new(
            AcceptedMessageId::new("message-a").unwrap(),
            TenantId::new("tenant-a").unwrap(),
            AssetId::new("device-a").unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("example.protocol").unwrap(),
                ProtocolPackageVersion::new("3").unwrap(),
            ),
            UnixTimeMillis::new(1_725_000_000_100),
            AcceptedPayload::new([0x01, 0x02]),
        )
    }

    fn output(endpoint: &str, signal: &str, value: i64) -> DecodedOutput {
        DecodedOutput::new(
            DeviceEndpointId::new(endpoint).unwrap(),
            SourceSignalId::new(signal).unwrap(),
            DecodedMeasurement::new(DecodedValue::Integer(value), None, ObservationQuality::Good)
                .unwrap(),
            Some(UnixTimeMillis::new(1_725_000_000_000)),
        )
    }
}
