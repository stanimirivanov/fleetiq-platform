//! Same-origin operator sign-in and opaque server-managed browser sessions.
//!
//! Provider credentials never enter browser JavaScript or protected metadata
//! handlers. The callback resolves an already provisioned identity; each later
//! request rechecks that exact identity still maps to the session's actor.

mod authentication;
mod configuration;
mod credential;
mod provider;
mod routes;
#[cfg(test)]
mod tests;

pub(crate) use authentication::BrowserRequestAuthenticator;
pub use configuration::{BrowserIdentityConfig, BrowserIdentityConfigError};
pub(crate) use credential::now;
pub(crate) use provider::OidcClient;
pub(crate) use routes::postgres_browser_router;
