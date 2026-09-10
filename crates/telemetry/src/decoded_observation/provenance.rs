use crate::{AcceptedMessageId, ProtocolPackageId, ProtocolPackageVersion};

/// Identifies one decoder output deterministically within an accepted message.
///
/// A protocol package can emit several observations from one wire message. The
/// zero-based output index makes each identity stable when the same accepted
/// message is replayed through the same immutable package release.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DecodedObservationId {
    message_id: AcceptedMessageId,
    decoded_by: ProtocolPackageRef,
    output_index: u32,
}

impl DecodedObservationId {
    /// Constructs a stable decoded-observation identity.
    pub const fn new(
        message_id: AcceptedMessageId,
        decoded_by: ProtocolPackageRef,
        output_index: u32,
    ) -> Self {
        Self {
            message_id,
            decoded_by,
            output_index,
        }
    }

    /// Returns the durably accepted raw message that produced this output.
    pub const fn message_id(&self) -> &AcceptedMessageId {
        &self.message_id
    }

    /// Returns the exact package release that produced this interpretation.
    pub const fn decoded_by(&self) -> &ProtocolPackageRef {
        &self.decoded_by
    }

    /// Returns the zero-based position in the package's decoded output.
    pub const fn output_index(&self) -> u32 {
        self.output_index
    }
}

/// Pins decoder provenance to one immutable protocol package release.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProtocolPackageRef {
    id: ProtocolPackageId,
    version: ProtocolPackageVersion,
}

impl ProtocolPackageRef {
    /// Constructs an exact protocol package reference.
    pub const fn new(id: ProtocolPackageId, version: ProtocolPackageVersion) -> Self {
        Self { id, version }
    }

    /// Returns the stable protocol package identity.
    pub const fn id(&self) -> &ProtocolPackageId {
        &self.id
    }

    /// Returns the immutable protocol package release.
    pub const fn version(&self) -> &ProtocolPackageVersion {
        &self.version
    }
}
