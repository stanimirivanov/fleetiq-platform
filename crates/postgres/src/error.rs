use std::{error::Error, fmt};

/// Describes a failed PostgreSQL repository operation.
#[derive(Debug)]
pub enum PostgresRepositoryError {
    /// A uniqueness constraint rejected a conflicting value.
    Conflict {
        /// The database constraint that detected the conflict, when available.
        constraint: Option<String>,
        /// The original database error.
        source: sqlx::Error,
    },
    /// A foreign key rejected a reference to missing tenant or definition data.
    InvalidReference {
        /// The database constraint that detected the invalid reference.
        constraint: Option<String>,
        /// The original database error.
        source: sqlx::Error,
    },
    /// A database check or deferred constraint rejected an invalid invariant.
    InvariantViolation {
        /// The database constraint that detected the violation, when available.
        constraint: Option<String>,
        /// The original database error.
        source: sqlx::Error,
    },
    /// An immutable fact revision did not immediately follow its persisted head.
    UnexpectedRevision {
        /// The next revision required by the repository.
        expected: u64,
        /// The revision supplied by the caller.
        actual: u64,
    },
    /// The latest revision already occupies the complete `u64` range.
    RevisionExhausted,
    /// Recorded time moved backwards relative to the preceding revision.
    RecordedTimeRegression {
        /// The preceding revision's signed Unix timestamp in milliseconds.
        previous_ms: i64,
        /// The rejected revision's signed Unix timestamp in milliseconds.
        actual_ms: i64,
    },
    /// Persisted data violated a domain invariant during reconstruction.
    CorruptData {
        /// The column or record that could not be reconstructed.
        field: &'static str,
        /// The domain validation failure.
        message: String,
    },
    /// Another database operation failed.
    Database(sqlx::Error),
}

impl fmt::Display for PostgresRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { constraint, .. } => {
                write!(formatter, "database conflict")?;
                format_constraint(formatter, constraint.as_deref())
            }
            Self::InvalidReference { constraint, .. } => {
                write!(formatter, "database reference is invalid")?;
                format_constraint(formatter, constraint.as_deref())
            }
            Self::InvariantViolation { constraint, .. } => {
                write!(formatter, "database invariant was violated")?;
                format_constraint(formatter, constraint.as_deref())
            }
            Self::UnexpectedRevision { expected, actual } => {
                write!(
                    formatter,
                    "revision {actual} is out of sequence; expected {expected}"
                )
            }
            Self::RevisionExhausted => formatter.write_str("revision range is exhausted"),
            Self::RecordedTimeRegression {
                previous_ms,
                actual_ms,
            } => write!(
                formatter,
                "recorded time {actual_ms} precedes previous time {previous_ms}"
            ),
            Self::CorruptData { field, message } => {
                write!(
                    formatter,
                    "persisted value in {field} is invalid: {message}"
                )
            }
            Self::Database(error) => write!(formatter, "database operation failed: {error}"),
        }
    }
}

impl Error for PostgresRepositoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Conflict { source, .. }
            | Self::InvalidReference { source, .. }
            | Self::InvariantViolation { source, .. } => Some(source),
            Self::Database(error) => Some(error),
            Self::CorruptData { .. }
            | Self::UnexpectedRevision { .. }
            | Self::RevisionExhausted
            | Self::RecordedTimeRegression { .. } => None,
        }
    }
}

pub(crate) fn classify_database_error(error: sqlx::Error) -> PostgresRepositoryError {
    let Some(database_error) = error.as_database_error() else {
        return PostgresRepositoryError::Database(error);
    };
    let code = database_error.code();
    let constraint = database_error.constraint().map(ToOwned::to_owned);

    match code.as_deref() {
        Some("23505") => PostgresRepositoryError::Conflict {
            constraint,
            source: error,
        },
        Some("23503") => PostgresRepositoryError::InvalidReference {
            constraint,
            source: error,
        },
        Some("23514") => PostgresRepositoryError::InvariantViolation {
            constraint,
            source: error,
        },
        _ => PostgresRepositoryError::Database(error),
    }
}

pub(crate) fn corrupt(field: &'static str, error: impl fmt::Display) -> PostgresRepositoryError {
    PostgresRepositoryError::CorruptData {
        field,
        message: error.to_string(),
    }
}

fn format_constraint(formatter: &mut fmt::Formatter<'_>, constraint: Option<&str>) -> fmt::Result {
    if let Some(constraint) = constraint {
        write!(formatter, " ({constraint})")?;
    }
    Ok(())
}
