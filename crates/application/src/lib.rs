//! Application ports and use-case orchestration for `FleetIQ`.
//!
//! Ports are owned by their consumers. Infrastructure crates implement these
//! contracts without exposing database or transport types to domain code.

mod definition_repository;
mod device_repository;
mod relationship_repository;
mod repository;

pub use definition_repository::DefinitionRepository;
pub use device_repository::DeviceRepository;
pub use relationship_repository::{RelationshipRepository, RelationshipSnapshotQuery};
pub use repository::AssetRepository;
