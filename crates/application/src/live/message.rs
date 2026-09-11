use std::{error::Error, fmt};

use super::ProjectionRevision;

/// A complete authorized projection at one committed revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionSnapshot<T> {
    revision: ProjectionRevision,
    items: Vec<T>,
}

impl<T> ProjectionSnapshot<T> {
    /// Constructs a complete projection snapshot.
    pub fn new(revision: ProjectionRevision, items: Vec<T>) -> Self {
        Self { revision, items }
    }

    /// Returns the revision represented by every item in this snapshot.
    pub const fn revision(&self) -> ProjectionRevision {
        self.revision
    }

    /// Returns the complete authorized projection contents.
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// Consumes the snapshot and returns its contents.
    pub fn into_items(self) -> Vec<T> {
        self.items
    }
}

/// A set of projection changes transforming one revision into a later revision.
///
/// A delta may combine several committed revisions. Consumers can apply it only
/// when their cursor equals its base revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionDelta<T> {
    base_revision: ProjectionRevision,
    revision: ProjectionRevision,
    changes: Vec<T>,
}

impl<T> ProjectionDelta<T> {
    /// Constructs a forward-only projection delta.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectionDeltaError::NonAdvancingRevision`] when `revision`
    /// is less than or equal to `base_revision`.
    pub fn new(
        base_revision: ProjectionRevision,
        revision: ProjectionRevision,
        changes: Vec<T>,
    ) -> Result<Self, ProjectionDeltaError> {
        if revision <= base_revision {
            return Err(ProjectionDeltaError::NonAdvancingRevision {
                base: base_revision,
                revision,
            });
        }

        Ok(Self {
            base_revision,
            revision,
            changes,
        })
    }

    /// Returns the revision a consumer must already hold.
    pub const fn base_revision(&self) -> ProjectionRevision {
        self.base_revision
    }

    /// Returns the revision after this delta is applied.
    pub const fn revision(&self) -> ProjectionRevision {
        self.revision
    }

    /// Returns the ordered projection changes.
    pub fn changes(&self) -> &[T] {
        &self.changes
    }

    /// Consumes the delta and returns its ordered changes.
    pub fn into_changes(self) -> Vec<T> {
        self.changes
    }
}

/// Describes why a projection delta is structurally invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectionDeltaError {
    /// The resulting revision does not advance beyond the base revision.
    NonAdvancingRevision {
        /// Revision on which the delta is based.
        base: ProjectionRevision,
        /// Rejected resulting revision.
        revision: ProjectionRevision,
    },
}

impl fmt::Display for ProjectionDeltaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonAdvancingRevision { base, revision } => write!(
                formatter,
                "projection delta revision {} must be greater than base revision {}",
                revision.get(),
                base.get()
            ),
        }
    }
}

impl Error for ProjectionDeltaError {}

/// Explains why incremental delivery cannot continue safely.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ResnapshotReason {
    /// A bounded server or client queue could not retain every required delta.
    Lagged,
    /// The consumer's revision does not match the next available delta.
    RevisionMismatch,
    /// The consumer's authorization changed and its visible projection must be rebuilt.
    AuthorizationChanged,
    /// The server replaced the projection baseline or revision sequence.
    ProjectionReset,
}

/// Directs a consumer to discard local projection state and obtain a snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResnapshotRequired {
    reason: ResnapshotReason,
    available_revision: ProjectionRevision,
}

impl ResnapshotRequired {
    /// Constructs a resnapshot instruction at the latest available revision.
    pub const fn new(reason: ResnapshotReason, available_revision: ProjectionRevision) -> Self {
        Self {
            reason,
            available_revision,
        }
    }

    /// Returns why incremental delivery stopped.
    pub const fn reason(self) -> ResnapshotReason {
        self.reason
    }

    /// Returns the latest revision available when the instruction was created.
    pub const fn available_revision(self) -> ProjectionRevision {
        self.available_revision
    }
}

/// One transport-independent message after an initial projection snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveMessage<T> {
    /// Applies ordered changes to a consumer holding the delta's base revision.
    Delta(ProjectionDelta<T>),
    /// Stops incremental application until the consumer obtains a new snapshot.
    ResnapshotRequired(ResnapshotRequired),
}
