//! Protocol-neutral telemetry contracts for `FleetIQ`.
//!
//! Protocol packages produce decoded observations through these types. The
//! crate contains no socket, broker, database, or plugin-runtime integration,
//! allowing native and sandboxed decoders to share the same semantic boundary.

mod accepted_message;
mod decoded_observation;
mod identifier;
mod protocol_artifact;

pub use accepted_message::{AcceptedMessage, AcceptedPayload};
pub use decoded_observation::{
    DecodedMeasurement, DecodedMeasurementError, DecodedObservation, DecodedObservationId,
    DecodedValue, ObservationQuality, ObservationTimes, ProtocolPackageRef,
};
pub use identifier::{
    AcceptedMessageId, PipelineIdentifierError, ProtocolPackageId, ProtocolPackageVersion,
};
pub use protocol_artifact::ProtocolArtifactDigest;
