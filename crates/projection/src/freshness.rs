//! Evaluation-time freshness is independent of source quality and connectivity.
use fleetiq_domain::UnixTimeMillis;
/// Age classification at the snapshot's explicit evaluation instant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Freshness {
    /// Event timestamp is no more than thirty seconds old.
    Fresh,
    /// Event timestamp is more than thirty seconds old.
    Stale,
    /// Source event lies after evaluation; never relabel it as fresh.
    Future,
}
/// Classifies the selected event without trusting clock synchronization.
///
/// Thirty seconds is a UI coverage policy, not an asset-health threshold.
/// An overflowed age is necessarily stale; quality is never promoted here.
pub fn freshness(event_at: UnixTimeMillis, evaluated_at: UnixTimeMillis) -> Freshness {
    if event_at > evaluated_at {
        Freshness::Future
    } else if evaluated_at
        .get()
        .checked_sub(event_at.get())
        .is_some_and(|age| age <= 30_000)
    {
        Freshness::Fresh
    } else {
        Freshness::Stale
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn age_boundary_future_and_overflow_are_explicit() {
        for (event, now, expected) in [
            (0, 0, Freshness::Fresh),
            (0, 30_000, Freshness::Fresh),
            (0, 30_001, Freshness::Stale),
            (1, 0, Freshness::Future),
            (i64::MIN, i64::MAX, Freshness::Stale),
        ] {
            assert_eq!(
                freshness(UnixTimeMillis::new(event), UnixTimeMillis::new(now)),
                expected
            );
        }
    }
}
