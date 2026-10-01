use std::fmt;

use fleetiq_protocol_host::DeferredReply;
use fleetiq_protocol_sdk::{DecodedOutput, assemble_observations};

use super::result::{DurablyAcceptedFrame, FrameAcceptanceError};
use crate::{AcceptedMessageLog, PendingAcceptedMessage};

/// Processed frame effects withheld until accepted-message durability succeeds.
#[derive(Clone, Eq, PartialEq)]
pub struct PendingFrameAcceptance {
    candidate: PendingAcceptedMessage,
    decoded_outputs: Vec<DecodedOutput>,
    deferred_replies: Vec<DeferredReply>,
}

impl PendingFrameAcceptance {
    pub(super) fn new(
        candidate: PendingAcceptedMessage,
        decoded_outputs: Vec<DecodedOutput>,
        deferred_replies: Vec<DeferredReply>,
    ) -> Self {
        Self {
            candidate,
            decoded_outputs,
            deferred_replies,
        }
    }

    /// Returns the complete message candidate supplied to the durable log.
    pub const fn candidate(&self) -> &PendingAcceptedMessage {
        &self.candidate
    }

    /// Returns the number of decoded outputs withheld before durability.
    pub fn decoded_output_count(&self) -> usize {
        self.decoded_outputs.len()
    }

    /// Returns the number of replies withheld before durability.
    pub fn deferred_reply_count(&self) -> usize {
        self.deferred_replies.len()
    }

    /// Durably appends the frame, assembles observations, and releases deferred replies.
    ///
    /// An append failure returns ownership of this pending value for an exact
    /// retry. A successful append is checked against the submitted candidate so
    /// a faulty log implementation cannot attach outputs or replies to unrelated
    /// acceptance evidence.
    ///
    /// # Errors
    ///
    /// Returns [`FrameAcceptanceError::Append`] when the durable log fails,
    /// [`FrameAcceptanceError::LogContractViolation`] when it returns a different
    /// accepted message, or [`FrameAcceptanceError::ObservationAssembly`] when
    /// output positions cannot be represented by observation identity.
    pub async fn accept<L>(
        self,
        log: &L,
    ) -> Result<DurablyAcceptedFrame, FrameAcceptanceError<L::Error>>
    where
        L: AcceptedMessageLog,
    {
        let append_outcome = match log.append(&self.candidate).await {
            Ok(outcome) => outcome,
            Err(source) => {
                return Err(FrameAcceptanceError::Append {
                    source,
                    pending: Box::new(self),
                });
            }
        };

        if !self.candidate.has_same_content_as(append_outcome.message()) {
            return Err(FrameAcceptanceError::LogContractViolation {
                pending: Box::new(self),
                returned: Box::new(append_outcome),
            });
        }

        let observations = match assemble_observations(
            append_outcome.message(),
            self.decoded_outputs.iter().cloned(),
        ) {
            Ok(observations) => observations,
            Err(source) => {
                return Err(FrameAcceptanceError::ObservationAssembly {
                    source,
                    append_outcome: Box::new(append_outcome),
                    decoded_outputs: self.decoded_outputs.into_boxed_slice(),
                    deferred_replies: self.deferred_replies.into_boxed_slice(),
                });
            }
        };
        let released_replies = self
            .deferred_replies
            .into_iter()
            .map(|reply| reply.release(append_outcome.message()))
            .collect();

        Ok(DurablyAcceptedFrame::new(
            append_outcome,
            observations,
            released_replies,
        ))
    }
}

impl fmt::Debug for PendingFrameAcceptance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingFrameAcceptance")
            .field("candidate", &self.candidate)
            .field("decoded_output_count", &self.decoded_outputs.len())
            .field("deferred_reply_count", &self.deferred_replies.len())
            .finish_non_exhaustive()
    }
}
