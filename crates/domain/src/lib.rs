//! Framework-independent `FleetIQ` domain types and invariants.
//!
//! The domain crate has no infrastructure dependencies. Transport and persistence
//! adapters must construct these types through their validating APIs rather than
//! duplicating domain rules.

mod identifier;

pub use identifier::{AssetId, IdentifierError, TenantId};
