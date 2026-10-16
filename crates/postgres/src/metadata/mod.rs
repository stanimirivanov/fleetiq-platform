//! PostgreSQL adapters for the metadata capability.

mod asset_repository;
mod definition_repository;
mod device_repository;
mod registration_repository;
mod relationship_repository;
mod signal_binding_repository;

pub use asset_repository::PostgresAssetRepository;
pub use definition_repository::PostgresDefinitionRepository;
pub use device_repository::PostgresDeviceRepository;
pub use relationship_repository::PostgresRelationshipRepository;
pub use signal_binding_repository::PostgresSignalBindingRepository;

pub use registration_repository::PostgresRegistrationRepository;

mod revision_command_repository;
pub use revision_command_repository::PostgresRevisionCommandRepository;
