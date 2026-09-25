//! Application ports and use-case orchestration for `FleetIQ`.
//!
//! Ports are owned by their consumers. Infrastructure crates implement these
//! contracts without exposing database or transport types to domain code.

mod accepted_message_log;
mod asset_service;
mod authorization;
mod decoded_observation_log;
mod definition_repository;
mod definition_service;
mod device_repository;
mod device_service;
mod frame_acceptance;
mod live;
mod observation_persistence;
mod protocol_package_catalog;
mod relationship_repository;
mod relationship_service;
mod repository;
mod signal_binding_repository;
mod signal_binding_service;
mod tenant_access_administration;

pub use accepted_message_log::{
    AcceptedMessageAppendOutcome, AcceptedMessageLog, PendingAcceptedMessage,
};
pub use asset_service::{AssetApplicationError, AssetApplicationService};
pub use authorization::{
    AccessDecision, AuthorizedTenantAction, TenantAccessPolicy, TenantAccessRequest, TenantAction,
    TenantAuthorizationError, TenantAuthorizationService,
};
pub use decoded_observation_log::{
    DecodedObservationAppendOutcome, DecodedObservationBatch, DecodedObservationBatchError,
    DecodedObservationLog,
};
pub use definition_repository::DefinitionRepository;
pub use definition_service::{DefinitionApplicationError, DefinitionApplicationService};
pub use device_repository::DeviceRepository;
pub use device_service::{DeviceApplicationError, DeviceApplicationService};
pub use frame_acceptance::{
    DurablyAcceptedFrame, FrameAcceptanceContext, FrameAcceptanceError, PendingFrameAcceptance,
    PreparedFrameAcceptance,
};
pub use live::{
    LiveMessage, ProjectionDelta, ProjectionDeltaError, ProjectionRevision, ProjectionSnapshot,
    ResnapshotReason, ResnapshotRequired, RevisionCursor, RevisionCursorError,
    RevisionOverflowError,
};
pub use observation_persistence::{
    DurablyPersistedFrame, ObservationPersistenceError, ObservationPersistencePreparationError,
    PendingObservationPersistence,
};
pub use protocol_package_catalog::{
    ProtocolPackageCatalog, ProtocolPackageRegistrationOutcome, ProtocolPackageRelease,
};
pub use relationship_repository::{RelationshipRepository, RelationshipSnapshotQuery};
pub use relationship_service::{RelationshipApplicationError, RelationshipApplicationService};
pub use repository::AssetRepository;
pub use signal_binding_repository::{SignalBindingRepository, SignalBindingSnapshotQuery};
pub use signal_binding_service::{SignalBindingApplicationError, SignalBindingApplicationService};
pub use tenant_access_administration::{
    TenantAccessAdministration, TenantAccessAdministrationError,
    TenantAccessAdministrationRepository, TenantAccessChange, TenantAccessChangeEffect,
    TenantAccessChangeKind, TenantAccessChangeOutcome, TenantAccessChangeRequest,
};
