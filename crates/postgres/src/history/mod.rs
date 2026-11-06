//! PostgreSQL adapter for immutable semantic property history.

mod decoded_observation_view;
mod property_change_log;
mod property_change_view;
pub(crate) use property_change_view::row::HistoryRow;

pub use decoded_observation_view::PostgresDecodedObservationView;
pub use property_change_view::PostgresAssetPropertyChangeView;

pub use property_change_log::{
    PostgresAssetPropertyChangeLog, PostgresAssetPropertyChangeLogError,
};

mod work;
pub use work::{PostgresSemanticWorkError, PostgresSemanticWorkRepository};

mod lifecycle_policy;
pub use lifecycle_policy::PostgresLifecyclePolicyRepository;

mod storage_usage;
pub use storage_usage::PostgresStorageUsage;

mod budgets;
pub use budgets::PostgresHistoryBudgets;
