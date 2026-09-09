//! Application ports and use-case orchestration for `FleetIQ`.
//!
//! Ports are owned by their consumers. Infrastructure crates implement these
//! contracts without exposing database or transport types to domain code.

mod definition_repository;
mod repository;

pub use definition_repository::DefinitionRepository;
pub use repository::AssetRepository;
