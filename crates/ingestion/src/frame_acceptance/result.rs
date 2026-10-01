use std::{error::Error, fmt};

use fleetiq_protocol_host::{DeferredReply, OutboundReply};
use fleetiq_protocol_sdk::{DecodedOutput, ObservationAssemblyError};
use fleetiq_telemetry::{AcceptedMessage, DecodedObservation};

use super::PendingFrameAcceptance;
use crate::AcceptedMessageAppendOutcome;

/// Accepted message, trusted observations, and newly released replies.
#[derive(Clone, Eq, PartialEq)]
pub struct DurablyAcceptedFrame {
    append_outcome: AcceptedMessageAppendOutcome,
    observations: Vec<DecodedObservation>,
    released_replies: Vec<OutboundReply>,
}

impl DurablyAcceptedFrame {
    pub(super) fn new(
        append_outcome: AcceptedMessageAppendOutcome,
        observations: Vec<DecodedObservation>,
        released_replies: Vec<OutboundReply>,
    ) -> Self {
        Self {
            append_outcome,
            observations,
            released_replies,
        }
    }

    /// Returns the durable append disposition and accepted message.
    pub const fn append_outcome(&self) -> &AcceptedMessageAppendOutcome {
        &self.append_outcome
    }

    /// Returns the durably accepted raw message.
    pub const fn message(&self) -> &AcceptedMessage {
        self.append_outcome.message()
    }

    /// Returns trusted observations in deterministic package-output order.
    pub fn observations(&self) -> &[DecodedObservation] {
        &self.observations
    }

    /// Returns replies released by this durability transition in package order.
    pub fn released_replies(&self) -> &[OutboundReply] {
        &self.released_replies
    }

    /// Consumes the result into its durable message, observations, and replies.
    pub fn into_parts(
        self,
    ) -> (
        AcceptedMessageAppendOutcome,
        Vec<DecodedObservation>,
        Vec<OutboundReply>,
    ) {
        (
            self.append_outcome,
            self.observations,
            self.released_replies,
        )
    }
}

impl fmt::Debug for DurablyAcceptedFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DurablyAcceptedFrame")
            .field("append_outcome", &self.append_outcome)
            .field("observation_count", &self.observations.len())
            .field("released_reply_count", &self.released_replies.len())
            .finish_non_exhaustive()
    }
}

/// Failure while moving processed frame effects through durable acceptance.
#[derive(Debug)]
pub enum FrameAcceptanceError<E> {
    /// Durable append failed; the complete pending value remains retryable.
    Append {
        /// Durable-log implementation failure.
        source: E,
        /// Exact candidate and gated effects retained for retry.
        pending: Box<PendingFrameAcceptance>,
    },
    /// The log returned acceptance evidence for different immutable content.
    LogContractViolation {
        /// Original candidate and gated effects that were not released.
        pending: Box<PendingFrameAcceptance>,
        /// Mismatched outcome returned by the log implementation.
        returned: Box<AcceptedMessageAppendOutcome>,
    },
    /// The message is durable, but decoded-output positions could not be assembled.
    ObservationAssembly {
        /// Deterministic observation-identity assembly failure.
        source: ObservationAssemblyError,
        /// Completed durable append and its accepted message.
        append_outcome: Box<AcceptedMessageAppendOutcome>,
        /// Original outputs retained for diagnosis or controlled recovery.
        decoded_outputs: Box<[DecodedOutput]>,
        /// Replies that remain opaque and unreleased.
        deferred_replies: Box<[DeferredReply]>,
    },
}

impl<E: fmt::Display> fmt::Display for FrameAcceptanceError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Append { source, .. } => {
                write!(formatter, "accepted-message append failed: {source}")
            }
            Self::LogContractViolation { .. } => {
                formatter.write_str("accepted-message log returned different immutable content")
            }
            Self::ObservationAssembly { source, .. } => {
                write!(formatter, "decoded-observation assembly failed: {source}")
            }
        }
    }
}

impl<E: Error + 'static> Error for FrameAcceptanceError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Append { source, .. } => Some(source),
            Self::ObservationAssembly { source, .. } => Some(source),
            Self::LogContractViolation { .. } => None,
        }
    }
}
