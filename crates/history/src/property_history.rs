//! Bounded hot-history queries; ordering never implies latest-state precedence.

mod service;
#[cfg(test)]
mod tests;
pub use service::{PropertyHistoryService, PropertyHistoryView};

use crate::StoredPropertyChange;
use fleetiq_domain::{SignalTarget, UnixTimeMillis};
use fleetiq_semantics::AssetPropertyChangeId;
use std::{error::Error, fmt};

/// Exclusive event-time/full-interpretation key, not an arrival watermark.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyHistoryCursor {
    event_at: UnixTimeMillis,
    id: AssetPropertyChangeId,
}
impl PropertyHistoryCursor {
    /// Creates a deterministic tie-breaking key from durable evidence.
    pub const fn new(event_at: UnixTimeMillis, id: AssetPropertyChangeId) -> Self {
        Self { event_at, id }
    }
    /// Returns the selected event time.
    pub const fn event_at(&self) -> UnixTimeMillis {
        self.event_at
    }
    /// Returns the complete immutable interpretation identity.
    pub const fn id(&self) -> &AssetPropertyChangeId {
        &self.id
    }
}

/// One exact asset/property version in a mandatory half-open event-time window.
///
/// Ordering is ascending event time, then message, package, version, output,
/// binding, revision and time basis (database text order). Pages are not a
/// snapshot: late appends behind a cursor require restarting the window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyHistoryQuery {
    target: SignalTarget,
    start: UnixTimeMillis,
    end: UnixTimeMillis,
    limit: u8,
    after: Option<PropertyHistoryCursor>,
}
impl PropertyHistoryQuery {
    /// Maximum rows returned, excluding the private look-ahead row.
    pub const MAX_LIMIT: u8 = 100;
    /// Maximum event-time span: seven days in milliseconds.
    pub const MAX_SPAN_MS: i64 = 604_800_000;
    /// Validates a bounded window and exclusive cursor before any storage call.
    ///
    /// # Errors
    /// Rejects invalid limits, empty/reversed/oversized windows (including
    /// subtraction overflow), or cursors outside the requested window.
    pub fn new(
        target: SignalTarget,
        start: UnixTimeMillis,
        end: UnixTimeMillis,
        limit: u8,
        after: Option<PropertyHistoryCursor>,
    ) -> Result<Self, InvalidPropertyHistoryQuery> {
        if limit == 0 || limit > Self::MAX_LIMIT {
            return Err(InvalidPropertyHistoryQuery);
        }
        let span = end
            .get()
            .checked_sub(start.get())
            .ok_or(InvalidPropertyHistoryQuery)?;
        if !(1..=Self::MAX_SPAN_MS).contains(&span)
            || after
                .as_ref()
                .is_some_and(|c| c.event_at() < start || c.event_at() >= end)
        {
            return Err(InvalidPropertyHistoryQuery);
        }
        Ok(Self {
            target,
            start,
            end,
            limit,
            after,
        })
    }
    /// Returns exact asset/property scope.
    pub const fn target(&self) -> &SignalTarget {
        &self.target
    }
    /// Returns inclusive event-time lower bound.
    pub const fn start(&self) -> UnixTimeMillis {
        self.start
    }
    /// Returns exclusive event-time upper bound.
    pub const fn end(&self) -> UnixTimeMillis {
        self.end
    }
    /// Returns the bounded result count.
    pub const fn limit(&self) -> u8 {
        self.limit
    }
    /// Returns the exclusive full-identity key.
    pub const fn after(&self) -> Option<&PropertyHistoryCursor> {
        self.after.as_ref()
    }
}
/// Invalid window, cursor time or page size; contains no request data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidPropertyHistoryQuery;
impl fmt::Display for InvalidPropertyHistoryQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid bounded property history query")
    }
}
impl Error for InvalidPropertyHistoryQuery {}

/// Bounded immutable interpretations and an exclusive continuation key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyHistoryPage {
    changes: Vec<StoredPropertyChange>,
    next_after: Option<PropertyHistoryCursor>,
}
impl PropertyHistoryPage {
    /// Assembles repository-validated rows in the query's deterministic order.
    pub fn new(
        changes: Vec<StoredPropertyChange>,
        next_after: Option<PropertyHistoryCursor>,
    ) -> Self {
        Self {
            changes,
            next_after,
        }
    }
    /// Returns immutable values and original provenance without reinterpretation.
    pub fn changes(&self) -> &[StoredPropertyChange] {
        &self.changes
    }
    /// Returns a continuation only when a look-ahead row exists.
    pub const fn next_after(&self) -> Option<&PropertyHistoryCursor> {
        self.next_after.as_ref()
    }
}
