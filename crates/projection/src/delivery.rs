//! Latest-state continuity policy; transport delivery does not replay history.
/// Last complete exact-property state installed by a consumer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryPosition {
    /// Durable tenant rebuild generation.
    pub generation: u64,
    /// Durable tenant commit revision; not an event timestamp.
    pub revision: u64,
}
/// Maximum coalesced tenant revision distance accepted for a reconnect.
pub const RESUME_REVISION_WINDOW: u64 = 256;
/// Observable synchronization decision for one fresh consistent database view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryDecision {
    /// No baseline was supplied: send the complete snapshot.
    Snapshot,
    /// Replace the complete property state; intermediate interpretations may coalesce.
    Replace,
    /// No committed revision changed; freshness/pending may still be sampled.
    Unchanged,
    /// A rebuild replaced the baseline.
    ProjectionReset,
    /// A future cursor or regressing server revision cannot establish continuity.
    RevisionMismatch,
    /// The bounded reconnect window no longer covers the consumer's position.
    ResumeWindowExceeded,
}
/// Classifies continuity without changing the caller's last applied position.
///
/// A forward jump inside the window is safe only because delivery replaces the
/// complete exact-property state. It is not permission to skip historical events.
#[must_use]
pub const fn delivery_decision(
    previous: Option<DeliveryPosition>,
    current: DeliveryPosition,
) -> DeliveryDecision {
    let Some(previous) = previous else {
        return DeliveryDecision::Snapshot;
    };
    if previous.generation != current.generation {
        DeliveryDecision::ProjectionReset
    } else if previous.revision > current.revision {
        DeliveryDecision::RevisionMismatch
    } else if current.revision - previous.revision > RESUME_REVISION_WINDOW {
        DeliveryDecision::ResumeWindowExceeded
    } else if previous.revision == current.revision {
        DeliveryDecision::Unchanged
    } else {
        DeliveryDecision::Replace
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuity_covers_initial_duplicate_coalesced_reset_future_and_expired_positions() {
        let baseline = DeliveryPosition {
            generation: 2,
            revision: 10,
        };
        assert_eq!(
            delivery_decision(None, baseline),
            DeliveryDecision::Snapshot
        );
        assert_eq!(
            delivery_decision(Some(baseline), baseline),
            DeliveryDecision::Unchanged
        );
        for (generation, revision, expected) in [
            (2, 11, DeliveryDecision::Replace),
            (2, 266, DeliveryDecision::Replace),
            (2, 267, DeliveryDecision::ResumeWindowExceeded),
            (2, 9, DeliveryDecision::RevisionMismatch),
            (3, 11, DeliveryDecision::ProjectionReset),
        ] {
            assert_eq!(
                delivery_decision(
                    Some(baseline),
                    DeliveryPosition {
                        generation,
                        revision
                    }
                ),
                expected
            );
        }
        let maximum = DeliveryPosition {
            generation: u64::MAX,
            revision: u64::MAX,
        };
        assert_eq!(
            delivery_decision(Some(maximum), maximum),
            DeliveryDecision::Unchanged
        );
    }
}
