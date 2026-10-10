//! Verifies append-only SQL history against an explicit, trusted Git baseline.
//!
//! The caller owns baseline selection. This sensor checks both the index and
//! working tree, never normalizes SQL bytes, and cannot replace schema execution
//! or an operator's deployment review.

mod git;
mod inventory;

#[cfg(test)]
mod tests;

use std::{error::Error, fmt, io, path::Path};

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const GUIDE: &str = "docs/development/sql-migrations.md";

#[derive(Debug)]
pub(crate) struct MigrationReport {
    immutable_count: usize,
    added_count: usize,
    base_commit: String,
}

impl MigrationReport {
    pub(crate) const fn immutable_count(&self) -> usize {
        self.immutable_count
    }

    pub(crate) const fn added_count(&self) -> usize {
        self.added_count
    }

    pub(crate) fn base_commit(&self) -> &str {
        &self.base_commit
    }
}

#[derive(Debug)]
pub(crate) enum MigrationCheckError {
    Io {
        operation: String,
        source: io::Error,
    },
    Git {
        operation: String,
        detail: String,
    },
    Policy(Vec<String>),
}

impl fmt::Display for MigrationCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(formatter, "{operation}: {source}")?,
            Self::Git { operation, detail } => write!(formatter, "{operation}: {detail}")?,
            Self::Policy(violations) => {
                for (index, violation) in violations.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str("\n")?;
                    }
                    write!(formatter, "- {violation}")?;
                }
            }
        }
        write!(formatter, "\nSee {GUIDE}")
    }
}

impl Error for MigrationCheckError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Git { .. } | Self::Policy(_) => None,
        }
    }
}

impl MigrationCheckError {
    fn io(operation: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            operation: operation.into(),
            source,
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::Policy(vec![message.into()])
    }
}

/// Checks published migration bytes and append-only version ordering.
///
/// `base` must be an explicitly selected commit or ref supplied by a trusted
/// workflow or reviewer. It must resolve locally and be an ancestor of `HEAD`;
/// unavailable history fails rather than treating the published chain as empty.
/// New files are checked even before staging. Missing files, content changes,
/// unsafe file kinds, and invalid or reused versions are policy violations.
///
/// # Errors
///
/// Returns an operational error when Git or filesystem evidence is unavailable,
/// or a policy error with corrective guidance when immutability is violated.
pub(crate) fn check(base: &str) -> Result<MigrationReport, MigrationCheckError> {
    check_at(Path::new(WORKSPACE_ROOT), base)
}

fn check_at(root: &Path, base: &str) -> Result<MigrationReport, MigrationCheckError> {
    if base.trim().is_empty() {
        return Err(MigrationCheckError::invalid(
            "provide an explicit trusted baseline with --base <commit-or-ref>; no default or empty baseline is permitted",
        ));
    }
    let commit = git::resolve_base(root, base)?;
    let published = git::published_entries(root, &commit)?;
    if published.is_empty() {
        return Err(MigrationCheckError::invalid(
            "the baseline has no published migrations; select a trusted commit containing the established migration chain",
        ));
    }
    let index = git::index_entries(root)?;
    let blobs = git::read_blobs(root, &published)?;
    let added_count = inventory::verify(root, &published, &index, &blobs)?;
    Ok(MigrationReport {
        immutable_count: published.len(),
        added_count,
        base_commit: commit,
    })
}
