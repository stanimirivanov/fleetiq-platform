use std::{env, error::Error, fmt, net::SocketAddr};

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8080";

/// Validated process configuration for the `FleetIQ` backend.
///
/// The database URL may contain credentials and is therefore excluded from the
/// `Debug` representation. Configuration is immutable after startup.
#[derive(Clone)]
pub struct ServerConfig {
    database_url: String,
    bind_address: SocketAddr,
}

impl ServerConfig {
    /// Reads and validates backend configuration from the process environment.
    ///
    /// `DATABASE_URL` is required. `FLEETIQ_BIND_ADDRESS` defaults to
    /// `127.0.0.1:8080`, keeping an unconfigured development server local.
    ///
    /// # Errors
    ///
    /// Returns [`ServerConfigError`] when the database URL is absent or empty,
    /// an environment value is not Unicode, or the bind address is invalid.
    pub fn from_env() -> Result<Self, ServerConfigError> {
        Self::from_values(
            env::var("DATABASE_URL").map_err(|error| environment_error("DATABASE_URL", &error))?,
            env::var("FLEETIQ_BIND_ADDRESS")
                .map_err(|error| environment_error("FLEETIQ_BIND_ADDRESS", &error))
                .or_else(|error| match error {
                    ServerConfigError::MissingEnvironment { .. } => {
                        Ok(DEFAULT_BIND_ADDRESS.to_owned())
                    }
                    _ => Err(error),
                })?,
        )
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
        })
    }

    pub(crate) fn database_url(&self) -> &str {
        &self.database_url
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
            Self::MissingEnvironment { .. }
            | Self::NonUnicodeEnvironment { .. }
            | Self::EmptyDatabaseUrl => None,
        }
    }
}

fn environment_error(name: &'static str, error: &env::VarError) -> ServerConfigError {
    match error {
        env::VarError::NotPresent => ServerConfigError::MissingEnvironment { name },
        env::VarError::NotUnicode(_) => ServerConfigError::NonUnicodeEnvironment { name },
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
}
