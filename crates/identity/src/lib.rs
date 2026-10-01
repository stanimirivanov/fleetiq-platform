//! Identity and tenant administration use cases and consumer-owned ports.
//!
//! Access policy issues exact authorization evidence. This crate consumes that
//! evidence to orchestrate actor, external-identity, grant, tenant, and
//! bootstrap operations; PostgreSQL implements the persistence ports.

use fleetiq_access::{
    AuthorizedPlatform, AuthorizedTenant, ManageAccess, ManageIdentity, ManagePlatformAccess,
    ManageTenantLifecycle, PlatformAction, ProvisionTenant, TenantAction,
};

#[cfg(test)]
use fleetiq_access::{
    AccessDecision, ActorIdentityResolver, AuthenticatedActor, CallerAuthenticationService,
    ExternalIdentityAuthenticator, PlatformAccessPolicy, PlatformAccessRequest,
    PlatformActionMarker, PlatformAuthorizationService, TenantAccessPolicy, TenantAccessRequest,
    TenantActionMarker, TenantAuthorizationService,
};

mod actor_catalog;
mod actor_lifecycle_administration;
mod actor_lifecycle_audit;
mod actor_provisioning;
mod external_identity_administration;
mod external_identity_audit;
mod external_identity_catalog;
mod pagination;
mod platform_access_administration;
mod platform_access_audit;
mod platform_bootstrap;
mod platform_grant_catalog;
mod tenant_access_administration;
mod tenant_access_audit;
mod tenant_catalog;
mod tenant_lifecycle_administration;
mod tenant_lifecycle_audit;
mod tenant_provisioning;

#[cfg(test)]
mod test_support;

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
