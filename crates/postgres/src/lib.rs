//! PostgreSQL persistence adapters for `FleetIQ`.
//!
//! Database row types remain private to this crate. Public operations accept and
//! return validated domain values through application and semantic-policy ports.

mod access;
mod action_codec;
mod browser_identity_repository;
mod error;
mod history;
mod identity;
mod ingestion;
mod metadata;

pub use access::{
    PostgresActorIdentityResolver, PostgresActorIdentityResolverError,
    PostgresPlatformAccessPolicy, PostgresPlatformAccessPolicyError, PostgresTenantAccessPolicy,
    PostgresTenantAccessPolicyError, PostgresTenantDiscoveryRepository,
};
pub use browser_identity_repository::{
    BrowserLoginAttempt, BrowserSession, PostgresBrowserIdentityRepository,
};
pub use error::PostgresRepositoryError;
pub use history::{
    PostgresAssetPropertyChangeLog, PostgresAssetPropertyChangeLogError,
    PostgresAssetPropertyChangeView, PostgresDecodedObservationView,
};
pub use identity::{
    PostgresActorCatalogRepository, PostgresActorCatalogRepositoryError,
    PostgresActorLifecycleAdministration, PostgresActorLifecycleAdministrationError,
    PostgresActorLifecycleAuditRepository, PostgresActorLifecycleAuditRepositoryError,
    PostgresActorProvisioning, PostgresActorProvisioningError,
    PostgresExternalIdentityAdministration, PostgresExternalIdentityAdministrationError,
    PostgresExternalIdentityAuditRepository, PostgresExternalIdentityAuditRepositoryError,
    PostgresExternalIdentityCatalogRepository, PostgresExternalIdentityCatalogRepositoryError,
    PostgresPlatformAccessAdministration, PostgresPlatformAccessAdministrationError,
    PostgresPlatformAccessAuditRepository, PostgresPlatformAccessAuditRepositoryError,
    PostgresPlatformBootstrap, PostgresPlatformBootstrapError,
    PostgresPlatformGrantCatalogRepository, PostgresPlatformGrantCatalogRepositoryError,
    PostgresTenantAccessAdministration, PostgresTenantAccessAdministrationError,
    PostgresTenantAccessAuditRepository, PostgresTenantAccessAuditRepositoryError,
    PostgresTenantCatalogRepository, PostgresTenantCatalogRepositoryError,
    PostgresTenantGrantCatalogRepository, PostgresTenantGrantCatalogRepositoryError,
    PostgresTenantLifecycleAdministration, PostgresTenantLifecycleAdministrationError,
    PostgresTenantLifecycleAuditRepository, PostgresTenantLifecycleAuditRepositoryError,
    PostgresTenantProvisioning, PostgresTenantProvisioningError,
};
pub use ingestion::{
    PostgresAcceptedMessageLog, PostgresAcceptedMessageLogError, PostgresDecodedObservationLog,
    PostgresDecodedObservationLogError, PostgresProtocolPackageCatalog,
    PostgresProtocolPackageCatalogError,
};
pub use metadata::{
    PostgresAssetRepository, PostgresDefinitionRepository, PostgresDeviceRepository,
    PostgresRegistrationRepository, PostgresRelationshipRepository,
    PostgresRevisionCommandRepository, PostgresSignalBindingRepository,
};
mod schema;
pub use schema::{SchemaCompatibilityError, apply_migrations, check_schema};

use sqlx::migrate::Migrator;

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

pub use ingestion::PostgresProtocolArtifactStore;

pub use ingestion::PostgresProtocolRoutingRepository;

pub use ingestion::{PostgresReplayError, PostgresReplayRepository};

pub use ingestion::PostgresIngestionDiagnostics;
