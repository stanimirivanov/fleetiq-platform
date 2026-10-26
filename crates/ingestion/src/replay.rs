//! Durable decoding policy and ports, independent of sockets and Wasmtime.
mod worker;
use crate::{DecodedObservationBatch, ProtocolAdmissionWorld};
use fleetiq_protocol_sdk::{DecodedOutput, assemble_observations};
use fleetiq_telemetry::{AcceptedMessage, ProtocolArtifactDigest};
use std::{error::Error, fmt, future::Future};
pub use worker::{ReplayExecutor, ReplayFailure, ReplayTick, ReplayWorker};
/// Maximum complete framed input for replay policy version 1.
pub const REPLAY_FRAME_LIMIT: usize = 65_536;
/// Maximum explicit protocol state for replay policy version 1.
pub const REPLAY_STATE_LIMIT: usize = 16_384;
/// Maximum decoded outputs for replay policy version 1.
pub const REPLAY_OUTPUT_LIMIT: usize = 1200;
/// Durable execution policy: fixed limits and deterministic wire interpretation.
pub const REPLAY_POLICY: i32 = 1;
/// One immutable replay input. Trusted transport supplies identity and sequencing.
/// Raw bytes and state are deliberately absent from Debug output.
#[derive(Clone, Eq, PartialEq)]
pub struct ReplayWork {
    /// Complete accepted frame and host-owned provenance.
    pub message: AcceptedMessage,
    /// Opaque host-generated session key; never a device identifier or credential.
    pub session: String,
    /// One-based turn number; no successor may enqueue before completion.
    pub sequence: u32,
    /// Exact immutable component content, never a latest-version lookup.
    pub digest: ProtocolArtifactDigest,
    /// Exact admitted WIT world.
    pub world: ProtocolAdmissionWorld,
    /// Pinned host policy and dependency graph, independent of package identity.
    pub execution: [u8; 32],
    /// Checkpoint before this frame, copied durably for deterministic replay.
    pub pre_state: Vec<u8>,
}
impl ReplayWork {
    /// Validates bounded replay input before storage or execution.
    /// # Errors
    /// Rejects invalid host session identity, sequence, state or frame size.
    pub fn validate(&self) -> Result<(), InvalidReplay> {
        if self.execution != fleetiq_protocol_sdk::replay_wire::replay_fingerprint()
            || self.session.is_empty()
            || self.session.len() > 128
            || !self
                .session
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || self.sequence == 0
            || self.message.payload().as_bytes().len() > REPLAY_FRAME_LIMIT
            || self.pre_state.len() > REPLAY_STATE_LIMIT
            || (self.sequence == 1 && !self.pre_state.is_empty())
        {
            return Err(InvalidReplay);
        }
        Ok(())
    }
}
impl fmt::Debug for ReplayWork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReplayWork")
            .field("message", &self.message.id())
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}
/// Fenced lease. Its generation must still own an unexpired claim at commit.
#[derive(Clone, Debug)]
pub struct ReplayLease {
    /// Immutable input read from durable storage.
    pub work: ReplayWork,
    /// Strictly increasing database-owned claim generation.
    pub generation: i64,
}
/// Validated output and successor state; deliberately contains no replies.
pub struct ReplayEffects {
    /// Ordered source outputs, before trusted provenance assembly.
    pub outputs: Vec<DecodedOutput>,
    /// Complete checkpoint after successful host validation.
    pub state: Vec<u8>,
}
impl ReplayEffects {
    /// Assembles trusted provenance and enforces execution-policy bounds.
    /// # Errors
    /// Rejects invalid input, excess effects or invalid observation identities.
    pub fn batch(
        self,
        work: &ReplayWork,
    ) -> Result<(DecodedObservationBatch, Vec<u8>), InvalidReplay> {
        work.validate()?;
        if self.outputs.len() > REPLAY_OUTPUT_LIMIT || self.state.len() > REPLAY_STATE_LIMIT {
            return Err(InvalidReplay);
        }
        let observations =
            assemble_observations(&work.message, self.outputs).map_err(|_| InvalidReplay)?;
        let batch = DecodedObservationBatch::new(work.message.clone(), observations)
            .map_err(|_| InvalidReplay)?;
        Ok((batch, self.state))
    }
}
/// Invalid bounded replay contract, without payload-controlled diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidReplay;
impl fmt::Display for InvalidReplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid replay contract")
    }
}
impl Error for InvalidReplay {}
/// Consumer-owned durable queue and atomic completion boundary.
pub trait ReplayRepository: Send + Sync {
    /// Storage failure; expected conflicts are returned as false.
    type Error: Error + Send + Sync + 'static;
    /// Atomically accepts a frame and its replay context; exact retries succeed.
    /// # Errors
    /// Returns storage or invariant failure; false means a conflicting session/input.
    fn enqueue(&self, work: &ReplayWork) -> impl Future<Output = Result<bool, Self::Error>> + Send;
    /// Claims one eligible turn for 30 seconds, at most three attempts.
    /// # Errors
    /// Returns storage/reconstruction failure. Expired exhausted work is quarantined.
    fn claim(&self) -> impl Future<Output = Result<Option<ReplayLease>, Self::Error>> + Send;
    /// Commits the complete batch, checkpoint and completion atomically.
    /// # Errors
    /// Returns storage/collision failure. False means stale/expired ownership.
    fn complete(
        &self,
        lease: &ReplayLease,
        batch: &DecodedObservationBatch,
        state: &[u8],
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send;
    /// Quarantines a protocol rejection without advancing the session.
    /// # Errors
    /// Returns storage failure; false means the lease no longer owns the turn.
    fn poison(&self, lease: &ReplayLease)
    -> impl Future<Output = Result<bool, Self::Error>> + Send;
}
#[cfg(test)]
mod tests;
