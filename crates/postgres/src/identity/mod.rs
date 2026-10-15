//! PostgreSQL adapters for the identity capability.

mod actor_catalog;
mod actor_lifecycle_administration;
mod actor_lifecycle_audit;
mod actor_provisioning;
mod external_identity_administration;
mod external_identity_audit;
mod external_identity_catalog;
mod platform_access_administration;
mod platform_access_audit;
mod platform_bootstrap;
mod platform_grant_catalog;
mod tenant_access_administration;
mod tenant_access_audit;
mod tenant_catalog;
mod tenant_grant_catalog;
mod tenant_lifecycle_administration;
mod tenant_lifecycle_audit;
mod tenant_provisioning;

pub use actor_catalog::{PostgresActorCatalogRepository, PostgresActorCatalogRepositoryError};
pub use actor_lifecycle_administration::{
    PostgresActorLifecycleAdministration, PostgresActorLifecycleAdministrationError,
};
pub use actor_lifecycle_audit::{
    PostgresActorLifecycleAuditRepository, PostgresActorLifecycleAuditRepositoryError,
};
pub use actor_provisioning::{PostgresActorProvisioning, PostgresActorProvisioningError};
pub use external_identity_administration::{
    PostgresExternalIdentityAdministration, PostgresExternalIdentityAdministrationError,
};
pub use external_identity_audit::{
    PostgresExternalIdentityAuditRepository, PostgresExternalIdentityAuditRepositoryError,
};
pub use external_identity_catalog::{
    PostgresExternalIdentityCatalogRepository, PostgresExternalIdentityCatalogRepositoryError,
};
pub use platform_access_administration::{
    PostgresPlatformAccessAdministration, PostgresPlatformAccessAdministrationError,
};
pub use platform_access_audit::{
    PostgresPlatformAccessAuditRepository, PostgresPlatformAccessAuditRepositoryError,
};
pub use platform_bootstrap::{PostgresPlatformBootstrap, PostgresPlatformBootstrapError};
pub use platform_grant_catalog::{
    PostgresPlatformGrantCatalogRepository, PostgresPlatformGrantCatalogRepositoryError,
};
pub use tenant_access_administration::{
    PostgresTenantAccessAdministration, PostgresTenantAccessAdministrationError,
};
pub use tenant_access_audit::{
    PostgresTenantAccessAuditRepository, PostgresTenantAccessAuditRepositoryError,
};
pub use tenant_catalog::{PostgresTenantCatalogRepository, PostgresTenantCatalogRepositoryError};
pub use tenant_lifecycle_administration::{
    PostgresTenantLifecycleAdministration, PostgresTenantLifecycleAdministrationError,
};
pub use tenant_lifecycle_audit::{
    PostgresTenantLifecycleAuditRepository, PostgresTenantLifecycleAuditRepositoryError,
};
pub use tenant_provisioning::{PostgresTenantProvisioning, PostgresTenantProvisioningError};

pub use tenant_grant_catalog::{
    PostgresTenantGrantCatalogRepository, PostgresTenantGrantCatalogRepositoryError,
};
