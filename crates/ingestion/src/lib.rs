//! Durable ingestion use cases and consumer-owned persistence ports for `FleetIQ`.
//!
//! Acceptance and decoded-result persistence are separate idempotent stages.
//! The trusted host retains pending work when a durable append fails and releases
//! replies only after their required durability boundary succeeds.

mod accepted_message_log;
mod decoded_observation_log;
mod frame_acceptance;
mod observation_persistence;
mod protocol_artifacts;
mod protocol_package_catalog;
pub use protocol_artifacts::{
    MAX_PROTOCOL_ARTIFACT_BYTES, PROTOCOL_ADMISSION_POLICY, ProtocolAdmission,
    ProtocolAdmissionWorld, ProtocolArtifact, ProtocolArtifactError, ProtocolArtifactStore,
    ProtocolCompiler, ProtocolCompilerError, ProtocolPackageService, ProtocolPackageServiceError,
};

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

mod protocol_routing;
pub use protocol_routing::{
    InvalidProtocolChannel, PinnedProtocolSession, ProtocolChannelId, ProtocolDeviceRoute,
    ProtocolRouteCommand, ProtocolRouteSelection, ProtocolRouteWrite, ProtocolRoutingRepository,
    ProtocolRoutingService, ProtocolSessionRoute, RegisteredIdentityChannel,
};

mod replay;
pub use replay::{
    InvalidReplay, REPLAY_FRAME_LIMIT, REPLAY_OUTPUT_LIMIT, REPLAY_POLICY, REPLAY_STATE_LIMIT,
    ReplayEffects, ReplayExecutor, ReplayFailure, ReplayLease, ReplayRepository, ReplayTick,
    ReplayWork, ReplayWorker,
};
