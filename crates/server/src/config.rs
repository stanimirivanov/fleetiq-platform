use std::{env, error::Error, fmt, net::SocketAddr};

use fleetiq_domain::{ExternalIdentity, ExternalIdentitySubject, IdentityAuthorityId};

use crate::browser_identity::{BrowserIdentityConfig, BrowserIdentityConfigError};

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8080";
const BEARER_TOKEN: &str = "FLEETIQ_HTTP_BEARER_TOKEN";
const BEARER_AUTHORITY: &str = "FLEETIQ_HTTP_IDENTITY_AUTHORITY";
const BEARER_SUBJECT: &str = "FLEETIQ_HTTP_IDENTITY_SUBJECT";

/// One configured bearer credential and the identity it establishes.
///
/// The credential is a 256-bit random value encoded as 64 hexadecimal ASCII
/// characters. This deliberately narrow adapter supports one configured
/// principal; the exact external identity must be provisioned separately.
#[derive(Clone)]
pub struct BearerIdentityConfig {
    token: [u8; 64],
    external_identity: ExternalIdentity,
}

impl BearerIdentityConfig {
    pub(crate) const fn token(&self) -> &[u8; 64] {
        &self.token
    }

    pub(crate) fn external_identity(&self) -> &ExternalIdentity {
        &self.external_identity
    }
}

impl fmt::Debug for BearerIdentityConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BearerIdentityConfig")
            .field("token", &"[REDACTED]")
            .field("external_identity", &"[REDACTED]")
            .finish()
    }
}

/// Validated process configuration for the `FleetIQ` backend.
///
/// The database URL may contain credentials and is therefore excluded from the
/// `Debug` representation. Configuration is immutable after startup.
#[derive(Clone)]
pub struct ServerConfig {
    database_url: String,
    bind_address: SocketAddr,
    bearer_identity: Option<BearerIdentityConfig>,
    browser_identity: Option<BrowserIdentityConfig>,
}

impl ServerConfig {
    /// Reads and validates backend configuration from the process environment.
    ///
    /// `DATABASE_URL` is required. `FLEETIQ_BIND_ADDRESS` defaults to
    /// `127.0.0.1:8080`, keeping an unconfigured development server local.
    /// Protected metadata queries require a complete configured workload
    /// identity or the five browser OIDC values, never both. Browser mode
    /// requires an HTTPS public origin and provider issuer.
    ///
    /// # Errors
    ///
    /// Returns [`ServerConfigError`] when the database URL is absent or empty,
    /// an environment value is not Unicode, the bind address is invalid, or
    /// the optional bearer identity configuration is incomplete or invalid.
    pub fn from_env() -> Result<Self, ServerConfigError> {
        let mut config = Self::from_values(
            env::var("DATABASE_URL").map_err(|error| environment_error("DATABASE_URL", &error))?,
            env::var("FLEETIQ_BIND_ADDRESS")
                .map_err(|error| environment_error("FLEETIQ_BIND_ADDRESS", &error))
                .or_else(|error| match error {
                    ServerConfigError::MissingEnvironment { .. } => {
                        Ok(DEFAULT_BIND_ADDRESS.to_owned())
                    }
                    _ => Err(error),
                })?,
        )?;
        let token = optional_environment(BEARER_TOKEN)?;
        let authority = optional_environment(BEARER_AUTHORITY)?;
        let subject = optional_environment(BEARER_SUBJECT)?;
        match (token, authority, subject) {
            (None, None, None) => {}
            (Some(token), Some(authority), Some(subject)) => {
                config = config.with_bearer_identity(token, authority, subject)?;
            }
            _ => return Err(ServerConfigError::IncompleteBearerIdentity),
        }
        let browser_values = [
            optional_environment("FLEETIQ_BROWSER_ORIGIN")?,
            optional_environment("FLEETIQ_OIDC_ISSUER")?,
            optional_environment("FLEETIQ_OIDC_CLIENT_ID")?,
            optional_environment("FLEETIQ_OIDC_CLIENT_SECRET")?,
            optional_environment("FLEETIQ_OIDC_IDENTITY_AUTHORITY")?,
        ];
        match browser_values {
            [None, None, None, None, None] => {}
            [
                Some(origin),
                Some(issuer),
                Some(client_id),
                Some(secret),
                Some(authority),
            ] => {
                let browser = BrowserIdentityConfig::from_values(
                    &origin, &issuer, &client_id, &secret, &authority,
                )
                .map_err(ServerConfigError::BrowserIdentity)?;
                config = config.with_browser_identity(browser)?;
            }
            _ => return Err(ServerConfigError::IncompleteBrowserIdentity),
        }
        Ok(config)
    }

    /// Validates explicit configuration values.
    ///
    /// This constructor supports deterministic configuration tests and
    /// composition roots that obtain values from a source other than the
    /// process environment.
    ///
    /// # Errors
    ///
    /// Returns [`ServerConfigError`] when the database URL is empty or the bind
    /// address is not a socket address.
    pub fn from_values(
        database_url: impl Into<String>,
        bind_address: impl Into<String>,
    ) -> Result<Self, ServerConfigError> {
        let database_url = database_url.into();
        if database_url.trim().is_empty() {
            return Err(ServerConfigError::EmptyDatabaseUrl);
        }
        let bind_address_text = bind_address.into();
        let bind_address =
            bind_address_text
                .parse()
                .map_err(|source| ServerConfigError::InvalidBindAddress {
                    value: bind_address_text,
                    source,
                })?;
        Ok(Self {
            database_url,
            bind_address,
            bearer_identity: None,
            browser_identity: None,
        })
    }

    /// Enables protected metadata reads with one validated opaque credential.
    ///
    /// # Errors
    ///
    /// Returns [`ServerConfigError`] when the credential is not exactly 64
    /// hexadecimal characters or either identity component is invalid.
    pub fn with_bearer_identity(
        mut self,
        token: impl AsRef<str>,
        authority: impl Into<String>,
        subject: impl Into<String>,
    ) -> Result<Self, ServerConfigError> {
        if self.browser_identity.is_some() {
            return Err(ServerConfigError::AmbiguousHttpIdentity);
        }
        let token = token.as_ref().as_bytes();
        let token: [u8; 64] = token
            .try_into()
            .map_err(|_| ServerConfigError::InvalidBearerToken)?;
        if !token.iter().all(u8::is_ascii_hexdigit) {
            return Err(ServerConfigError::InvalidBearerToken);
        }
        let authority = IdentityAuthorityId::new(authority)
            .map_err(|_| ServerConfigError::InvalidBearerIdentity)?;
        let subject = ExternalIdentitySubject::new(subject)
            .map_err(|_| ServerConfigError::InvalidBearerIdentity)?;
        self.bearer_identity = Some(BearerIdentityConfig {
            token,
            external_identity: ExternalIdentity::new(authority, subject),
        });
        Ok(self)
    }

    /// Enables provider-neutral browser login and same-origin operator sessions.
    ///
    /// Browser and workload modes are mutually exclusive on a listener. This
    /// prevents browser requests from falling back to a configured principal.
    ///
    /// # Errors
    ///
    /// Returns [`ServerConfigError::AmbiguousHttpIdentity`] if a workload
    /// bearer identity was already configured.
    pub fn with_browser_identity(
        mut self,
        browser: BrowserIdentityConfig,
    ) -> Result<Self, ServerConfigError> {
        if self.bearer_identity.is_some() {
            return Err(ServerConfigError::AmbiguousHttpIdentity);
        }
        self.browser_identity = Some(browser);
        Ok(self)
    }

    pub(crate) fn browser_identity(&self) -> Option<&BrowserIdentityConfig> {
        self.browser_identity.as_ref()
    }

    pub(crate) fn database_url(&self) -> &str {
        &self.database_url
    }

    pub(crate) fn bearer_identity(&self) -> Option<&BearerIdentityConfig> {
        self.bearer_identity.as_ref()
    }

    /// Returns the validated HTTP listener address.
    pub const fn bind_address(&self) -> SocketAddr {
        self.bind_address
    }
}

impl fmt::Debug for ServerConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerConfig")
            .field("database_url", &"[REDACTED]")
            .field("bind_address", &self.bind_address)
            .field("bearer_identity", &self.bearer_identity)
            .field("browser_identity", &self.browser_identity)
            .finish()
    }
}

/// Invalid or unavailable backend process configuration.
#[derive(Debug)]
pub enum ServerConfigError {
    /// A required environment variable is absent.
    MissingEnvironment {
        /// Required variable name.
        name: &'static str,
    },
    /// An environment variable is not valid Unicode.
    NonUnicodeEnvironment {
        /// Invalid variable name.
        name: &'static str,
    },
    /// The database connection string is empty.
    EmptyDatabaseUrl,
    /// Only some of the three bearer identity environment values were set.
    IncompleteBearerIdentity,
    /// The bearer credential is not a 256-bit hexadecimal value.
    InvalidBearerToken,
    /// The configured authority or subject violates its domain invariant.
    InvalidBearerIdentity,
    /// Some browser identity environment values are missing.
    IncompleteBrowserIdentity,
    /// Browser and configured workload authentication were enabled together.
    AmbiguousHttpIdentity,
    /// Browser identity configuration violates its security invariants.
    BrowserIdentity(BrowserIdentityConfigError),
    /// The HTTP listener value is not a valid socket address.
    InvalidBindAddress {
        /// Rejected non-secret configuration value.
        value: String,
        /// Address parsing failure.
        source: std::net::AddrParseError,
    },
}

impl fmt::Display for ServerConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironment { name } => {
                write!(formatter, "required environment variable {name} is missing")
            }
            Self::NonUnicodeEnvironment { name } => {
                write!(
                    formatter,
                    "environment variable {name} is not valid Unicode"
                )
            }
            Self::EmptyDatabaseUrl => formatter.write_str("DATABASE_URL must not be empty"),
            Self::IncompleteBearerIdentity => formatter.write_str(
                "HTTP bearer token, identity authority, and subject must be configured together",
            ),
            Self::InvalidBearerToken => {
                formatter.write_str("HTTP bearer token must be 64 hexadecimal characters")
            }
            Self::InvalidBearerIdentity => formatter.write_str("HTTP bearer identity is invalid"),
            Self::IncompleteBrowserIdentity => formatter.write_str("browser origin, OIDC issuer, client ID, client secret, and authority must be configured together"),
            Self::AmbiguousHttpIdentity => formatter.write_str("browser and configured workload identity modes are mutually exclusive"),
            Self::BrowserIdentity(error) => error.fmt(formatter),
            Self::InvalidBindAddress { value, .. } => {
                write!(formatter, "FLEETIQ_BIND_ADDRESS is invalid: {value}")
            }
        }
    }
}

impl Error for ServerConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidBindAddress { source, .. } => Some(source),
            Self::BrowserIdentity(source) => Some(source),
            Self::MissingEnvironment { .. }
            | Self::NonUnicodeEnvironment { .. }
            | Self::EmptyDatabaseUrl
            | Self::IncompleteBearerIdentity
            | Self::InvalidBearerToken
            | Self::InvalidBearerIdentity
            | Self::IncompleteBrowserIdentity
            | Self::AmbiguousHttpIdentity => None,
        }
    }
}

fn environment_error(name: &'static str, error: &env::VarError) -> ServerConfigError {
    match error {
        env::VarError::NotPresent => ServerConfigError::MissingEnvironment { name },
        env::VarError::NotUnicode(_) => ServerConfigError::NonUnicodeEnvironment { name },
    }
}

fn optional_environment(name: &'static str) -> Result<Option<String>, ServerConfigError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(environment_error(name, &error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_values_preserve_the_listener_and_redact_the_database_url() {
        let config =
            ServerConfig::from_values("postgres://user:secret@localhost/fleetiq", "127.0.0.1:9000")
                .expect("test configuration is valid");

        assert_eq!(config.bind_address().to_string(), "127.0.0.1:9000");
        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn empty_database_url_and_invalid_bind_address_are_rejected() {
        assert!(matches!(
            ServerConfig::from_values(" ", DEFAULT_BIND_ADDRESS),
            Err(ServerConfigError::EmptyDatabaseUrl)
        ));
        assert!(matches!(
            ServerConfig::from_values("postgres://localhost/fleetiq", "localhost"),
            Err(ServerConfigError::InvalidBindAddress { .. })
        ));
    }

    #[test]
    fn bearer_configuration_validates_shape_and_redacts_identity() {
        let base = ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0")
            .expect("valid base configuration");
        assert!(matches!(
            base.clone()
                .with_bearer_identity("short", "workload", "reader"),
            Err(ServerConfigError::InvalidBearerToken)
        ));
        assert!(matches!(
            base.clone()
                .with_bearer_identity("a".repeat(64), " workload", "reader"),
            Err(ServerConfigError::InvalidBearerIdentity)
        ));
        let configured = base
            .with_bearer_identity("a".repeat(64), "workload", "private-subject")
            .expect("valid bearer configuration");
        let debug = format!("{configured:?}");
        assert!(!debug.contains(&"a".repeat(64)));
        assert!(!debug.contains("private-subject"));
        assert!(configured.bearer_identity().is_some());
    }

    #[test]
    fn browser_and_workload_modes_cannot_be_enabled_together() {
        let browser = BrowserIdentityConfig::from_values(
            "https://fleet.example",
            "https://identity.example",
            "client",
            "secret",
            "workforce",
        )
        .unwrap();
        let base = ServerConfig::from_values("postgres://localhost/test", "127.0.0.1:0").unwrap();
        assert!(matches!(
            base.clone()
                .with_bearer_identity("a".repeat(64), "workload", "reader")
                .unwrap()
                .with_browser_identity(browser.clone()),
            Err(ServerConfigError::AmbiguousHttpIdentity)
        ));
        assert!(matches!(
            base.with_browser_identity(browser)
                .unwrap()
                .with_bearer_identity("a".repeat(64), "workload", "reader"),
            Err(ServerConfigError::AmbiguousHttpIdentity)
        ));
    }
}
