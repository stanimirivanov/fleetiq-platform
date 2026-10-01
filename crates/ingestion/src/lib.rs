//! Durable ingestion use cases and consumer-owned persistence ports for `FleetIQ`.
//!
//! Acceptance and decoded-result persistence are separate idempotent stages.
//! The trusted host retains pending work when a durable append fails and releases
//! replies only after their required durability boundary succeeds.

mod accepted_message_log;
mod decoded_observation_log;
mod frame_acceptance;
mod observation_persistence;
mod protocol_package_catalog;

pub use accepted_message_log::{
    AcceptedMessageAppendOutcome, AcceptedMessageLog, PendingAcceptedMessage,
};
pub use decoded_observation_log::{
    DecodedObservationAppendOutcome, DecodedObservationBatch, DecodedObservationBatchError,
    DecodedObservationLog,
};
pub use frame_acceptance::{
    DurablyAcceptedFrame, FrameAcceptanceContext, FrameAcceptanceError, PendingFrameAcceptance,
    PreparedFrameAcceptance,
};
pub use observation_persistence::{
    DurablyPersistedFrame, ObservationPersistenceError, ObservationPersistencePreparationError,
    PendingObservationPersistence,
};
pub use protocol_package_catalog::{
    ProtocolPackageCatalog, ProtocolPackageRegistrationOutcome, ProtocolPackageRelease,
};
