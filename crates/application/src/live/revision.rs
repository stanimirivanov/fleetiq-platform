use std::{error::Error, fmt};

use super::ProjectionDelta;

/// Monotonic revision within one authorized projection stream.
///
/// Revisions are scoped to a projection, such as one tenant and query. They are
/// continuity tokens rather than globally comparable domain revisions. Zero is
/// a valid initial revision for a projection with no committed changes.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProjectionRevision(u64);

impl ProjectionRevision {
    /// Returns the initial projection revision.
    pub const fn initial() -> Self {
        Self(0)
    }

    /// Constructs a revision from its transport-independent numeric value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the numeric revision value.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next revision without wrapping at the numeric limit.
    ///
    /// # Errors
    ///
    /// Returns [`RevisionOverflowError`] when this revision is `u64::MAX`.
    pub fn checked_next(self) -> Result<Self, RevisionOverflowError> {
        self.0.checked_add(1).map(Self).ok_or(RevisionOverflowError)
    }
}

/// Failure to advance beyond the largest representable projection revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RevisionOverflowError;

impl fmt::Display for RevisionOverflowError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("projection revision cannot advance beyond u64::MAX")
    }
}

impl Error for RevisionOverflowError {}

/// Tracks the last projection revision applied by a live consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RevisionCursor {
    current: ProjectionRevision,
}

impl RevisionCursor {
    /// Starts a cursor at the revision supplied by a full snapshot.
    pub const fn from_snapshot<T>(snapshot: &super::ProjectionSnapshot<T>) -> Self {
        Self {
            current: snapshot.revision(),
        }
    }

    /// Constructs a cursor from a previously retained revision.
    pub const fn new(current: ProjectionRevision) -> Self {
        Self { current }
    }

    /// Returns the last revision applied by the consumer.
    pub const fn current(self) -> ProjectionRevision {
        self.current
    }

    /// Validates and records a delta transition.
    ///
    /// # Errors
    ///
    /// Returns [`RevisionCursorError::RevisionMismatch`] when the delta is based
    /// on a revision other than the cursor's current revision. The cursor is not
    /// changed when validation fails. Call this only after applying the delta's
    /// changes successfully so local state and its cursor advance together.
    pub fn advance<T>(&mut self, delta: &ProjectionDelta<T>) -> Result<(), RevisionCursorError> {
        if delta.base_revision() != self.current {
            return Err(RevisionCursorError::RevisionMismatch {
                current: self.current,
                delta_base: delta.base_revision(),
            });
        }

        self.current = delta.revision();
        Ok(())
    }
}

/// Describes why a live consumer cannot apply the next delta safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevisionCursorError {
    /// The delta does not begin at the consumer's current revision.
    RevisionMismatch {
        /// Last revision successfully applied by the consumer.
        current: ProjectionRevision,
        /// Base revision required by the rejected delta.
        delta_base: ProjectionRevision,
    },
}

impl fmt::Display for RevisionCursorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RevisionMismatch {
                current,
                delta_base,
            } => write!(
                formatter,
                "projection cursor is at revision {}; delta requires revision {}",
                current.get(),
                delta_base.get()
            ),
        }
    }
}

impl Error for RevisionCursorError {}
