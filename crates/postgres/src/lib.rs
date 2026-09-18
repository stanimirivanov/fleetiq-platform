//! PostgreSQL persistence adapters for `FleetIQ`.
//!
//! Database row types remain private to this crate. Public operations accept and
//! return validated domain values through application-owned repository ports.

mod accepted_message_log;
mod asset_repository;
mod decoded_observation_log;
mod definition_repository;
mod device_repository;
mod error;
mod protocol_package_catalog;
mod relationship_repository;
mod signal_binding_repository;
mod tenant_access_policy;

pub use accepted_message_log::{PostgresAcceptedMessageLog, PostgresAcceptedMessageLogError};
pub use asset_repository::PostgresAssetRepository;
pub use decoded_observation_log::{
    PostgresDecodedObservationLog, PostgresDecodedObservationLogError,
};
pub use definition_repository::PostgresDefinitionRepository;
pub use device_repository::PostgresDeviceRepository;
pub use error::PostgresRepositoryError;
pub use protocol_package_catalog::{
    PostgresProtocolPackageCatalog, PostgresProtocolPackageCatalogError,
};
pub use relationship_repository::PostgresRelationshipRepository;
pub use signal_binding_repository::PostgresSignalBindingRepository;
use sqlx::migrate::Migrator;
pub use tenant_access_policy::{PostgresTenantAccessPolicy, PostgresTenantAccessPolicyError};

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
