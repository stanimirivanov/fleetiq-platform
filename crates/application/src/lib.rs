//! Application ports and use-case orchestration for `FleetIQ`.
//!
//! Ports are owned by their consumers. Infrastructure crates implement these
//! contracts without exposing database or transport types to domain code.

mod accepted_message_log;
mod definition_repository;
mod device_repository;
mod live;
mod relationship_repository;
mod repository;
mod signal_binding_repository;

pub use accepted_message_log::{
    AcceptedMessageAppendOutcome, AcceptedMessageLog, PendingAcceptedMessage,
};
pub use definition_repository::DefinitionRepository;
pub use device_repository::DeviceRepository;
pub use live::{
    LiveMessage, ProjectionDelta, ProjectionDeltaError, ProjectionRevision, ProjectionSnapshot,
    ResnapshotReason, ResnapshotRequired, RevisionCursor, RevisionCursorError,
    RevisionOverflowError,
};
pub use relationship_repository::{RelationshipRepository, RelationshipSnapshotQuery};
pub use repository::AssetRepository;
pub use signal_binding_repository::{SignalBindingRepository, SignalBindingSnapshotQuery};
