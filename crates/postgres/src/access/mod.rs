//! PostgreSQL adapters for the access capability.

mod actor_identity_resolver;
mod platform_access_policy;
mod tenant_access_policy;
mod tenant_discovery;
pub use tenant_discovery::PostgresTenantDiscoveryRepository;

pub use actor_identity_resolver::{
    PostgresActorIdentityResolver, PostgresActorIdentityResolverError,
};
pub use platform_access_policy::{PostgresPlatformAccessPolicy, PostgresPlatformAccessPolicyError};
pub use tenant_access_policy::{PostgresTenantAccessPolicy, PostgresTenantAccessPolicyError};
