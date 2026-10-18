use std::{env, error::Error, fmt};

use fleetiq_postgres::MIGRATOR;
use serde::Serialize;
use sqlx::postgres::PgPoolOptions;

use crate::{SchemaMode, SchemaPreparationError, ServerConfigError};

/// Database-only configuration for an explicit one-shot migration process.
///
/// HTTP, OIDC and bootstrap settings are deliberately not read. Supply temporary
/// migration-owner credentials through the environment, never command arguments.
#[derive(Clone)]
pub struct MigrationConfig {
    database_url: String,
}

impl MigrationConfig {
    /// Reads only `DATABASE_URL`.
    ///
    /// # Errors
    /// Returns [`ServerConfigError`] for a missing, non-Unicode or empty URL.
    pub fn from_env() -> Result<Self, ServerConfigError> {
        let value = env::var("DATABASE_URL").map_err(|e| match e {
            env::VarError::NotPresent => ServerConfigError::MissingEnvironment {
                name: "DATABASE_URL",
            },
            env::VarError::NotUnicode(_) => ServerConfigError::NonUnicodeEnvironment {
                name: "DATABASE_URL",
            },
        })?;
        Self::from_values(value)
    }

    /// Validates an injected connection string without connecting.
    ///
    /// # Errors
    /// Returns [`ServerConfigError::EmptyDatabaseUrl`] for an empty value.
    pub fn from_values(database_url: impl Into<String>) -> Result<Self, ServerConfigError> {
        let database_url = database_url.into();
        if database_url.trim().is_empty() {
            return Err(ServerConfigError::EmptyDatabaseUrl);
        }
        Ok(Self { database_url })
    }
}
impl fmt::Debug for MigrationConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MigrationConfig")
            .field("database_url", &"[REDACTED]")
            .finish()
    }
}

/// Applies the embedded chain with `SQLx` advisory locking, validates it and exits.
///
/// No listener, identity client or background worker is started. Connections are
/// closed on success and failure, releasing a migration lock even after an error.
/// The caller must independently fence existing writers before changing schema.
///
/// # Errors
/// Returns a redacted connection or migration failure; no automatic rollback or retry.
pub async fn migrate_database(config: MigrationConfig) -> Result<(), MigrationCommandError> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&config.database_url)
        .await
        .map_err(MigrationCommandError::Connection)?;
    let result = SchemaMode::Migrate
        .prepare(&pool)
        .await
        .map_err(MigrationCommandError::Schema);
    pool.close().await;
    result
}

/// Non-secret failure from the migration-only process.
pub enum MigrationCommandError {
    /// A database connection could not be established.
    Connection(sqlx::Error),
    /// Applying or verifying the migration chain failed.
    Schema(SchemaPreparationError),
}
impl fmt::Display for MigrationCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(_) => f.write_str("migration database connection failed"),
            Self::Schema(e) => e.fmt(f),
        }
    }
}
impl fmt::Debug for MigrationCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for MigrationCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Schema(e) => Some(e),
            Self::Connection(error) => Some(error),
        }
    }
}

#[derive(Serialize)]
struct Inventory {
    format: &'static str,
    checksum_algorithm: &'static str,
    migrations: Vec<Entry>,
}
#[derive(Serialize)]
struct Entry {
    version: String,
    description: String,
    checksum: String,
}

/// Serializes this binary's ordered migration versions and SHA-384 checksums.
///
/// Requires no environment or database. Bind this inventory to the image digest
/// and source revision in producer provenance; it does not attest applied state.
///
/// # Errors
/// Returns a serialization error if the maintained JSON serializer fails.
pub fn migration_inventory() -> Result<String, serde_json::Error> {
    use std::fmt::Write;
    let migrations = MIGRATOR
        .iter()
        .map(|migration| {
            let mut checksum = String::with_capacity(migration.checksum.len() * 2);
            for byte in migration.checksum.iter() {
                // Writing to String cannot fail.
                let _ = write!(checksum, "{byte:02x}");
            }
            Entry {
                version: migration.version.to_string(),
                description: migration.description.to_string(),
                checksum,
            }
        })
        .collect();
    serde_json::to_string(&Inventory {
        format: "fleetiq-migrations.v1",
        checksum_algorithm: "sha384",
        migrations,
    })
}
