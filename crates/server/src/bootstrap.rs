use std::{
    env,
    error::Error,
    fmt,
    time::{SystemTime, SystemTimeError, UNIX_EPOCH},
};

use fleetiq_application::{
    PlatformBootstrapError, PlatformBootstrapOutcome, PlatformBootstrapRequest,
    PlatformBootstrapService,
};
use fleetiq_domain::{
    ActorId, ExternalIdentity, ExternalIdentitySubject, ExternalIdentitySubjectError,
    IdentifierError, IdentityAuthorityId, UnixTimeMillis,
};
use fleetiq_postgres::{MIGRATOR, PostgresPlatformBootstrap, PostgresPlatformBootstrapError};
use sqlx::postgres::PgPoolOptions;

const DATABASE_URL: &str = "DATABASE_URL";
const ACTOR_ID: &str = "FLEETIQ_BOOTSTRAP_ACTOR_ID";
const AUTHORITY_ID: &str = "FLEETIQ_BOOTSTRAP_AUTHORITY_ID";
const EXTERNAL_SUBJECT: &str = "FLEETIQ_BOOTSTRAP_EXTERNAL_SUBJECT";

/// Validated configuration for the explicit platform-bootstrap command.
///
/// The database URL can contain credentials and the external subject can be
/// personally identifying. Both values are redacted from `Debug` output.
#[derive(Clone)]
pub struct PlatformBootstrapConfig {
    database_url: String,
    actor_id: ActorId,
    external_identity: ExternalIdentity,
}

impl PlatformBootstrapConfig {
    /// Reads the database and initial identity from dedicated environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformBootstrapConfigError`] when a required value is absent,
    /// non-Unicode, empty, or violates its domain invariant.
    pub fn from_env() -> Result<Self, PlatformBootstrapConfigError> {
        Self::from_values(
            required_environment(DATABASE_URL)?,
            required_environment(ACTOR_ID)?,
            required_environment(AUTHORITY_ID)?,
            required_environment(EXTERNAL_SUBJECT)?,
        )
    }

    /// Validates explicit bootstrap configuration values.
    ///
    /// This constructor supports deterministic adapter tests and configuration
    /// sources other than the process environment.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformBootstrapConfigError`] when the database URL is empty
    /// or an identity value violates its domain invariant.
    pub fn from_values(
        database_url: impl Into<String>,
        actor_id: impl Into<String>,
        authority_id: impl Into<String>,
        external_subject: impl Into<String>,
    ) -> Result<Self, PlatformBootstrapConfigError> {
        let database_url = database_url.into();
        if database_url.trim().is_empty() {
            return Err(PlatformBootstrapConfigError::EmptyDatabaseUrl);
        }
        let actor_id = ActorId::new(actor_id).map_err(PlatformBootstrapConfigError::ActorId)?;
        let authority_id = IdentityAuthorityId::new(authority_id)
            .map_err(PlatformBootstrapConfigError::AuthorityId)?;
        let external_subject = ExternalIdentitySubject::new(external_subject)
            .map_err(PlatformBootstrapConfigError::ExternalSubject)?;
        Ok(Self {
            database_url,
            actor_id,
            external_identity: ExternalIdentity::new(authority_id, external_subject),
        })
    }

    fn request(&self, recorded_at: UnixTimeMillis) -> PlatformBootstrapRequest {
        PlatformBootstrapRequest::new(
            self.actor_id.clone(),
            self.external_identity.clone(),
            recorded_at,
        )
    }
}

impl fmt::Debug for PlatformBootstrapConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PlatformBootstrapConfig")
            .field("database_url", &"[REDACTED]")
            .field("actor_id", &self.actor_id)
            .field("authority_id", &self.external_identity.authority_id())
            .field("external_subject", &"[REDACTED]")
            .finish()
    }
}

/// Runs the explicit one-time platform bootstrap operation and closes its pool.
///
/// The command applies the embedded migration chain before attempting
/// bootstrap, making it suitable for a new empty installation. It does not
/// start an HTTP listener and emits no identity or database values.
///
/// # Errors
///
/// Returns [`PlatformBootstrapCommandError`] when database connection,
/// migration, system-clock conversion, or atomic bootstrap fails.
pub async fn bootstrap_platform_operator(
    config: PlatformBootstrapConfig,
) -> Result<PlatformBootstrapOutcome, PlatformBootstrapCommandError> {
    let pool = PgPoolOptions::new()
        .connect(&config.database_url)
        .await
        .map_err(PlatformBootstrapCommandError::DatabaseConnection)?;

    let result = async {
        MIGRATOR
            .run(&pool)
            .await
            .map_err(PlatformBootstrapCommandError::Migration)?;
        let recorded_at = current_unix_time()?;
        PlatformBootstrapService::new(PostgresPlatformBootstrap::new(pool.clone()))
            .bootstrap(&config.request(recorded_at))
            .await
            .map_err(PlatformBootstrapCommandError::Bootstrap)
    }
    .await;
    pool.close().await;
    result
}

fn current_unix_time() -> Result<UnixTimeMillis, PlatformBootstrapCommandError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(PlatformBootstrapCommandError::Clock)?;
    let milliseconds = i64::try_from(duration.as_millis())
        .map_err(|_| PlatformBootstrapCommandError::TimestampOutOfRange)?;
    Ok(UnixTimeMillis::new(milliseconds))
}

fn required_environment(name: &'static str) -> Result<String, PlatformBootstrapConfigError> {
    env::var(name).map_err(|error| match error {
        env::VarError::NotPresent => PlatformBootstrapConfigError::MissingEnvironment { name },
        env::VarError::NotUnicode(_) => {
            PlatformBootstrapConfigError::NonUnicodeEnvironment { name }
        }
    })
}

/// Invalid or unavailable platform-bootstrap configuration.
#[derive(Debug)]
pub enum PlatformBootstrapConfigError {
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
    /// The internal actor identifier is invalid.
    ActorId(IdentifierError),
    /// The trusted authority identifier is invalid.
    AuthorityId(IdentifierError),
    /// The external subject is invalid.
    ExternalSubject(ExternalIdentitySubjectError),
}

impl fmt::Display for PlatformBootstrapConfigError {
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
            Self::ActorId(_) => formatter.write_str("bootstrap actor ID is invalid"),
            Self::AuthorityId(_) => formatter.write_str("bootstrap authority ID is invalid"),
            Self::ExternalSubject(_) => {
                formatter.write_str("bootstrap external subject is invalid")
            }
        }
    }
}

impl Error for PlatformBootstrapConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ActorId(error) | Self::AuthorityId(error) => Some(error),
            Self::ExternalSubject(error) => Some(error),
            Self::MissingEnvironment { .. }
            | Self::NonUnicodeEnvironment { .. }
            | Self::EmptyDatabaseUrl => None,
        }
    }
}

/// Failure while executing the platform-bootstrap process command.
#[derive(Debug)]
pub enum PlatformBootstrapCommandError {
    /// PostgreSQL could not be reached with the configured connection string.
    DatabaseConnection(sqlx::Error),
    /// The embedded migration chain could not be applied.
    Migration(sqlx::migrate::MigrateError),
    /// The host clock is earlier than the Unix epoch.
    Clock(SystemTimeError),
    /// The current Unix-millisecond value does not fit the persisted contract.
    TimestampOutOfRange,
    /// The atomic application bootstrap operation failed.
    Bootstrap(PlatformBootstrapError<PostgresPlatformBootstrapError>),
}

impl fmt::Display for PlatformBootstrapCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatabaseConnection(_) => {
                formatter.write_str("failed to connect to PostgreSQL for platform bootstrap")
            }
            Self::Migration(_) => {
                formatter.write_str("failed to apply PostgreSQL migrations for platform bootstrap")
            }
            Self::Clock(_) => formatter.write_str("host clock is earlier than the Unix epoch"),
            Self::TimestampOutOfRange => {
                formatter.write_str("current Unix time exceeds the supported millisecond range")
            }
            Self::Bootstrap(_) => formatter.write_str("platform operator bootstrap failed"),
        }
    }
}

impl Error for PlatformBootstrapCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::DatabaseConnection(error) => Some(error),
            Self::Migration(error) => Some(error),
            Self::Clock(error) => Some(error),
            Self::Bootstrap(error) => Some(error),
            Self::TimestampOutOfRange => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_values_validate_identity_and_redact_sensitive_configuration() {
        let config = PlatformBootstrapConfig::from_values(
            "postgres://user:secret@localhost/fleetiq",
            "operator-a",
            "workforce-oidc",
            "external-subject-a",
        )
        .expect("bootstrap configuration should be valid");

        let debug = format!("{config:?}");
        assert!(debug.contains("operator-a"));
        assert!(debug.contains("workforce-oidc"));
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("external-subject-a"));
    }

    #[test]
    fn invalid_database_and_identity_values_are_rejected() {
        assert!(matches!(
            PlatformBootstrapConfig::from_values(" ", "operator-a", "authority-a", "subject-a"),
            Err(PlatformBootstrapConfigError::EmptyDatabaseUrl)
        ));
        assert!(matches!(
            PlatformBootstrapConfig::from_values(
                "postgres://localhost/fleetiq",
                " operator-a",
                "authority-a",
                "subject-a",
            ),
            Err(PlatformBootstrapConfigError::ActorId(_))
        ));
        assert!(matches!(
            PlatformBootstrapConfig::from_values(
                "postgres://localhost/fleetiq",
                "operator-a",
                " authority-a",
                "subject-a",
            ),
            Err(PlatformBootstrapConfigError::AuthorityId(_))
        ));
        assert!(matches!(
            PlatformBootstrapConfig::from_values(
                "postgres://localhost/fleetiq",
                "operator-a",
                "authority-a",
                "",
            ),
            Err(PlatformBootstrapConfigError::ExternalSubject(_))
        ));
    }
}
