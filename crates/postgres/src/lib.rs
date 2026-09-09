//! PostgreSQL persistence adapters for `FleetIQ`.
//!
//! Database row types remain private to this crate. Public operations accept and
//! return validated domain values through application-owned repository ports.

mod asset_repository;

pub use asset_repository::{PostgresAssetRepository, PostgresAssetRepositoryError};
use sqlx::migrate::Migrator;

/// Product metadata migrations embedded for deployment and isolated tests.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");
