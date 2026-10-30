//! Evidence-driven alert lifecycle. No wall clock, storage or notification I/O.
use crate::AlertCondition;
use fleetiq_domain::UnixTimeMillis;

/// Durable externally meaningful lifecycle phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertPhase {
    /// No outstanding matched condition.
    Inactive,
    /// Matched evidence has not yet satisfied the hold duration.
    Pending,
    /// The hold duration was satisfied; unknown evidence cannot resolve it.
    Firing,
    /// Explicit false evidence satisfied the resolution duration.
    Resolved,
}
impl AlertPhase {
    /// Stable persistence vocabulary.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Pending => "pending",
            Self::Firing => "firing",
            Self::Resolved => "resolved",
        }
    }
}
/// Checkpoint of one tenant/scope/pinned-rule-revision lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlertLifecycle {
    phase: AlertPhase,
    candidate_since: Option<UnixTimeMillis>,
    last_event: Option<UnixTimeMillis>,
}
impl AlertLifecycle {
    /// Starts without evidence or an outstanding condition.
    pub const fn empty() -> Self {
        Self {
            phase: AlertPhase::Inactive,
            candidate_since: None,
            last_event: None,
        }
    }
    /// Reconstructs a checkpoint, rejecting impossible duration candidates.
    pub fn restore(
        phase: AlertPhase,
        since: Option<UnixTimeMillis>,
        last: Option<UnixTimeMillis>,
    ) -> Option<Self> {
        if (phase == AlertPhase::Pending && since.is_none())
            || (matches!(phase, AlertPhase::Inactive | AlertPhase::Resolved) && since.is_some())
            || since.is_some_and(|s| last.is_none_or(|e| s > e))
            || (last.is_none() && phase != AlertPhase::Inactive)
        {
            return None;
        }
        Some(Self {
            phase,
            candidate_since: since,
            last_event: last,
        })
    }
    /// Current retained phase.
    pub const fn phase(self) -> AlertPhase {
        self.phase
    }
    /// First matched/false observation end in the current duration candidate.
    pub const fn candidate_since(self) -> Option<UnixTimeMillis> {
        self.candidate_since
    }
    /// Latest accepted window end; equal/older ends cannot advance timers.
    pub const fn last_event(self) -> Option<UnixTimeMillis> {
        self.last_event
    }
    /// Consumes one usable window boundary or explicit unknown classification.
    /// `resumed` breaks duration continuity after an activation generation change.
    /// Returns none for invalid windows/bounds or out-of-order/equal-time input.
    /// Gaps and missing evidence break timers, but never resolve firing state.
    #[must_use]
    pub fn advance(
        self,
        condition: AlertCondition,
        start: UnixTimeMillis,
        end: UnixTimeMillis,
        hold_ms: u32,
        resolve_ms: u32,
        resumed: bool,
    ) -> Option<Self> {
        let span = i128::from(end.get()) - i128::from(start.get());
        if !(1..=604_800_000).contains(&span)
            || hold_ms > 604_800_000
            || resolve_ms > 604_800_000
            || self.last_event.is_some_and(|last| end <= last)
        {
            return None;
        }
        let mut next = self;
        if resumed || self.last_event.is_some_and(|last| start > last) {
            next.candidate_since = None;
            if next.phase == AlertPhase::Pending {
                next.phase = AlertPhase::Inactive;
            }
        }
        let since = next.candidate_since.unwrap_or(end);
        let elapsed = i128::from(end.get()) - i128::from(since.get());
        match (next.phase, condition) {
            (AlertPhase::Firing, AlertCondition::NotMatched) => {
                if elapsed >= i128::from(resolve_ms) {
                    next.phase = AlertPhase::Resolved;
                    next.candidate_since = None;
                } else {
                    next.candidate_since = Some(since);
                }
            }
            (AlertPhase::Firing, _) => next.candidate_since = None,
            (_, AlertCondition::Matched) => {
                if elapsed >= i128::from(hold_ms) {
                    next.phase = AlertPhase::Firing;
                    next.candidate_since = None;
                } else {
                    next.phase = AlertPhase::Pending;
                    next.candidate_since = Some(since);
                }
            }
            (AlertPhase::Pending, _) => {
                next.phase = AlertPhase::Inactive;
                next.candidate_since = None;
            }
            _ => {}
        }
        next.last_event = Some(end);
        Some(next)
    }
}
#[cfg(test)]
mod tests;
