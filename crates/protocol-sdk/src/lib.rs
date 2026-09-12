//! Contracts shared by protocol packages and the protocol host.
//!
//! Protocol packages decode device-specific payloads into [`DecodedOutput`]
//! values. The trusted host combines those values with an [`AcceptedMessage`]
//! so tenant, device, package, and receipt-time provenance cannot be supplied
//! by package code.
//!
//! This crate defines the semantic boundary. It deliberately does not define
//! a stable binary or WebAssembly ABI.

mod assembly;
mod decoded_output;
mod framing;
mod invocation;
mod reply;
mod session_state;

pub use assembly::{ObservationAssemblyError, assemble_observation, assemble_observations};
pub use decoded_output::DecodedOutput;
pub use fleetiq_domain::{DeviceEndpointId, SourceSignalId, UnitId, UnixTimeMillis};
pub use fleetiq_telemetry::{
    AcceptedMessage, DecodedMeasurement, DecodedMeasurementError, DecodedObservation, DecodedValue,
    ObservationQuality,
};
pub use framing::FrameBoundary;
pub use invocation::{FrameProcessingInput, FrameProcessingOutput, FramingInput};
pub use reply::{ProposedReply, ProposedReplyError, ReplyRelease};
pub use session_state::SessionStateAction;
