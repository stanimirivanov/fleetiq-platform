use std::fmt;

use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};

use crate::{AcceptedMessageId, DecodedObservationId, ProtocolPackageRef};

/// Immutable wire bytes retained after durable message acceptance.
///
/// The payload remains opaque to the host. A protocol package owns framing and
/// interpretation, including whether an empty frame is meaningful. Its custom
/// [`Debug`] representation reports only the byte count so routine diagnostics
/// do not print device payloads.
#[derive(Clone, Eq, PartialEq)]
pub struct AcceptedPayload(Box<[u8]>);

impl AcceptedPayload {
    /// Copies or moves bytes into an immutable accepted payload.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into().into_boxed_slice())
    }

    /// Returns the uninterpreted wire bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns the payload size in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Reports whether the accepted frame contains no bytes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Consumes the payload and returns its immutable bytes.
    pub fn into_bytes(self) -> Box<[u8]> {
        self.0
    }
}

impl fmt::Debug for AcceptedPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcceptedPayload")
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

/// One protocol frame exposed after the host's durability condition succeeds.
///
/// Device admission supplies tenant and communication-device identity before
/// decoding. The session pins an immutable protocol-package release, making a
/// later decoded observation reproducible. Source endpoints and signals are not
/// assigned here because one frame can produce observations from several
/// endpoint-scoped sources.
///
/// Construction represents the pipeline contract; the type cannot itself prove
/// that storage is durable. The ingestion host must construct or publish this
/// value only after its documented acceptance condition has succeeded.
#[derive(Clone, Eq, PartialEq)]
pub struct AcceptedMessage {
    id: AcceptedMessageId,
    tenant_id: TenantId,
    device_asset_id: AssetId,
    protocol_package: ProtocolPackageRef,
    received_at: UnixTimeMillis,
    payload: AcceptedPayload,
}

impl AcceptedMessage {
    /// Constructs a durably accepted protocol frame from host-owned context.
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

    /// Returns the identity retained across decoding retries.
    pub const fn id(&self) -> &AcceptedMessageId {
        &self.id
    }

    /// Returns the tenant established during device admission.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the admitted communication-device asset.
    pub const fn device_asset_id(&self) -> &AssetId {
        &self.device_asset_id
    }

    /// Returns the immutable package release pinned to the admitted session.
    pub const fn protocol_package(&self) -> &ProtocolPackageRef {
        &self.protocol_package
    }

    /// Returns when the host received the complete framed message.
    pub const fn received_at(&self) -> UnixTimeMillis {
        self.received_at
    }

    /// Returns the opaque wire payload supplied to the protocol package.
    pub const fn payload(&self) -> &AcceptedPayload {
        &self.payload
    }

    /// Derives a stable identity for one position in this message's output.
    ///
    /// Using the accepted message as the source prevents a decoder from pairing
    /// an output with a different message or protocol-package release.
    pub fn decoded_observation_id(&self, output_index: u32) -> DecodedObservationId {
        DecodedObservationId::new(self.id.clone(), self.protocol_package.clone(), output_index)
    }

    /// Consumes the message and returns its opaque wire payload.
    pub fn into_payload(self) -> AcceptedPayload {
        self.payload
    }
}

impl fmt::Debug for AcceptedMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcceptedMessage")
            .field("id", &self.id)
            .field("tenant_id", &self.tenant_id)
            .field("device_asset_id", &self.device_asset_id)
            .field("protocol_package", &self.protocol_package)
            .field("received_at", &self.received_at)
            .field("payload", &self.payload)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};

    use crate::{AcceptedMessageId, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};

    use super::*;

    #[test]
    fn preserves_admission_package_time_and_opaque_payload() {
        let message = AcceptedMessage::new(
            AcceptedMessageId::new("message-1").expect("valid message ID"),
            TenantId::new("tenant-a").expect("valid tenant ID"),
            AssetId::new("gateway-1").expect("valid device asset ID"),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("example.binary").expect("valid package ID"),
                ProtocolPackageVersion::new("1").expect("valid package version"),
            ),
            UnixTimeMillis::new(1_000),
            AcceptedPayload::new(b"private-wire-data".as_slice()),
        );

        assert_eq!(message.id().as_str(), "message-1");
        assert_eq!(message.tenant_id().as_str(), "tenant-a");
        assert_eq!(message.device_asset_id().as_str(), "gateway-1");
        assert_eq!(message.protocol_package().id().as_str(), "example.binary");
        assert_eq!(message.received_at(), UnixTimeMillis::new(1_000));
        assert_eq!(message.payload().as_bytes(), b"private-wire-data");

        let output_id = message.decoded_observation_id(3);
        assert_eq!(output_id.message_id(), message.id());
        assert_eq!(output_id.decoded_by(), message.protocol_package());
        assert_eq!(output_id.output_index(), 3);

        let diagnostics = format!("{message:?}");
        assert!(diagnostics.contains("len: 17"));
        assert!(!diagnostics.contains("private-wire-data"));
    }

    #[test]
    fn leaves_empty_frame_semantics_to_the_protocol_package() {
        let payload = AcceptedPayload::new(Vec::new());

        assert!(payload.is_empty());
        assert_eq!(payload.len(), 0);
    }
}
