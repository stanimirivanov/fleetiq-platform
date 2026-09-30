//! Authentication and authorization policy for `FleetIQ`.
//!
//! This crate owns the consumer-facing identity-resolution and access-policy
//! ports, the closed action vocabulary, and evidence produced by successful
//! authentication and authorization. Evidence has no public constructor;
//! adapters supply trusted identity and current policy decisions.

mod actor_identity_resolver;
mod authorization;
mod caller_authentication;
mod platform_authorization;

pub use actor_identity_resolver::ActorIdentityResolver;
pub use authorization::{
    AccessDecision, AuthorizedTenant, ManageAccess, ManageMetadata, ReadMetadata,
    TenantAccessPolicy, TenantAccessRequest, TenantAction, TenantActionMarker,
    TenantAuthorizationError, TenantAuthorizationService,
};
pub use caller_authentication::{
    AuthenticatedActor, CallerAuthenticationError, CallerAuthenticationService,
    ExternalIdentityAuthenticator,
};
pub use platform_authorization::{
    AuthorizedPlatform, ManageIdentity, ManagePlatformAccess, ManageTenantLifecycle,
    PlatformAccessPolicy, PlatformAccessRequest, PlatformAction, PlatformActionMarker,
    PlatformAuthorizationError, PlatformAuthorizationService, ProvisionTenant,
};
