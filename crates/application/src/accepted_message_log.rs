use std::{error::Error, fmt, future::Future};

use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_telemetry::{AcceptedMessage, AcceptedMessageId, AcceptedPayload, ProtocolPackageRef};

/// A complete framed message awaiting the ingestion durability boundary.
///
/// The host assigns identity, admission context, package provenance, receipt
/// time, and exact wire bytes before append. This value is not yet an
/// [`AcceptedMessage`]: application code must not assemble observations or
/// release durability-gated replies until [`AcceptedMessageLog::append`]
/// succeeds.
#[derive(Clone, Eq, PartialEq)]
pub struct PendingAcceptedMessage {
    id: AcceptedMessageId,
    tenant_id: TenantId,
    device_asset_id: AssetId,
    protocol_package: ProtocolPackageRef,
    received_at: UnixTimeMillis,
    payload: AcceptedPayload,
}

impl PendingAcceptedMessage {
    /// Creates a complete append candidate from trusted session context.
    pub const fn new(
        id: AcceptedMessageId,
        tenant_id: TenantId,
        device_asset_id: AssetId,
        protocol_package: ProtocolPackageRef,
        received_at: UnixTimeMillis,
        payload: AcceptedPayload,
    ) -> Self {
        Self {
            id,
            tenant_id,
            device_asset_id,
            protocol_package,
            received_at,
            payload,
        }
    }

    /// Returns the stable identity used for append retries and deduplication.
    pub const fn id(&self) -> &AcceptedMessageId {
        &self.id
    }

    /// Returns the tenant established by device admission.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the admitted communication-device asset.
    pub const fn device_asset_id(&self) -> &AssetId {
        &self.device_asset_id
    }

    /// Returns the immutable package release pinned to the session.
    pub const fn protocol_package(&self) -> &ProtocolPackageRef {
        &self.protocol_package
    }

    /// Returns when the host received the complete frame.
    pub const fn received_at(&self) -> UnixTimeMillis {
        self.received_at
    }

    /// Returns the exact opaque wire payload awaiting durable acceptance.
    pub const fn payload(&self) -> &AcceptedPayload {
        &self.payload
    }

    /// Constructs the accepted value after an implementation establishes its
    /// documented durability condition.
    pub fn into_accepted_message(self) -> AcceptedMessage {
        AcceptedMessage::new(
            self.id,
            self.tenant_id,
            self.device_asset_id,
            self.protocol_package,
            self.received_at,
            self.payload,
        )
    }
}

impl fmt::Debug for PendingAcceptedMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingAcceptedMessage")
            .field("id", &self.id)
            .field("tenant_id", &self.tenant_id)
            .field("device_asset_id", &self.device_asset_id)
            .field("protocol_package", &self.protocol_package)
            .field("received_at", &self.received_at)
            .field("payload", &self.payload)
            .finish()
    }
}

/// Durable result of appending one accepted protocol message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcceptedMessageAppendOutcome {
    /// This call durably appended the message for the first time.
    Appended(AcceptedMessage),
    /// An identical message with this stable identity was already durable.
    AlreadyPresent(AcceptedMessage),
}

impl AcceptedMessageAppendOutcome {
    /// Returns the durably accepted message for either idempotent outcome.
    pub const fn message(&self) -> &AcceptedMessage {
        match self {
            Self::Appended(message) | Self::AlreadyPresent(message) => message,
        }
    }

    /// Returns whether this call created the durable record.
    pub const fn was_appended(&self) -> bool {
        matches!(self, Self::Appended(_))
    }

    /// Consumes the outcome and returns the durably accepted message.
    pub fn into_message(self) -> AcceptedMessage {
        match self {
            Self::Appended(message) | Self::AlreadyPresent(message) => message,
        }
    }
}

/// Append-only durability boundary for raw protocol messages.
///
/// Implementations own the concrete storage transaction, acknowledgement, and
/// retry mechanism. `Ok` may be returned only after the configured durability
/// condition has succeeded. Appending the same candidate again must be
/// idempotent and return [`AcceptedMessageAppendOutcome::AlreadyPresent`]. The
/// same identity with different immutable content is a collision and must fail.
///
/// An error may be ambiguous about whether durability was reached. Callers
/// retain the borrowed candidate and retry with the same identity and content;
/// implementations use the idempotency rule to resolve that ambiguity.
pub trait AcceptedMessageLog: Send + Sync {
    /// The infrastructure-specific append or identity-collision failure.
    type Error: Error + Send + Sync + 'static;

    /// Durably appends one complete raw protocol message.
    ///
    /// # Errors
    ///
    /// Returns the implementation error when storage fails, durability cannot
    /// be confirmed, or the message identity already names different immutable
    /// content. An error never authorizes observation assembly or release of a
    /// durability-gated reply.
    fn append<'a>(
        &'a self,
        candidate: &'a PendingAcceptedMessage,
    ) -> impl Future<Output = Result<AcceptedMessageAppendOutcome, Self::Error>> + Send + 'a;
}

#[cfg(test)]
mod tests {
    use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageVersion};

    use super::*;

    #[test]
    fn pending_message_preserves_context_without_exposing_payload_in_debug() {
        let pending = pending_message();

        assert_eq!(pending.id().as_str(), "message-1");
        assert_eq!(pending.tenant_id().as_str(), "tenant-a");
        assert_eq!(pending.device_asset_id().as_str(), "device-a");
        assert_eq!(pending.protocol_package().id().as_str(), "vendor.protocol");
        assert_eq!(pending.received_at(), UnixTimeMillis::new(1_000));
        assert_eq!(pending.payload().as_bytes(), b"private-wire-frame");
        assert!(!format!("{pending:?}").contains("private-wire-frame"));
    }

    #[test]
    fn append_outcome_retains_idempotency_disposition_and_message() {
        let appended =
            AcceptedMessageAppendOutcome::Appended(pending_message().into_accepted_message());
        let existing =
            AcceptedMessageAppendOutcome::AlreadyPresent(pending_message().into_accepted_message());

        assert!(appended.was_appended());
        assert!(!existing.was_appended());
        assert_eq!(appended.message().id().as_str(), "message-1");
        assert_eq!(existing.into_message().id().as_str(), "message-1");
    }

    fn pending_message() -> PendingAcceptedMessage {
        PendingAcceptedMessage::new(
            AcceptedMessageId::new("message-1").unwrap(),
            TenantId::new("tenant-a").unwrap(),
            AssetId::new("device-a").unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("vendor.protocol").unwrap(),
                ProtocolPackageVersion::new("2026.09.1").unwrap(),
            ),
            UnixTimeMillis::new(1_000),
            AcceptedPayload::new(b"private-wire-frame".as_slice()),
        )
    }
}
