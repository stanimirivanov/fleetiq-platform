//! Bounded connection-local ingestion; only the reply sink owns transport delivery.
mod session;
#[cfg(test)]
mod tests;
use crate::{
    ProtocolPackageRelease, ProtocolRoutingRepository, RegisteredIdentityChannel, ReplayLease,
    ReplayRepository,
};
use fleetiq_protocol_sdk::{DecodedOutput, DeviceIdentityClaim, ProposedReply};
use fleetiq_telemetry::AcceptedMessage;
pub use session::{LiveSession, LiveStep};
use std::{error::Error, fmt, future::Future};
/// Live routing additionally needs bounded admission before an unknown peer executes.
pub trait LiveRoutingRepository: ProtocolRoutingRepository {
    /// Reads a current active tenant's exact admitted/stored identity-v1 release.
    /// # Errors
    /// Reports storage or reconstruction failure. Absence grants no execution.
    fn bootstrap(
        &self,
        channel: &RegisteredIdentityChannel,
    ) -> impl Future<Output = Result<Option<ProtocolPackageRelease>, Self::Error>> + Send;
}
/// Live work must claim its exact frame, never consume unrelated historical work.
pub trait LiveReplayRepository: ReplayRepository {
    /// Claims only this immutable accepted identity under the ordinary lease policy.
    /// # Errors
    /// Reports storage failure. None means a competing worker or no eligible frame.
    fn claim_exact(
        &self,
        message: &AcceptedMessage,
    ) -> impl Future<Output = Result<Option<ReplayLease>, Self::Error>> + Send;
}
/// Closed execution failure; guest-controlled diagnostics never cross the port.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveExecutionFailure {
    /// Input, admission or proposed effects failed host policy.
    Rejected,
    /// Execution infrastructure, deadline or cancellation failed.
    Unavailable,
}
/// A complete prefix; proposals still need parent-side validation and routing.
pub struct LivePrepared {
    /// Exact complete prefix within the supplied unread bytes.
    pub consumed: usize,
    /// Required only for the first processed frame.
    pub identity: Option<DeviceIdentityClaim>,
    /// Complete bounded successor state.
    pub state: Vec<u8>,
    /// Ordered decoded source outputs, without trusted provenance.
    pub outputs: Vec<DecodedOutput>,
    /// Reply proposals, retaining their minimum release conditions.
    pub replies: Vec<ProposedReply>,
}
/// Execution port for an exact trusted release, never a socket.
/// The adapter's guest request omits host tenant/device identity and storage authority.
pub trait LiveExecutor: Send + Sync {
    /// Frames at most one prefix; None means more data with no proposed effects.
    /// # Errors
    /// Returns a closed rejection or infrastructure failure; no guest diagnostic escapes.
    fn prepare(
        &self,
        release: &ProtocolPackageRelease,
        unread: &[u8],
        state: &[u8],
        identify: bool,
    ) -> impl Future<Output = Result<Option<LivePrepared>, LiveExecutionFailure>> + Send;
}
/// The originating connection supplies this write-only, deadline-bound port.
/// It must never be retained by a queue, receipt or historical worker.
pub trait LiveReplySink: Send {
    /// Writes one already released reply or reports delivery uncertainty.
    /// # Errors
    /// Reports an uncertain/failed originating-connection write. Never retry on another socket.
    fn send(&mut self, bytes: &[u8]) -> impl Future<Output = std::io::Result<()>> + Send;
}
/// Session failures are terminal; dropping the connection preserves durable recovery.
pub enum LiveSessionError {
    /// Frame, state, quota or reconstructed proposal violated policy.
    Rejected,
    /// Current tenant, identifier, route, release or admission is ineligible.
    Ineligible,
    /// Another worker owns the accepted turn, or its lease was lost.
    LostLease,
    /// Isolated execution did not produce a validated turn.
    Execution(LiveExecutionFailure),
    /// Storage failure; its source is retained privately for controlled diagnostics.
    Storage(Box<dyn Error + Send + Sync>),
    /// Socket delivery is uncertain; no replacement socket may receive this reply.
    Transport(std::io::Error),
}
impl fmt::Display for LiveSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Rejected => "TCP session policy rejected",
            Self::Ineligible => "TCP session route unavailable",
            Self::LostLease => "TCP session completion ownership unavailable",
            Self::Execution(_) => "TCP session execution unavailable",
            Self::Storage(_) => "TCP session storage failed; durable recovery may be required",
            Self::Transport(_) => "TCP reply delivery uncertain",
        })
    }
}
impl fmt::Debug for LiveSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for LiveSessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(e) => Some(e.as_ref()),
            Self::Transport(e) => Some(e),
            _ => None,
        }
    }
}
