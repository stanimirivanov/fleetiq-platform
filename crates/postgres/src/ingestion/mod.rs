//! PostgreSQL adapters for the ingestion capability.

mod accepted_message_log;
mod decoded_observation_log;
mod protocol_package_catalog;

pub use accepted_message_log::{PostgresAcceptedMessageLog, PostgresAcceptedMessageLogError};
pub use decoded_observation_log::{
    PostgresDecodedObservationLog, PostgresDecodedObservationLogError,
};
pub use protocol_package_catalog::{
    PostgresProtocolPackageCatalog, PostgresProtocolPackageCatalogError,
};

mod protocol_artifact_store;
pub use protocol_artifact_store::PostgresProtocolArtifactStore;

mod protocol_routing;
pub use protocol_routing::PostgresProtocolRoutingRepository;
