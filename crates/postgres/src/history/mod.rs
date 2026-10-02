//! PostgreSQL adapter for immutable semantic property history.

mod property_change_log;

pub use property_change_log::{
    PostgresAssetPropertyChangeLog, PostgresAssetPropertyChangeLogError,
};
