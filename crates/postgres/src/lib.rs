//! PostgreSQL persistence adapters for `FleetIQ`.
//!
//! Database row types remain private to this crate. Public operations accept and
//! return validated domain values through application-owned repository ports.

mod asset_repository;
mod definition_repository;
mod device_repository;
mod error;
mod relationship_repository;

pub use asset_repository::PostgresAssetRepository;
pub use definition_repository::PostgresDefinitionRepository;
pub use device_repository::PostgresDeviceRepository;
pub use error::PostgresRepositoryError;
pub use relationship_repository::PostgresRelationshipRepository;
use sqlx::migrate::Migrator;

/// Error returned by PostgreSQL asset repository operations.
pub type PostgresAssetRepositoryError = PostgresRepositoryError;

/// Error returned by PostgreSQL semantic-definition repository operations.
pub type PostgresDefinitionRepositoryError = PostgresRepositoryError;

/// Error returned by PostgreSQL communication-device repository operations.
pub type PostgresDeviceRepositoryError = PostgresRepositoryError;

/// Error returned by PostgreSQL asset-relationship repository operations.
pub type PostgresRelationshipRepositoryError = PostgresRepositoryError;

/// Product metadata migrations embedded for deployment and isolated tests.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");
