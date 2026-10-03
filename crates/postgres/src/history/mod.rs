//! PostgreSQL adapter for immutable semantic property history.

mod decoded_observation_view;
mod property_change_log;

pub use decoded_observation_view::PostgresDecodedObservationView;

pub use property_change_log::{
    PostgresAssetPropertyChangeLog, PostgresAssetPropertyChangeLogError,
};
