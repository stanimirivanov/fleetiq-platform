//! Validation between generated component values and semantic SDK contracts.

use std::{error::Error, fmt, num::NonZeroUsize};

use fleetiq_domain::IdentifierError;
use fleetiq_protocol_sdk::{
    DecodedMeasurement, DecodedMeasurementError, DecodedOutput, DecodedValue, DeviceEndpointId,
    FrameBoundary, FrameProcessingInput, FrameProcessingOutput, FramingInput, ObservationQuality,
    ProposedReply, ProposedReplyError, ReplyRelease, SessionStateAction, SourceSignalId, UnitId,
    UnixTimeMillis,
};

use super::bindings::exports::fleetiq::protocol_package::runtime as wit;

impl From<FramingInput<'_>> for wit::InvocationInput {
    fn from(input: FramingInput<'_>) -> Self {
        Self {
            payload: input.unread().to_vec(),
            session_state: input.session_state().to_vec(),
        }
    }
}

impl From<FrameProcessingInput<'_>> for wit::InvocationInput {
    fn from(input: FrameProcessingInput<'_>) -> Self {
        Self {
            payload: input.frame().to_vec(),
            session_state: input.session_state().to_vec(),
        }
    }
}

impl TryFrom<wit::FrameBoundary> for FrameBoundary {
    type Error = ComponentOutputError;

    fn try_from(boundary: wit::FrameBoundary) -> Result<Self, Self::Error> {
        match boundary {
            wit::FrameBoundary::NeedMoreData => Ok(Self::NeedMoreData),
            wit::FrameBoundary::Frame(length) => {
                let length = usize::try_from(length)
                    .map_err(|_| ComponentOutputError::FrameLengthUnsupported { length })?;
                let length =
                    NonZeroUsize::new(length).ok_or(ComponentOutputError::ZeroFrameLength)?;
                Ok(Self::Frame { length })
            }
        }
    }
}

impl TryFrom<wit::FrameProcessingOutput> for FrameProcessingOutput {
    type Error = ComponentOutputError;

    fn try_from(output: wit::FrameProcessingOutput) -> Result<Self, Self::Error> {
        let decoded_outputs = output
            .decoded_outputs
            .into_iter()
            .enumerate()
            .map(|(index, output)| convert_decoded_output(index, output))
            .collect::<Result<Vec<_>, _>>()?;
        let replies = output
            .replies
            .into_iter()
            .enumerate()
            .map(|(index, reply)| convert_reply(index, reply))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self::new(
            decoded_outputs,
            convert_state_action(output.session_state),
            replies,
        ))
    }
}

fn convert_decoded_output(
    index: usize,
    output: wit::DecodedOutput,
) -> Result<DecodedOutput, ComponentOutputError> {
    let endpoint_id = DeviceEndpointId::new(output.endpoint_id).map_err(|source| {
        ComponentOutputError::InvalidEndpointId {
            output_index: index,
            source,
        }
    })?;
    let signal_id = SourceSignalId::new(output.signal_id).map_err(|source| {
        ComponentOutputError::InvalidSignalId {
            output_index: index,
            source,
        }
    })?;
    let value = convert_value(index, output.value)?;
    let source_unit = output
        .source_unit
        .map(UnitId::new)
        .transpose()
        .map_err(|source| ComponentOutputError::InvalidSourceUnit {
            output_index: index,
            source,
        })?;
    let measurement = DecodedMeasurement::new(value, source_unit, convert_quality(output.quality))
        .map_err(|source| ComponentOutputError::InvalidMeasurement {
            output_index: index,
            source,
        })?;

    Ok(DecodedOutput::new(
        endpoint_id,
        signal_id,
        measurement,
        output.device_recorded_at_ms.map(UnixTimeMillis::new),
    ))
}

fn convert_value(
    output_index: usize,
    value: wit::DecodedValue,
) -> Result<DecodedValue, ComponentOutputError> {
    match value {
        wit::DecodedValue::Boolean(value) => Ok(DecodedValue::Boolean(value)),
        wit::DecodedValue::Integer(value) => Ok(DecodedValue::Integer(value)),
        wit::DecodedValue::Decimal(value) => value
            .parse()
            .map(DecodedValue::Decimal)
            .map_err(|_| ComponentOutputError::InvalidDecimal { output_index }),
        wit::DecodedValue::Text(value) => Ok(DecodedValue::Text(value)),
    }
}

const fn convert_quality(quality: wit::ObservationQuality) -> ObservationQuality {
    match quality {
        wit::ObservationQuality::Good => ObservationQuality::Good,
        wit::ObservationQuality::Uncertain => ObservationQuality::Uncertain,
        wit::ObservationQuality::Invalid => ObservationQuality::Invalid,
    }
}

fn convert_reply(
    index: usize,
    reply: wit::ProposedReply,
) -> Result<ProposedReply, ComponentOutputError> {
    ProposedReply::new(reply.bytes, convert_reply_release(reply.release)).map_err(|source| {
        ComponentOutputError::InvalidReply {
            reply_index: index,
            source,
        }
    })
}

const fn convert_reply_release(release: wit::ReplyRelease) -> ReplyRelease {
    match release {
        wit::ReplyRelease::AfterProcessing => ReplyRelease::AfterProcessing,
        wit::ReplyRelease::AfterDurableAcceptance => ReplyRelease::AfterDurableAcceptance,
    }
}

fn convert_state_action(action: wit::SessionStateAction) -> SessionStateAction {
    match action {
        wit::SessionStateAction::Keep => SessionStateAction::Keep,
        wit::SessionStateAction::Replace(bytes) => {
            SessionStateAction::Replace(bytes.into_boxed_slice())
        }
    }
}

/// Invalid data lifted from an untrusted protocol component.
///
/// Every variant identifies the output position and field class without
/// retaining package-controlled values. This lets callers diagnose package
/// conformance failures without copying raw telemetry into routine logs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComponentOutputError {
    /// A component reported zero as a complete frame length.
    ZeroFrameLength,
    /// The component's 32-bit frame length cannot be represented by this host.
    FrameLengthUnsupported {
        /// Length reported through the component interface.
        length: u32,
    },
    /// A decoded output contains an invalid endpoint identifier.
    InvalidEndpointId {
        /// Zero-based decoded-output position.
        output_index: usize,
        /// Violated identifier invariant.
        source: IdentifierError,
    },
    /// A decoded output contains an invalid signal identifier.
    InvalidSignalId {
        /// Zero-based decoded-output position.
        output_index: usize,
        /// Violated identifier invariant.
        source: IdentifierError,
    },
    /// A decoded output contains invalid decimal text.
    InvalidDecimal {
        /// Zero-based decoded-output position.
        output_index: usize,
    },
    /// A decoded output contains an invalid source-unit identifier.
    InvalidSourceUnit {
        /// Zero-based decoded-output position.
        output_index: usize,
        /// Violated identifier invariant.
        source: IdentifierError,
    },
    /// A decoded value and its measurement metadata are incompatible.
    InvalidMeasurement {
        /// Zero-based decoded-output position.
        output_index: usize,
        /// Violated measurement invariant.
        source: DecodedMeasurementError,
    },
    /// A proposed reply violates the semantic reply contract.
    InvalidReply {
        /// Zero-based reply position.
        reply_index: usize,
        /// Violated reply invariant.
        source: ProposedReplyError,
    },
}

impl fmt::Display for ComponentOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroFrameLength => {
                formatter.write_str("protocol component reported a zero-byte frame")
            }
            Self::FrameLengthUnsupported { length } => write!(
                formatter,
                "protocol component frame length {length} cannot be represented by this host"
            ),
            Self::InvalidEndpointId {
                output_index,
                source,
            } => write!(
                formatter,
                "protocol component output {output_index} has an invalid endpoint ID: {source}"
            ),
            Self::InvalidSignalId {
                output_index,
                source,
            } => write!(
                formatter,
                "protocol component output {output_index} has an invalid signal ID: {source}"
            ),
            Self::InvalidDecimal { output_index } => write!(
                formatter,
                "protocol component output {output_index} has invalid decimal text"
            ),
            Self::InvalidSourceUnit {
                output_index,
                source,
            } => write!(
                formatter,
                "protocol component output {output_index} has an invalid source unit: {source}"
            ),
            Self::InvalidMeasurement {
                output_index,
                source,
            } => write!(
                formatter,
                "protocol component output {output_index} has invalid measurement metadata: {source}"
            ),
            Self::InvalidReply {
                reply_index,
                source,
            } => write!(
                formatter,
                "protocol component reply {reply_index} is invalid: {source}"
            ),
        }
    }
}

impl Error for ComponentOutputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidEndpointId { source, .. }
            | Self::InvalidSignalId { source, .. }
            | Self::InvalidSourceUnit { source, .. } => Some(source),
            Self::InvalidMeasurement { source, .. } => Some(source),
            Self::InvalidReply { source, .. } => Some(source),
            Self::ZeroFrameLength
            | Self::FrameLengthUnsupported { .. }
            | Self::InvalidDecimal { .. } => None,
        }
    }
}
