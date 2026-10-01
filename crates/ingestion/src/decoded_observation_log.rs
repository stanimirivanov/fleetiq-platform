use std::{error::Error, fmt, future::Future};

use fleetiq_telemetry::{AcceptedMessage, DecodedObservation};

/// One complete, ordered decoding result awaiting durable persistence.
///
/// The accepted message identifies the raw input and immutable protocol-package
/// release. Observations must carry that same trusted provenance and occupy
/// every zero-based output position exactly once. An empty collection is a
/// meaningful result: it records that decoding completed successfully without
/// producing telemetry.
#[derive(Clone, Eq, PartialEq)]
pub struct DecodedObservationBatch {
    message: AcceptedMessage,
    observations: Vec<DecodedObservation>,
}

impl DecodedObservationBatch {
    /// Validates and constructs one complete decoding result.
    ///
    /// # Errors
    ///
    /// Returns [`DecodedObservationBatchError`] at the first output whose
    /// position or trusted message provenance differs from `message`.
    pub fn new(
        message: AcceptedMessage,
        observations: Vec<DecodedObservation>,
    ) -> Result<Self, DecodedObservationBatchError> {
        Self::validate(&message, &observations)?;

        Ok(Self {
            message,
            observations,
        })
    }

    /// Validates trusted provenance and output order without taking ownership.
    ///
    /// This supports transitions that must retain an earlier pipeline value if
    /// an internal invariant is violated before constructing the batch.
    ///
    /// # Errors
    ///
    /// Returns [`DecodedObservationBatchError`] at the first output whose
    /// position or trusted message provenance differs from `message`.
    pub(crate) fn validate(
        message: &AcceptedMessage,
        observations: &[DecodedObservation],
    ) -> Result<(), DecodedObservationBatchError> {
        for (index, observation) in observations.iter().enumerate() {
            let expected_output_index = u32::try_from(index)
                .map_err(|_| DecodedObservationBatchError::OutputCountExceedsIdentityRange)?;

            if observation.id().output_index() != expected_output_index {
                return Err(DecodedObservationBatchError::OutputIndexMismatch {
                    index,
                    actual: observation.id().output_index(),
                });
            }
            if observation.tenant_id() != message.tenant_id() {
                return Err(DecodedObservationBatchError::TenantMismatch { index });
            }
            if observation.source().device_asset_id() != message.device_asset_id() {
                return Err(DecodedObservationBatchError::DeviceMismatch { index });
            }
            if observation.id().message_id() != message.id() {
                return Err(DecodedObservationBatchError::MessageMismatch { index });
            }
            if observation.decoded_by() != message.protocol_package() {
                return Err(DecodedObservationBatchError::ProtocolPackageMismatch { index });
            }
            if observation.received_at() != message.received_at() {
                return Err(DecodedObservationBatchError::ReceivedAtMismatch { index });
            }
        }

        Ok(())
    }

    // Only application transitions that have just validated these borrowed
    // parts may use this constructor. Moving ownership must not repeat copying
    // the wire payload or the complete observation collection.
    pub(crate) fn from_validated_parts(
        message: AcceptedMessage,
        observations: Vec<DecodedObservation>,
    ) -> Self {
        Self {
            message,
            observations,
        }
    }

    /// Returns the durably accepted raw message decoded by this batch.
    pub const fn message(&self) -> &AcceptedMessage {
        &self.message
    }

    /// Returns decoded observations in deterministic package-output order.
    pub fn observations(&self) -> &[DecodedObservation] {
        &self.observations
    }

    /// Returns the number of decoded observations.
    pub const fn len(&self) -> usize {
        self.observations.len()
    }

    /// Reports whether decoding completed without producing observations.
    pub const fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }

    /// Consumes the batch into its accepted message and ordered observations.
    pub fn into_parts(self) -> (AcceptedMessage, Vec<DecodedObservation>) {
        (self.message, self.observations)
    }
}

impl fmt::Debug for DecodedObservationBatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DecodedObservationBatch")
            .field("message", &self.message)
            .field("observation_count", &self.observations.len())
            .finish_non_exhaustive()
    }
}

/// Explains why observations cannot form one complete decoding result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodedObservationBatchError {
    /// The collection is too large for the `u32` output-index identity.
    OutputCountExceedsIdentityRange,
    /// An observation does not occupy its collection position.
    OutputIndexMismatch {
        /// Zero-based collection position where validation failed.
        index: usize,
        /// Output index carried by the observation identity.
        actual: u32,
    },
    /// An observation belongs to another tenant.
    TenantMismatch {
        /// Zero-based collection position where validation failed.
        index: usize,
    },
    /// An observation names another communication device.
    DeviceMismatch {
        /// Zero-based collection position where validation failed.
        index: usize,
    },
    /// An observation derives from another accepted message.
    MessageMismatch {
        /// Zero-based collection position where validation failed.
        index: usize,
    },
    /// An observation derives from another protocol-package release.
    ProtocolPackageMismatch {
        /// Zero-based collection position where validation failed.
        index: usize,
    },
    /// An observation carries a different host receipt time.
    ReceivedAtMismatch {
        /// Zero-based collection position where validation failed.
        index: usize,
    },
}

impl fmt::Display for DecodedObservationBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputCountExceedsIdentityRange => {
                formatter.write_str("decoded output count exceeds the identity range")
            }
            Self::OutputIndexMismatch { index, actual } => write!(
                formatter,
                "decoded observation at position {index} carries output index {actual}"
            ),
            Self::TenantMismatch { index } => {
                write!(
                    formatter,
                    "decoded observation at position {index} has another tenant"
                )
            }
            Self::DeviceMismatch { index } => write!(
                formatter,
                "decoded observation at position {index} has another communication device"
            ),
            Self::MessageMismatch { index } => write!(
                formatter,
                "decoded observation at position {index} has another accepted message"
            ),
            Self::ProtocolPackageMismatch { index } => write!(
                formatter,
                "decoded observation at position {index} has another protocol package"
            ),
            Self::ReceivedAtMismatch { index } => write!(
                formatter,
                "decoded observation at position {index} has another receipt time"
            ),
        }
    }
}

impl Error for DecodedObservationBatchError {}

/// Durable result of appending one complete decoding result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodedObservationAppendOutcome {
    /// This call durably appended the result for the first time.
    Appended,
    /// The identical result was already durable.
    AlreadyPresent,
}

impl DecodedObservationAppendOutcome {
    /// Returns whether this call created the durable result.
    pub const fn was_appended(self) -> bool {
        matches!(self, Self::Appended)
    }
}

/// Append-only durability boundary for complete decoded-message results.
///
/// Implementations persist the result marker and all ordered observations as
/// one atomic operation. `Ok` may be returned only after the configured
/// durability condition succeeds. The batch identity is its tenant, accepted
/// message ID, and immutable protocol-package release. An exact retry must
/// return [`DecodedObservationAppendOutcome::AlreadyPresent`]; reusing that
/// identity with a different observation count, order, or immutable content is
/// a collision and must fail.
///
/// A successful empty batch remains durable evidence that decoding completed.
/// An error exposes no partial success and can be ambiguous about durability.
/// The caller retains the borrowed batch and may retry it unchanged.
pub trait DecodedObservationLog: Send + Sync {
    /// The infrastructure-specific append or identity-collision failure.
    type Error: Error + Send + Sync + 'static;

    /// Durably appends one complete decoded-message result.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when storage fails, durability cannot
    /// be confirmed, or the batch identity already names different immutable
    /// content. Implementations must not report a partially appended batch.
    fn append<'a>(
        &'a self,
        batch: &'a DecodedObservationBatch,
    ) -> impl Future<Output = Result<DecodedObservationAppendOutcome, Self::Error>> + Send + 'a;
}

#[cfg(test)]
mod tests;
