//! Explicit PostgreSQL migration and read-only compatibility for the embedded `SQLx` inventory.

use std::{error::Error, fmt};

use sqlx::{PgPool, migrate::MigrateError};

use crate::MIGRATOR;

/// Applies embedded migrations in an owned session with an explicit public search path.
///
/// This retains `SQLx` locking and transactional semantics. The connection is
/// closed rather than returned to application traffic, including cancellation,
/// so migration locks and session settings cannot leak through the shared pool.
/// Fixing the search path also pins migration ledger lookup;
/// a role-specific schema cannot shadow the public migration ledger.
///
/// # Errors
/// Returns [`MigrateError`] for connection, session, DDL, history or close failure.
/// Its cause may contain SQL diagnostics; render only a bounded process error.
pub async fn apply_migrations(pool: &PgPool) -> Result<(), MigrateError> {
    let mut connection = pool.acquire().await?;
    connection.close_on_drop();
    let result = async {
        sqlx::query("SET search_path TO public")
            .execute(&mut *connection)
            .await?;
        MIGRATOR.run(&mut *connection).await
    }
    .await;
    let closed = connection.close().await;
    result?;
    closed.map_err(MigrateError::Execute)
}

/// Requires the database ledger to match this binary's complete migration set.
///
/// This performs one bounded, schema-qualified SELECT. It never creates the
/// ledger, acquires a migration lock, or executes DDL, so a runtime role needs
/// only SELECT on `public._sqlx_migrations`. A missing, failed, modified or
/// unknown migration is incompatible. Newer schemas are deliberately refused;
/// compatibility ranges require a separately reviewed deployment contract.
///
/// The snapshot does not fence concurrent DDL. Release orchestration must stop
/// writers before migrating; `SQLx`'s migration lock does not serialize readers.
///
/// # Errors
///
/// Returns [`SchemaCompatibilityError`] for inaccessible/missing history or any
/// mismatch. Database causes remain available to trusted callers, but Debug and
/// Display never include SQL, connection strings or server diagnostics.
pub async fn check_schema(pool: &PgPool) -> Result<(), SchemaCompatibilityError> {
    // Read at most one more row than the binary knows: even a foreign ledger
    // cannot turn a readiness probe into an unbounded allocation.
    let limit = i64::try_from(MIGRATOR.iter().count())
        .unwrap_or(i64::MAX - 1)
        .saturating_add(1);
    // SQLx 0.9 checksums are SHA-384 (48 bytes). Keep one extra byte to
    // reject oversized values with a correct prefix without downloading them.
    let applied = sqlx::query!(
        r#"SELECT version, substring(checksum FROM 1 FOR 49) AS "checksum!", success FROM public._sqlx_migrations ORDER BY version LIMIT $1"#,
        limit
    )
    .fetch_all(pool)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|e| e.code().as_deref() == Some("42P01"))
        {
            SchemaCompatibilityError::MissingLedger
        } else {
            SchemaCompatibilityError::Database(error)
        }
    })?;

    for row in &applied {
        if !row.success {
            return Err(SchemaCompatibilityError::FailedMigration {
                version: row.version,
            });
        }
        let expected = MIGRATOR
            .iter()
            .find(|migration| migration.version == row.version)
            .ok_or(SchemaCompatibilityError::UnknownMigration {
                version: row.version,
            })?;
        if expected.checksum.as_ref() != row.checksum {
            return Err(SchemaCompatibilityError::ChecksumMismatch {
                version: row.version,
            });
        }
    }
    for expected in MIGRATOR.iter() {
        if !applied.iter().any(|row| row.version == expected.version) {
            return Err(SchemaCompatibilityError::MissingMigration {
                version: expected.version,
            });
        }
    }
    Ok(())
}

/// A bounded, non-secret reason this binary cannot serve the current schema.
pub enum SchemaCompatibilityError {
    /// No migration ledger exists; run the explicit migration command first.
    MissingLedger,
    /// An embedded migration has not been applied.
    MissingMigration {
        /// Embedded positive migration version.
        version: i64,
    },
    /// The ledger contains a version not embedded in this binary.
    UnknownMigration {
        /// Unexpected applied migration version.
        version: i64,
    },
    /// An applied migration's checksum differs from the immutable embedded SQL.
    ChecksumMismatch {
        /// Modified migration version.
        version: i64,
    },
    /// The ledger records an unsuccessful migration.
    FailedMigration {
        /// Failed migration version.
        version: i64,
    },
    /// Connection, permission or ledger-query failure; never render its details.
    Database(sqlx::Error),
}

impl fmt::Display for SchemaCompatibilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingLedger => f.write_str("migration ledger is missing"),
            Self::MissingMigration { version } => {
                write!(f, "required migration {version} is missing")
            }
            Self::UnknownMigration { version } => {
                write!(f, "migration {version} is unknown to this binary")
            }
            Self::ChecksumMismatch { version } => write!(f, "migration {version} checksum differs"),
            Self::FailedMigration { version } => write!(f, "migration {version} is unsuccessful"),
            Self::Database(_) => f.write_str("migration history could not be read"),
        }
    }
}

impl fmt::Debug for SchemaCompatibilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for SchemaCompatibilityError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            Self::MissingLedger
            | Self::MissingMigration { .. }
            | Self::UnknownMigration { .. }
            | Self::ChecksumMismatch { .. }
            | Self::FailedMigration { .. } => None,
        }
    }
}
