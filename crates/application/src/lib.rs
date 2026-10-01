//! Live projection contracts and compatibility exports for `FleetIQ`.
//!
//! Capability crates own their use cases and consumer-owned ports. This facade
//! preserves existing import paths while callers migrate to those crates.

mod live;

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
pub use fleetiq_ingestion::{
    AcceptedMessageAppendOutcome, AcceptedMessageLog, DecodedObservationAppendOutcome,
    DecodedObservationBatch, DecodedObservationBatchError, DecodedObservationLog,
    DurablyAcceptedFrame, DurablyPersistedFrame, FrameAcceptanceContext, FrameAcceptanceError,
    ObservationPersistenceError, ObservationPersistencePreparationError, PendingAcceptedMessage,
    PendingFrameAcceptance, PendingObservationPersistence, PreparedFrameAcceptance,
    ProtocolPackageCatalog, ProtocolPackageRegistrationOutcome, ProtocolPackageRelease,
};
pub use fleetiq_metadata::{
    AssetApplicationError, AssetApplicationService, AssetRepository, DefinitionApplicationError,
    DefinitionApplicationService, DefinitionRepository, DeviceApplicationError,
    DeviceApplicationService, DeviceRepository, RelationshipApplicationError,
    RelationshipApplicationService, RelationshipRepository, RelationshipSnapshotQuery,
    SignalBindingApplicationError, SignalBindingApplicationService, SignalBindingRepository,
    SignalBindingSnapshotQuery,
};
pub use live::{
    LiveMessage, ProjectionDelta, ProjectionDeltaError, ProjectionRevision, ProjectionSnapshot,
    ResnapshotReason, ResnapshotRequired, RevisionCursor, RevisionCursorError,
    RevisionOverflowError,
};
