//! Atomic validation and application of complete package processing results.

use std::{convert::Infallible, error::Error, fmt, num::NonZeroUsize};

use fleetiq_protocol_sdk::{DecodedOutput, FrameProcessingOutput};

use crate::{ReplyGate, ReplyStagingError, SessionState, SessionStateUpdateError, StagedReply};

/// Hard host limits for one complete frame-processing result.
///
/// The decoded-output and reply count limits bound collection cardinality.
/// Reply bytes have both an individual ceiling and an aggregate ceiling so a
/// package cannot evade the frame-level budget by emitting many small replies.
/// Session-state bytes remain governed by the limit on [`SessionState`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameProcessingLimits {
    decoded_outputs: NonZeroUsize,
    replies: NonZeroUsize,
    reply_bytes: NonZeroUsize,
    total_reply_bytes: NonZeroUsize,
}

impl FrameProcessingLimits {
    /// Creates explicit limits for one frame-processing result.
    pub const fn new(
        max_decoded_outputs: NonZeroUsize,
        max_replies: NonZeroUsize,
        max_reply_bytes: NonZeroUsize,
        max_total_reply_bytes: NonZeroUsize,
    ) -> Self {
        Self {
            decoded_outputs: max_decoded_outputs,
            replies: max_replies,
            reply_bytes: max_reply_bytes,
            total_reply_bytes: max_total_reply_bytes,
        }
    }

    /// Returns the maximum number of decoded outputs per frame.
    pub const fn max_decoded_outputs(self) -> NonZeroUsize {
        self.decoded_outputs
    }

    /// Returns the maximum number of reply proposals per frame.
    pub const fn max_replies(self) -> NonZeroUsize {
        self.replies
    }

    /// Returns the maximum byte length of one reply.
    pub const fn max_reply_bytes(self) -> NonZeroUsize {
        self.reply_bytes
    }

    /// Returns the maximum combined reply bytes per frame.
    pub const fn max_total_reply_bytes(self) -> NonZeroUsize {
        self.total_reply_bytes
    }
}

/// Host gate that validates and applies one complete package result atomically.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameProcessingGate {
    limits: FrameProcessingLimits,
    reply_gate: ReplyGate,
}

impl FrameProcessingGate {
    /// Creates a processing gate with explicit per-frame limits.
    pub const fn new(limits: FrameProcessingLimits) -> Self {
        Self {
            reply_gate: ReplyGate::new(limits.reply_bytes),
            limits,
        }
    }

    /// Validates and applies every proposed effect from one processed frame.
    ///
    /// Validation checks decoded-output cardinality, reply cardinality, total
    /// reply bytes, each individual reply, and the session-state replacement.
    /// Replies are staged in package order before the state action is applied.
    /// The returned value becomes visible only after every check succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`FrameProcessingValidationError`] for the first violated limit.
    /// Any failure preserves `session_state` and exposes no decoded outputs or
    /// staged replies to the caller.
    pub fn apply(
        &self,
        session_state: &mut SessionState,
        proposed: FrameProcessingOutput,
    ) -> Result<AppliedFrameProcessingOutput, FrameProcessingValidationError> {
        let (decoded_outputs, state_action, replies) = proposed.into_parts();

        if decoded_outputs.len() > self.limits.decoded_outputs.get() {
            return Err(FrameProcessingValidationError::DecodedOutputLimitExceeded {
                limit: self.limits.decoded_outputs.get(),
                attempted: decoded_outputs.len(),
            });
        }

        if replies.len() > self.limits.replies.get() {
            return Err(FrameProcessingValidationError::ReplyCountLimitExceeded {
                limit: self.limits.replies.get(),
                attempted: replies.len(),
            });
        }

        let total_reply_bytes = replies
            .iter()
            .fold(0_usize, |total, reply| total.saturating_add(reply.len()));
        if total_reply_bytes > self.limits.total_reply_bytes.get() {
            return Err(FrameProcessingValidationError::ReplyBytesLimitExceeded {
                limit: self.limits.total_reply_bytes.get(),
                attempted: total_reply_bytes,
            });
        }

        let staged_replies = replies
            .into_iter()
            .enumerate()
            .map(|(index, reply)| {
                self.reply_gate
                    .stage(reply)
                    .map_err(|source| FrameProcessingValidationError::Reply { index, source })
            })
            .collect::<Result<Vec<_>, _>>()?;

        match session_state.update(|_| Ok::<_, Infallible>(state_action)) {
            Ok(()) => {}
            Err(SessionStateUpdateError::LimitExceeded { limit, attempted }) => {
                return Err(FrameProcessingValidationError::SessionStateLimitExceeded {
                    limit,
                    attempted,
                });
            }
            Err(SessionStateUpdateError::Package(never)) => match never {},
        }

        Ok(AppliedFrameProcessingOutput {
            decoded_outputs,
            staged_replies,
        })
    }

    /// Returns the configured per-frame processing limits.
    pub const fn limits(self) -> FrameProcessingLimits {
        self.limits
    }
}

/// Successfully validated package outputs after the state action was applied.
///
/// Both collections retain package order. Reply variants preserve whether each
/// reply is ready for transport or still requires durable-acceptance evidence.
#[derive(Clone, Eq, PartialEq)]
pub struct AppliedFrameProcessingOutput {
    decoded_outputs: Vec<DecodedOutput>,
    staged_replies: Vec<StagedReply>,
}

impl AppliedFrameProcessingOutput {
    /// Returns validated decoded outputs in package-emission order.
    pub fn decoded_outputs(&self) -> &[DecodedOutput] {
        &self.decoded_outputs
    }

    /// Returns validated, classified replies in package-emission order.
    pub fn staged_replies(&self) -> &[StagedReply] {
        &self.staged_replies
    }

    /// Consumes the result into values used by later trusted-host stages.
    pub fn into_parts(self) -> (Vec<DecodedOutput>, Vec<StagedReply>) {
        (self.decoded_outputs, self.staged_replies)
    }
}

impl fmt::Debug for AppliedFrameProcessingOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppliedFrameProcessingOutput")
            .field("decoded_output_count", &self.decoded_outputs.len())
            .field("staged_reply_count", &self.staged_replies.len())
            .finish_non_exhaustive()
    }
}

/// A host limit rejected a complete package processing result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FrameProcessingValidationError {
    /// The package emitted too many decoded outputs for one frame.
    DecodedOutputLimitExceeded {
        /// Maximum decoded outputs allowed per frame.
        limit: usize,
        /// Number of decoded outputs in the rejected result.
        attempted: usize,
    },
    /// The package proposed too many replies for one frame.
    ReplyCountLimitExceeded {
        /// Maximum reply proposals allowed per frame.
        limit: usize,
        /// Number of replies in the rejected result.
        attempted: usize,
    },
    /// The combined reply payload exceeded the per-frame byte limit.
    ReplyBytesLimitExceeded {
        /// Maximum combined reply bytes allowed per frame.
        limit: usize,
        /// Combined bytes in the rejected result.
        attempted: usize,
    },
    /// One reply exceeded the host's per-reply byte limit.
    Reply {
        /// Zero-based position of the rejected reply.
        index: usize,
        /// Specific per-reply validation failure.
        source: ReplyStagingError,
    },
    /// The proposed replacement exceeded this session's state-byte limit.
    SessionStateLimitExceeded {
        /// Maximum state bytes retained for the session.
        limit: usize,
        /// Bytes in the rejected replacement.
        attempted: usize,
    },
}

impl fmt::Display for FrameProcessingValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DecodedOutputLimitExceeded { limit, attempted } => write!(
                formatter,
                "protocol package emitted {attempted} decoded outputs, exceeding the {limit}-output frame limit"
            ),
            Self::ReplyCountLimitExceeded { limit, attempted } => write!(
                formatter,
                "protocol package proposed {attempted} replies, exceeding the {limit}-reply frame limit"
            ),
            Self::ReplyBytesLimitExceeded { limit, attempted } => write!(
                formatter,
                "protocol package proposed {attempted} total reply bytes, exceeding the {limit}-byte frame limit"
            ),
            Self::Reply { index, source } => {
                write!(
                    formatter,
                    "protocol package reply at index {index} was rejected: {source}"
                )
            }
            Self::SessionStateLimitExceeded { limit, attempted } => write!(
                formatter,
                "protocol package proposed {attempted} state bytes, exceeding the {limit}-byte session limit"
            ),
        }
    }
}

impl Error for FrameProcessingValidationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Reply { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
