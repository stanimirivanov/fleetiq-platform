//! Application ports and use-case orchestration for `FleetIQ`.
//!
//! Ports are owned by their consumers. Infrastructure crates implement these
//! contracts without exposing database or transport types to domain code.

mod accepted_message_log;
mod actor_catalog;
mod actor_lifecycle_administration;
mod actor_lifecycle_audit;
mod actor_provisioning;
mod asset_service;
mod decoded_observation_log;
mod definition_repository;
mod definition_service;
mod device_repository;
mod device_service;
mod external_identity_administration;
mod external_identity_audit;
mod external_identity_catalog;
mod frame_acceptance;
mod live;
mod observation_persistence;
mod platform_access_administration;
mod platform_access_audit;
mod platform_bootstrap;
mod platform_grant_catalog;
mod protocol_package_catalog;
mod relationship_repository;
mod relationship_service;
mod repository;
mod signal_binding_repository;
mod signal_binding_service;
mod tenant_access_administration;
mod tenant_access_audit;
mod tenant_catalog;
mod tenant_lifecycle_administration;
mod tenant_lifecycle_audit;
mod tenant_provisioning;

#[cfg(test)]
mod test_support;

pub use accepted_message_log::{
    AcceptedMessageAppendOutcome, AcceptedMessageLog, PendingAcceptedMessage,
};
pub use actor_catalog::{
    ActorCatalog, ActorCatalogCursor, ActorCatalogEntry, ActorCatalogError, ActorCatalogPage,
    ActorCatalogPageSize, ActorCatalogPageSizeError, ActorCatalogQuery, ActorCatalogRepository,
    ActorLifecycleState,
};
pub use actor_lifecycle_administration::{
    ActorLifecycleAdministration, ActorLifecycleAdministrationError,
    ActorLifecycleAdministrationRepository, ActorLifecycleChange, ActorLifecycleChangeEffect,
    ActorLifecycleChangeKind, ActorLifecycleChangeOutcome, ActorLifecycleChangeRequest,
};
pub use actor_lifecycle_audit::{
    ActorLifecycleAudit, ActorLifecycleAuditCursor, ActorLifecycleAuditEntry,
    ActorLifecycleAuditError, ActorLifecycleAuditPage, ActorLifecycleAuditPageSize,
    ActorLifecycleAuditPageSizeError, ActorLifecycleAuditQuery, ActorLifecycleAuditRepository,
};
pub use actor_provisioning::{
    ActorProvisioningCommand, ActorProvisioningError, ActorProvisioningOutcome,
    ActorProvisioningRepository, ActorProvisioningRequest, ActorProvisioningService,
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
pub use external_identity_administration::{
    ExternalIdentityAdministration, ExternalIdentityAdministrationError,
    ExternalIdentityAdministrationRepository, ExternalIdentityChange, ExternalIdentityChangeEffect,
    ExternalIdentityChangeKind, ExternalIdentityChangeOutcome, ExternalIdentityChangeRequest,
};
pub use external_identity_audit::{
    ExternalIdentityAudit, ExternalIdentityAuditCursor, ExternalIdentityAuditEntry,
    ExternalIdentityAuditError, ExternalIdentityAuditPage, ExternalIdentityAuditPageSize,
    ExternalIdentityAuditPageSizeError, ExternalIdentityAuditQuery,
    ExternalIdentityAuditRepository,
};
pub use external_identity_catalog::{
    ExternalIdentityCatalog, ExternalIdentityCatalogCursor, ExternalIdentityCatalogEntry,
    ExternalIdentityCatalogError, ExternalIdentityCatalogPage, ExternalIdentityCatalogPageSize,
    ExternalIdentityCatalogPageSizeError, ExternalIdentityCatalogQuery,
    ExternalIdentityCatalogRepository,
};
pub use fleetiq_access::{
    AccessDecision, ActorIdentityResolver, AuthenticatedActor, AuthorizedPlatformAction,
    AuthorizedTenant, CallerAuthenticationError, CallerAuthenticationService,
    ExternalIdentityAuthenticator, ManageAccess, ManageMetadata, PlatformAccessPolicy,
    PlatformAccessRequest, PlatformAction, PlatformAuthorizationError,
    PlatformAuthorizationService, ReadMetadata, TenantAccessPolicy, TenantAccessRequest,
    TenantAction, TenantActionMarker, TenantAuthorizationError, TenantAuthorizationService,
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
pub use platform_access_administration::{
    PlatformAccessAdministration, PlatformAccessAdministrationError,
    PlatformAccessAdministrationRepository, PlatformAccessChange, PlatformAccessChangeEffect,
    PlatformAccessChangeKind, PlatformAccessChangeOutcome, PlatformAccessChangeRequest,
};
pub use platform_access_audit::{
    PlatformAccessAudit, PlatformAccessAuditCursor, PlatformAccessAuditEntry,
    PlatformAccessAuditError, PlatformAccessAuditPage, PlatformAccessAuditPageSize,
    PlatformAccessAuditPageSizeError, PlatformAccessAuditQuery, PlatformAccessAuditRepository,
};
pub use platform_bootstrap::{
    PlatformBootstrapError, PlatformBootstrapOutcome, PlatformBootstrapRepository,
    PlatformBootstrapRequest, PlatformBootstrapService,
};
pub use platform_grant_catalog::{
    PlatformGrantCatalog, PlatformGrantCatalogCursor, PlatformGrantCatalogEntry,
    PlatformGrantCatalogError, PlatformGrantCatalogPage, PlatformGrantCatalogPageSize,
    PlatformGrantCatalogPageSizeError, PlatformGrantCatalogQuery, PlatformGrantCatalogRepository,
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
pub use tenant_access_audit::{
    TenantAccessAudit, TenantAccessAuditCursor, TenantAccessAuditEntry, TenantAccessAuditError,
    TenantAccessAuditPage, TenantAccessAuditPageSize, TenantAccessAuditPageSizeError,
    TenantAccessAuditQuery, TenantAccessAuditRepository,
};
pub use tenant_catalog::{
    TenantCatalog, TenantCatalogCursor, TenantCatalogEntry, TenantCatalogError, TenantCatalogPage,
    TenantCatalogPageSize, TenantCatalogPageSizeError, TenantCatalogQuery, TenantCatalogRepository,
    TenantLifecycleState,
};
pub use tenant_lifecycle_administration::{
    TenantLifecycleAdministration, TenantLifecycleAdministrationError,
    TenantLifecycleAdministrationRepository, TenantLifecycleChange, TenantLifecycleChangeEffect,
    TenantLifecycleChangeKind, TenantLifecycleChangeOutcome, TenantLifecycleChangeRequest,
};
pub use tenant_lifecycle_audit::{
    TenantLifecycleAudit, TenantLifecycleAuditCursor, TenantLifecycleAuditEntry,
    TenantLifecycleAuditError, TenantLifecycleAuditPage, TenantLifecycleAuditPageSize,
    TenantLifecycleAuditPageSizeError, TenantLifecycleAuditQuery, TenantLifecycleAuditRepository,
};
pub use tenant_provisioning::{
    TenantProvisioningCommand, TenantProvisioningError, TenantProvisioningOutcome,
    TenantProvisioningRepository, TenantProvisioningRequest, TenantProvisioningService,
};
