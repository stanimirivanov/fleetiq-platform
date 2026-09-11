//! Host-owned policies for executing `FleetIQ` protocol packages.
//!
//! The host retains control of transport bytes and resource limits. Protocol
//! packages inspect borrowed input and return constrained decisions through the
//! contracts in `fleetiq-protocol-sdk`.

mod framing;
mod reply_gate;
mod session_state;

pub use framing::{FramePoll, FramePollError, InputBufferError, SessionInputBuffer, WireFrame};
pub use reply_gate::{DeferredReply, OutboundReply, ReplyGate, ReplyStagingError, StagedReply};
pub use session_state::{SessionState, SessionStateUpdateError};
