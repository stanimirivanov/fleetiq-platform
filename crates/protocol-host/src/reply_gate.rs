use std::{error::Error, fmt, num::NonZeroUsize};

use fleetiq_protocol_sdk::{ProposedReply, ReplyRelease};
use fleetiq_telemetry::{AcceptedMessage, AcceptedMessageId};

/// Host policy that validates and stages protocol-package replies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplyGate {
    limit: NonZeroUsize,
}

impl ReplyGate {
    /// Creates a reply gate with the given hard per-reply byte limit.
    pub const fn new(limit: NonZeroUsize) -> Self {
        Self { limit }
    }

    /// Validates and stages a package-proposed reply.
    ///
    /// Replies released after processing become immediately accessible to host
    /// transport code. Replies tied to durable acceptance remain opaque behind a
    /// [`DeferredReply`] until the caller supplies the corresponding accepted
    /// message.
    ///
    /// # Errors
    ///
    /// Returns [`ReplyStagingError::LimitExceeded`] when the proposed reply is
    /// larger than the configured limit.
    pub fn stage(&self, proposed: ProposedReply) -> Result<StagedReply, ReplyStagingError> {
        if proposed.len() > self.limit.get() {
            return Err(ReplyStagingError::LimitExceeded {
                limit: self.limit.get(),
                attempted: proposed.len(),
            });
        }

        let (bytes, release) = proposed.into_parts();
        Ok(match release {
            ReplyRelease::AfterProcessing => {
                StagedReply::Ready(OutboundReply::after_processing(bytes))
            }
            ReplyRelease::AfterDurableAcceptance => {
                StagedReply::AwaitingDurableAcceptance(DeferredReply(bytes))
            }
        })
    }

    /// Returns the configured hard per-reply byte limit.
    pub const fn limit(&self) -> NonZeroUsize {
        self.limit
    }
}

/// A validated reply classified by its remaining release condition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StagedReply {
    /// The reply may be passed to host transport code.
    Ready(OutboundReply),
    /// The reply must wait for durable acceptance of its triggering frame.
    AwaitingDurableAcceptance(DeferredReply),
}

/// A validated reply that cannot expose its bytes until durable acceptance.
#[derive(Clone, Eq, PartialEq)]
pub struct DeferredReply(Box<[u8]>);

impl DeferredReply {
    /// Releases the reply after the triggering frame has been durably accepted.
    ///
    /// The caller must supply the [`AcceptedMessage`] created for the frame whose
    /// processing proposed this reply. Its identity is retained on the resulting
    /// [`OutboundReply`] as release evidence.
    pub fn release(self, accepted: &AcceptedMessage) -> OutboundReply {
        OutboundReply {
            bytes: self.0,
            durable_acceptance: Some(accepted.id().clone()),
        }
    }

    /// Returns the deferred reply length without exposing its bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the deferred reply has no bytes.
    ///
    /// A deferred reply originates from a nonempty [`ProposedReply`].
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for DeferredReply {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeferredReply")
            .field("length", &self.len())
            .finish_non_exhaustive()
    }
}

/// Reply bytes validated and released for host-controlled transport output.
#[derive(Clone, Eq, PartialEq)]
pub struct OutboundReply {
    bytes: Box<[u8]>,
    durable_acceptance: Option<AcceptedMessageId>,
}

impl OutboundReply {
    fn after_processing(bytes: Box<[u8]>) -> Self {
        Self {
            bytes,
            durable_acceptance: None,
        }
    }

    /// Returns the bytes that host transport code may write.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consumes the reply and returns the bytes for host transport output.
    pub fn into_bytes(self) -> Box<[u8]> {
        self.bytes
    }

    /// Returns the reply length in bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns whether the reply has no bytes.
    ///
    /// An outbound reply originates from a nonempty [`ProposedReply`].
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Returns durable-acceptance evidence when the release required it.
    pub const fn durable_acceptance(&self) -> Option<&AcceptedMessageId> {
        self.durable_acceptance.as_ref()
    }
}

impl fmt::Debug for OutboundReply {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OutboundReply")
            .field("length", &self.len())
            .field("durable_acceptance", &self.durable_acceptance)
            .finish_non_exhaustive()
    }
}

/// Failure to stage a package-proposed reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyStagingError {
    /// The reply exceeds the host's hard per-reply byte limit.
    LimitExceeded {
        /// Maximum bytes allowed in one reply.
        limit: usize,
        /// Bytes in the rejected reply.
        attempted: usize,
    },
}

impl fmt::Display for ReplyStagingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded { limit, attempted } => write!(
                formatter,
                "protocol package proposed a {attempted}-byte reply, exceeding the {limit}-byte limit"
            ),
        }
    }
}

impl Error for ReplyStagingError {}

#[cfg(test)]
mod tests;
