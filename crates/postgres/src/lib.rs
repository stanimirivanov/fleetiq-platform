//! PostgreSQL persistence adapters for `FleetIQ`.
//!
//! Database row types remain private to this crate. Public operations accept and
//! return validated domain values through application-owned repository ports.

mod accepted_message_log;
mod actor_identity_resolver;
mod actor_provisioning;
mod asset_repository;
mod decoded_observation_log;
mod definition_repository;
mod device_repository;
mod error;
mod external_identity_administration;
mod platform_access_administration;
mod platform_access_audit;
mod platform_access_policy;
mod platform_bootstrap;
mod protocol_package_catalog;
mod relationship_repository;
mod signal_binding_repository;
mod tenant_access_administration;
mod tenant_access_audit;
mod tenant_access_policy;
mod tenant_provisioning;

pub use accepted_message_log::{PostgresAcceptedMessageLog, PostgresAcceptedMessageLogError};
pub use actor_identity_resolver::{
    PostgresActorIdentityResolver, PostgresActorIdentityResolverError,
};
pub use actor_provisioning::{PostgresActorProvisioning, PostgresActorProvisioningError};
pub use asset_repository::PostgresAssetRepository;
pub use decoded_observation_log::{
    PostgresDecodedObservationLog, PostgresDecodedObservationLogError,
};
pub use definition_repository::PostgresDefinitionRepository;
pub use device_repository::PostgresDeviceRepository;
pub use error::PostgresRepositoryError;
pub use external_identity_administration::{
    PostgresExternalIdentityAdministration, PostgresExternalIdentityAdministrationError,
};
pub use platform_access_administration::{
    PostgresPlatformAccessAdministration, PostgresPlatformAccessAdministrationError,
};
pub use platform_access_audit::{
    PostgresPlatformAccessAuditRepository, PostgresPlatformAccessAuditRepositoryError,
};
pub use platform_access_policy::{PostgresPlatformAccessPolicy, PostgresPlatformAccessPolicyError};
pub use platform_bootstrap::{PostgresPlatformBootstrap, PostgresPlatformBootstrapError};
pub use protocol_package_catalog::{
    PostgresProtocolPackageCatalog, PostgresProtocolPackageCatalogError,
};
pub use relationship_repository::PostgresRelationshipRepository;
pub use signal_binding_repository::PostgresSignalBindingRepository;
use sqlx::migrate::Migrator;
pub use tenant_access_administration::{
    PostgresTenantAccessAdministration, PostgresTenantAccessAdministrationError,
};
pub use tenant_access_audit::{
    PostgresTenantAccessAuditRepository, PostgresTenantAccessAuditRepositoryError,
};
pub use tenant_access_policy::{PostgresTenantAccessPolicy, PostgresTenantAccessPolicyError};
pub use tenant_provisioning::{PostgresTenantProvisioning, PostgresTenantProvisioningError};

/// Error returned by PostgreSQL asset repository operations.
pub type PostgresAssetRepositoryError = PostgresRepositoryError;

/// Error returned by PostgreSQL semantic-definition repository operations.
pub type PostgresDefinitionRepositoryError = PostgresRepositoryError;

/// Error returned by PostgreSQL communication-device repository operations.
pub type PostgresDeviceRepositoryError = PostgresRepositoryError;

/// Error returned by PostgreSQL asset-relationship repository operations.
pub type PostgresRelationshipRepositoryError = PostgresRepositoryError;

/// Error returned by PostgreSQL signal-binding repository operations.
pub type PostgresSignalBindingRepositoryError = PostgresRepositoryError;

/// Product database migrations embedded for deployment and isolated tests.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");
