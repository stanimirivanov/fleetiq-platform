//! Bounded interpretation history for one durable decoded observation.

use std::{error::Error, fmt};

use fleetiq_semantics::AssetPropertyChangeId;
use fleetiq_telemetry::DecodedObservationId;

use crate::StoredPropertyChange;

/// An exclusive-keyset request for one observation's immutable interpretations.
///
/// Pages sort by binding ID, binding revision, then event-time basis. A cursor
/// belongs to the exact observation in the request. Because new interpretations
/// can append between calls, pages are not a repeatable historical snapshot;
/// callers can restart from the beginning to discover newly appended facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationInterpretationPageQuery {
    observation_id: DecodedObservationId,
    limit: u8,
    after: Option<AssetPropertyChangeId>,
}

impl ObservationInterpretationPageQuery {
    /// Maximum interpretations returned in one page.
    pub const MAX_LIMIT: u8 = 100;

    /// Creates a page request with an optional exclusive cursor.
    ///
    /// # Errors
    ///
    /// Rejects a zero or excessive limit, or a cursor from another decoded
    /// observation. Tenant scope is supplied separately by the caller.
    pub fn new(
        observation_id: DecodedObservationId,
        limit: u8,
        after: Option<AssetPropertyChangeId>,
    ) -> Result<Self, InvalidObservationInterpretationPageQuery> {
        if limit == 0 || limit > Self::MAX_LIMIT {
            return Err(InvalidObservationInterpretationPageQuery::Limit);
        }
        if after
            .as_ref()
            .is_some_and(|cursor| cursor.observation_id() != &observation_id)
        {
            return Err(InvalidObservationInterpretationPageQuery::CursorObservation);
        }
        Ok(Self {
            observation_id,
            limit,
            after,
        })
    }

    /// Returns the exact decoded output whose interpretations are requested.
    pub const fn observation_id(&self) -> &DecodedObservationId {
        &self.observation_id
    }

    /// Returns the maximum entries to expose.
    pub const fn limit(&self) -> u8 {
        self.limit
    }

    /// Returns the exclusive interpretation cursor, if present.
    pub const fn after(&self) -> Option<&AssetPropertyChangeId> {
        self.after.as_ref()
    }
}

/// Invalid bounded-page limit or a cursor from another observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidObservationInterpretationPageQuery {
    /// The limit is outside 1 through 100.
    Limit,
    /// The cursor names a different decoded observation.
    CursorObservation,
}

impl fmt::Display for InvalidObservationInterpretationPageQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit => formatter.write_str("interpretation page limit must be 1 through 100"),
            Self::CursorObservation => {
                formatter.write_str("interpretation cursor has another source")
            }
        }
    }
}

impl Error for InvalidObservationInterpretationPageQuery {}

/// One ordered page of complete immutable interpretations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservationInterpretationPage {
    changes: Vec<StoredPropertyChange>,
    next_after: Option<AssetPropertyChangeId>,
}

impl ObservationInterpretationPage {
    /// Assembles a page from repository-validated interpretations.
    pub fn new(
        changes: Vec<StoredPropertyChange>,
        next_after: Option<AssetPropertyChangeId>,
    ) -> Self {
        Self {
            changes,
            next_after,
        }
    }

    /// Returns changes sorted by binding identity, revision, and time basis.
    pub fn changes(&self) -> &[StoredPropertyChange] {
        &self.changes
    }

    /// Returns the exclusive cursor for the next page, when more rows exist.
    pub const fn next_after(&self) -> Option<&AssetPropertyChangeId> {
        self.next_after.as_ref()
    }
}

#[cfg(test)]
mod tests;
