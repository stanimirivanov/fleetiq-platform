use std::{error::Error, fmt};

use fleetiq_protocol_host::OutboundReply;
use fleetiq_telemetry::{AcceptedMessage, DecodedObservation};

use crate::{
    DecodedObservationAppendOutcome, DecodedObservationBatch, DecodedObservationBatchError,
    DecodedObservationLog, DurablyAcceptedFrame,
};

/// Complete accepted-frame effects awaiting decoded-result durability.
#[derive(Clone, Eq, PartialEq)]
pub struct PendingObservationPersistence {
    accepted_message_was_appended: bool,
    batch: DecodedObservationBatch,
    released_replies: Vec<OutboundReply>,
}

impl PendingObservationPersistence {
    /// Returns whether raw-message acceptance was a first append.
    pub const fn accepted_message_was_appended(&self) -> bool {
        self.accepted_message_was_appended
    }

    /// Returns the already durably accepted raw message.
    pub const fn message(&self) -> &AcceptedMessage {
        self.batch.message()
    }

    /// Returns the number of observations withheld until batch durability.
    pub const fn observation_count(&self) -> usize {
        self.batch.len()
    }

    /// Returns replies already eligible after accepted-message durability.
    ///
    /// Observation persistence does not change reply eligibility. Callers may
    /// pass these replies to transport while the batch append is pending.
    pub fn released_replies(&self) -> &[OutboundReply] {
        &self.released_replies
    }

    /// Durably appends the complete observation batch.
    ///
    /// # Errors
    ///
    /// Returns [`ObservationPersistenceError::Append`] with ownership of this
    /// exact pending value when the log cannot confirm durability. The caller
    /// can retry without reconstructing the batch or losing released replies.
    pub async fn persist<L>(
        self,
        log: &L,
    ) -> Result<DurablyPersistedFrame, ObservationPersistenceError<L::Error>>
    where
        L: DecodedObservationLog,
    {
        match log.append(&self.batch).await {
            Ok(observation_append_outcome) => Ok(DurablyPersistedFrame {
                accepted_message_was_appended: self.accepted_message_was_appended,
                observation_append_outcome,
                batch: self.batch,
                released_replies: self.released_replies,
            }),
            Err(source) => Err(ObservationPersistenceError::Append {
                source,
                pending: Box::new(self),
            }),
        }
    }
}

impl fmt::Debug for PendingObservationPersistence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingObservationPersistence")
            .field(
                "accepted_message_was_appended",
                &self.accepted_message_was_appended,
            )
            .field("message", &self.batch.message())
            .field("observation_count", &self.batch.len())
            .field("released_reply_count", &self.released_replies.len())
            .finish_non_exhaustive()
    }
}

/// Accepted frame whose complete decoded result is also durable.
#[derive(Clone, Eq, PartialEq)]
pub struct DurablyPersistedFrame {
    accepted_message_was_appended: bool,
    observation_append_outcome: DecodedObservationAppendOutcome,
    batch: DecodedObservationBatch,
    released_replies: Vec<OutboundReply>,
}

impl DurablyPersistedFrame {
    /// Returns whether raw-message acceptance was a first append.
    pub const fn accepted_message_was_appended(&self) -> bool {
        self.accepted_message_was_appended
    }

    /// Returns the decoded-result append disposition.
    pub const fn observation_append_outcome(&self) -> DecodedObservationAppendOutcome {
        self.observation_append_outcome
    }

    /// Returns the durably accepted raw message.
    pub const fn message(&self) -> &AcceptedMessage {
        self.batch.message()
    }

    /// Returns the complete durably persisted observation batch.
    pub const fn batch(&self) -> &DecodedObservationBatch {
        &self.batch
    }

    /// Returns durable observations in deterministic package-output order.
    pub fn observations(&self) -> &[DecodedObservation] {
        self.batch.observations()
    }

    /// Returns replies released by accepted-message durability.
    pub fn released_replies(&self) -> &[OutboundReply] {
        &self.released_replies
    }

    /// Consumes the result into its append dispositions, batch, and replies.
    pub fn into_parts(
        self,
    ) -> (
        bool,
        DecodedObservationAppendOutcome,
        DecodedObservationBatch,
        Vec<OutboundReply>,
    ) {
        (
            self.accepted_message_was_appended,
            self.observation_append_outcome,
            self.batch,
            self.released_replies,
        )
    }
}

impl fmt::Debug for DurablyPersistedFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DurablyPersistedFrame")
            .field(
                "accepted_message_was_appended",
                &self.accepted_message_was_appended,
            )
            .field(
                "observation_append_outcome",
                &self.observation_append_outcome,
            )
            .field("message", &self.batch.message())
            .field("observation_count", &self.batch.len())
            .field("released_reply_count", &self.released_replies.len())
            .finish_non_exhaustive()
    }
}

/// Internal invariant failure while preparing decoded-result persistence.
#[derive(Debug)]
pub struct ObservationPersistencePreparationError {
    source: DecodedObservationBatchError,
    frame: Box<DurablyAcceptedFrame>,
}

impl ObservationPersistencePreparationError {
    /// Returns the observation-batch invariant that failed.
    pub const fn batch_error(&self) -> DecodedObservationBatchError {
        self.source
    }

    /// Recovers the complete durably accepted frame for diagnosis.
    pub fn into_frame(self) -> DurablyAcceptedFrame {
        *self.frame
    }
}

impl fmt::Display for ObservationPersistencePreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "durably accepted frame cannot form an observation batch: {}",
            self.source
        )
    }
}

impl Error for ObservationPersistencePreparationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Failure while making an accepted frame's decoded result durable.
#[derive(Debug)]
pub enum ObservationPersistenceError<E> {
    /// Durable append failed; the complete pending value remains retryable.
    Append {
        /// Decoded-observation log implementation failure.
        source: E,
        /// Exact batch, append disposition, and released replies retained.
        pending: Box<PendingObservationPersistence>,
    },
}

impl<E: fmt::Display> fmt::Display for ObservationPersistenceError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Append { source, .. } => {
                write!(formatter, "decoded-observation append failed: {source}")
            }
        }
    }
}

impl<E: Error + 'static> Error for ObservationPersistenceError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Append { source, .. } => Some(source),
        }
    }
}

impl DurablyAcceptedFrame {
    /// Converts accepted-frame effects into one retryable persistence unit.
    ///
    /// Released replies remain eligible and accessible on the pending value;
    /// this transition does not strengthen their release condition.
    ///
    /// # Errors
    ///
    /// Returns [`ObservationPersistencePreparationError`] with the complete
    /// frame if its observations violate the batch invariant. Values produced
    /// by the supported acceptance flow satisfy this invariant by construction.
    pub fn prepare_observation_persistence(
        self,
    ) -> Result<PendingObservationPersistence, ObservationPersistencePreparationError> {
        if let Err(source) = DecodedObservationBatch::validate(self.message(), self.observations())
        {
            return Err(ObservationPersistencePreparationError {
                source,
                frame: Box::new(self),
            });
        }

        let (append_outcome, observations, released_replies) = self.into_parts();
        let accepted_message_was_appended = append_outcome.was_appended();
        let message = append_outcome.into_message();
        let batch = DecodedObservationBatch::from_validated_parts(message, observations);

        Ok(PendingObservationPersistence {
            accepted_message_was_appended,
            batch,
            released_replies,
        })
    }
}

#[cfg(test)]
mod tests;
