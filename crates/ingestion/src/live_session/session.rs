//! One in-memory owner per socket. Commit precedes checkpoint/buffer advancement.
mod turn;
use super::{
    LiveExecutor, LiveReplayRepository, LiveReplySink, LiveRoutingRepository, LiveSessionError,
};
use crate::{
    PinnedProtocolSession, ProtocolRoutingService, REPLAY_FRAME_LIMIT, REPLAY_STATE_LIMIT,
    RegisteredIdentityChannel,
};
use fleetiq_domain::UnixTimeMillis;
/// Maximum successfully processed frames per connection; reconnect creates new identities.
pub const MAX_SESSION_TURNS: u32 = 256;
/// One framing poll's next action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveStep {
    /// The buffer remains unchanged; read more within the same bounds/deadline.
    NeedMore,
    /// Complete effects and checkpoint committed; another buffered prefix may run.
    Committed,
}
/// Coordinates policy and durable effects without owning a socket, task or clock.
/// Any error or cancellation makes this instance unusable; discard its connection.
pub struct LiveSession<R, Q, E> {
    channel: RegisteredIdentityChannel,
    routing: ProtocolRoutingService<R>,
    repository: Q,
    pub(super) executor: E,
    pinned: Option<PinnedProtocolSession>,
    input: Vec<u8>,
    state: Vec<u8>,
    session: String,
    sequence: u32,
    polls: u32,
    received: usize,
    failed: bool,
}
impl<R: LiveRoutingRepository, Q: LiveReplayRepository, E: LiveExecutor> LiveSession<R, Q, E> {
    /// Binds explicit host scope and an independently random per-connection nonce.
    pub fn new(
        nonce: [u8; 16],
        channel: RegisteredIdentityChannel,
        routing: R,
        repository: Q,
        executor: E,
    ) -> Self {
        let session = nonce
            .into_iter()
            .flat_map(|byte| [byte >> 4, byte & 15])
            .map(|digit| {
                char::from(if digit < 10 {
                    b'0' + digit
                } else {
                    b'a' + digit - 10
                })
            })
            .collect();
        Self {
            channel,
            routing: ProtocolRoutingService::new(routing),
            repository,
            executor,
            pinned: None,
            input: Vec::new(),
            state: Vec::new(),
            session,
            sequence: 0,
            polls: 0,
            received: 0,
            failed: false,
        }
    }
    /// Remaining host-buffer allowance. A full incomplete buffer must be rejected.
    pub fn available(&self) -> usize {
        REPLAY_FRAME_LIMIT - self.input.len()
    }
    /// Whether there are bytes to frame without reading another socket chunk.
    pub fn has_input(&self) -> bool {
        !self.input.is_empty()
    }
    /// Appends socket bytes within 64-KiB buffer and one-MiB lifetime quotas.
    /// # Errors
    /// Rejects excess bytes or any reuse after a terminal failure/cancellation.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), LiveSessionError> {
        if self.failed
            || bytes.len() > self.available()
            || self.received.saturating_add(bytes.len()) > 1024 * 1024
        {
            self.failed = true;
            return Err(LiveSessionError::Rejected);
        }
        self.received += bytes.len();
        self.input.extend_from_slice(bytes);
        Ok(())
    }
    /// Processes at most one prefix and releases replies only at their minimum stage.
    /// No automatic retry is performed: uncertain writes close the connection.
    /// # Errors
    /// Any failure is terminal. Cancellation also latches failure; accepted work
    /// remains recoverable, with no reply sink retained by persistence.
    pub async fn step(
        &mut self,
        received_at: UnixTimeMillis,
        sink: &mut impl LiveReplySink,
    ) -> Result<LiveStep, LiveSessionError> {
        if self.failed
            || self.input.is_empty()
            || self.sequence >= MAX_SESSION_TURNS
            || self.polls >= 512
        {
            self.failed = true;
            return Err(LiveSessionError::Rejected);
        }
        self.failed = true;
        self.polls += 1;
        let result = self.process(received_at, sink).await;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
}
