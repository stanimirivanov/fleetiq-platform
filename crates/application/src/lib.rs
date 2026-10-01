//! Application ports and use-case orchestration for `FleetIQ`.
//!
//! Ports are owned by their consumers. Infrastructure crates implement these
//! contracts without exposing database or transport types to domain code.

mod accepted_message_log;
mod asset_service;
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

#[cfg(test)]
mod test_support;

pub use accepted_message_log::{
    AcceptedMessageAppendOutcome, AcceptedMessageLog, PendingAcceptedMessage,
};
pub use asset_service::{AssetApplicationError, AssetApplicationService};
pub use decoded_observation_log::{
    DecodedObservationAppendOutcome, DecodedObservationBatch, DecodedObservationBatchError,
    DecodedObservationLog,
};
pub use definition_repository::DefinitionRepository;
pub use definition_service::{DefinitionApplicationError, DefinitionApplicationService};
pub use device_repository::DeviceRepository;
pub use device_service::{DeviceApplicationError, DeviceApplicationService};
pub use fleetiq_access::{
    AccessDecision, ActorIdentityResolver, AuthenticatedActor, AuthorizedPlatform,
    AuthorizedTenant, CallerAuthenticationError, CallerAuthenticationService,
    ExternalIdentityAuthenticator, ManageAccess, ManageIdentity, ManageMetadata,
    ManagePlatformAccess, ManageTenantLifecycle, PlatformAccessPolicy, PlatformAccessRequest,
    PlatformAction, PlatformActionMarker, PlatformAuthorizationError, PlatformAuthorizationService,
    ProvisionTenant, ReadMetadata, TenantAccessPolicy, TenantAccessRequest, TenantAction,
    TenantActionMarker, TenantAuthorizationError, TenantAuthorizationService,
};
pub use fleetiq_identity::{
    ActorCatalog, ActorCatalogCursor, ActorCatalogEntry, ActorCatalogError, ActorCatalogPage,
    ActorCatalogPageSize, ActorCatalogPageSizeError, ActorCatalogQuery, ActorCatalogRepository,
    ActorLifecycleState,
};
pub use fleetiq_identity::{
    ActorLifecycleAdministration, ActorLifecycleAdministrationError,
    ActorLifecycleAdministrationRepository, ActorLifecycleChange, ActorLifecycleChangeEffect,
    ActorLifecycleChangeKind, ActorLifecycleChangeOutcome, ActorLifecycleChangeRequest,
};
pub use fleetiq_identity::{
    ActorLifecycleAudit, ActorLifecycleAuditCursor, ActorLifecycleAuditEntry,
    ActorLifecycleAuditError, ActorLifecycleAuditPage, ActorLifecycleAuditPageSize,
    ActorLifecycleAuditPageSizeError, ActorLifecycleAuditQuery, ActorLifecycleAuditRepository,
};
pub use fleetiq_identity::{
    ActorProvisioningCommand, ActorProvisioningError, ActorProvisioningOutcome,
    ActorProvisioningRepository, ActorProvisioningRequest, ActorProvisioningService,
};
pub use fleetiq_identity::{
    ExternalIdentityAdministration, ExternalIdentityAdministrationError,
    ExternalIdentityAdministrationRepository, ExternalIdentityChange, ExternalIdentityChangeEffect,
    ExternalIdentityChangeKind, ExternalIdentityChangeOutcome, ExternalIdentityChangeRequest,
};
pub use fleetiq_identity::{
    ExternalIdentityAudit, ExternalIdentityAuditCursor, ExternalIdentityAuditEntry,
    ExternalIdentityAuditError, ExternalIdentityAuditPage, ExternalIdentityAuditPageSize,
    ExternalIdentityAuditPageSizeError, ExternalIdentityAuditQuery,
    ExternalIdentityAuditRepository,
};
pub use fleetiq_identity::{
    ExternalIdentityCatalog, ExternalIdentityCatalogCursor, ExternalIdentityCatalogEntry,
    ExternalIdentityCatalogError, ExternalIdentityCatalogPage, ExternalIdentityCatalogPageSize,
    ExternalIdentityCatalogPageSizeError, ExternalIdentityCatalogQuery,
    ExternalIdentityCatalogRepository,
};
pub use fleetiq_identity::{
    PlatformAccessAdministration, PlatformAccessAdministrationError,
    PlatformAccessAdministrationRepository, PlatformAccessChange, PlatformAccessChangeEffect,
    PlatformAccessChangeKind, PlatformAccessChangeOutcome, PlatformAccessChangeRequest,
};
pub use fleetiq_identity::{
    PlatformAccessAudit, PlatformAccessAuditCursor, PlatformAccessAuditEntry,
    PlatformAccessAuditError, PlatformAccessAuditPage, PlatformAccessAuditPageSize,
    PlatformAccessAuditPageSizeError, PlatformAccessAuditQuery, PlatformAccessAuditRepository,
};
pub use fleetiq_identity::{
    PlatformBootstrapError, PlatformBootstrapOutcome, PlatformBootstrapRepository,
    PlatformBootstrapRequest, PlatformBootstrapService,
};
pub use fleetiq_identity::{
    PlatformGrantCatalog, PlatformGrantCatalogCursor, PlatformGrantCatalogEntry,
    PlatformGrantCatalogError, PlatformGrantCatalogPage, PlatformGrantCatalogPageSize,
    PlatformGrantCatalogPageSizeError, PlatformGrantCatalogQuery, PlatformGrantCatalogRepository,
};
pub use fleetiq_identity::{
    TenantAccessAdministration, TenantAccessAdministrationError,
    TenantAccessAdministrationRepository, TenantAccessChange, TenantAccessChangeEffect,
    TenantAccessChangeKind, TenantAccessChangeOutcome, TenantAccessChangeRequest,
};
pub use fleetiq_identity::{
    TenantAccessAudit, TenantAccessAuditCursor, TenantAccessAuditEntry, TenantAccessAuditError,
    TenantAccessAuditPage, TenantAccessAuditPageSize, TenantAccessAuditPageSizeError,
    TenantAccessAuditQuery, TenantAccessAuditRepository,
};
pub use fleetiq_identity::{
    TenantCatalog, TenantCatalogCursor, TenantCatalogEntry, TenantCatalogError, TenantCatalogPage,
    TenantCatalogPageSize, TenantCatalogPageSizeError, TenantCatalogQuery, TenantCatalogRepository,
    TenantLifecycleState,
};
pub use fleetiq_identity::{
    TenantLifecycleAdministration, TenantLifecycleAdministrationError,
    TenantLifecycleAdministrationRepository, TenantLifecycleChange, TenantLifecycleChangeEffect,
    TenantLifecycleChangeKind, TenantLifecycleChangeOutcome, TenantLifecycleChangeRequest,
};
pub use fleetiq_identity::{
    TenantLifecycleAudit, TenantLifecycleAuditCursor, TenantLifecycleAuditEntry,
    TenantLifecycleAuditError, TenantLifecycleAuditPage, TenantLifecycleAuditPageSize,
    TenantLifecycleAuditPageSizeError, TenantLifecycleAuditQuery, TenantLifecycleAuditRepository,
};
pub use fleetiq_identity::{
    TenantProvisioningCommand, TenantProvisioningError, TenantProvisioningOutcome,
    TenantProvisioningRepository, TenantProvisioningRequest, TenantProvisioningService,
};
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
