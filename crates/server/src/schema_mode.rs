use fleetiq_postgres::{SchemaCompatibilityError, apply_migrations, check_schema};
use sqlx::PgPool;

/// Selects whether a process may migrate or must only validate an existing schema.
///
/// Production serving and bootstrap use Check with a non-DDL role. Migrate is
/// retained solely for backwards-compatible development startup. Neither mode
/// provides writer fencing, release approval or automatic database rollback.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SchemaMode {
    /// Apply the complete embedded chain with `SQLx` locking before starting work.
    #[default]
    Migrate,
    /// Read the exact version/checksum ledger without executing DDL.
    Check,
}

impl SchemaMode {
    pub(crate) async fn prepare(self, pool: &PgPool) -> Result<(), SchemaPreparationError> {
        match self {
            Self::Migrate => apply_migrations(pool)
                .await
                .map_err(SchemaPreparationError::Migration)?,
            Self::Check => {}
        }
        check_schema(pool)
            .await
            .map_err(SchemaPreparationError::Compatibility)
    }
}

/// Failure to prepare or validate the schema; diagnostics never expose SQL.
pub enum SchemaPreparationError {
    /// `SQLx` could not apply the complete embedded chain.
    Migration(sqlx::migrate::MigrateError),
    /// The database does not match this binary's exact embedded inventory.
    Compatibility(SchemaCompatibilityError),
}

impl std::fmt::Display for SchemaPreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Migration(_) => f.write_str("embedded database migration failed"),
            Self::Compatibility(error) => error.fmt(f),
        }
    }
}
impl std::fmt::Debug for SchemaPreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for SchemaPreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Migration(e) => Some(e),
            Self::Compatibility(e) => Some(e),
        }
    }
}
