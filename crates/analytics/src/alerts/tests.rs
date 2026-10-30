use super::*;
fn time(n: i64) -> UnixTimeMillis {
    UnixTimeMillis::new(n)
}
fn step(s: AlertLifecycle, c: AlertCondition, start: i64, end: i64) -> AlertLifecycle {
    s.advance(c, time(start), time(end), 100, 100, false)
        .unwrap()
}
#[test]
fn pending_firing_and_resolution_use_explicit_event_evidence() {
    use AlertCondition::{Matched, NotMatched};
    let p = step(AlertLifecycle::empty(), Matched, 0, 100);
    assert_eq!(p.phase(), AlertPhase::Pending);
    let f = step(p, Matched, 100, 200);
    assert_eq!(f.phase(), AlertPhase::Firing);
    let clearing = step(f, NotMatched, 200, 300);
    assert_eq!(clearing.phase(), AlertPhase::Firing);
    let r = step(clearing, NotMatched, 300, 400);
    assert_eq!(r.phase(), AlertPhase::Resolved);
    assert_eq!(step(r, Matched, 400, 500).phase(), AlertPhase::Pending);
    let immediate = AlertLifecycle::empty()
        .advance(Matched, time(0), time(1), 0, 0, false)
        .unwrap();
    assert_eq!(immediate.phase(), AlertPhase::Firing);
    assert_eq!(
        immediate
            .advance(NotMatched, time(1), time(2), 0, 0, false)
            .unwrap()
            .phase(),
        AlertPhase::Resolved
    );
}
#[test]
fn unknown_gaps_and_reactivation_break_timers_without_resolving_firing() {
    use AlertCondition::{Matched, Missing, NotMatched};
    let p = step(AlertLifecycle::empty(), Matched, 0, 100);
    assert_eq!(step(p, Missing, 100, 200).phase(), AlertPhase::Inactive);
    assert_eq!(step(p, NotMatched, 100, 200).phase(), AlertPhase::Inactive);
    let gap = step(p, Matched, 200, 300);
    assert_eq!(gap.phase(), AlertPhase::Pending);
    assert_eq!(gap.candidate_since(), Some(time(300)));
    let resumed = p
        .advance(Matched, time(100), time(200), 100, 100, true)
        .unwrap();
    assert_eq!(resumed.phase(), AlertPhase::Pending);
    assert_eq!(resumed.candidate_since(), Some(time(200)));
    let f = step(p, Matched, 100, 200);
    let clearing = step(f, NotMatched, 200, 300);
    let unknown = step(clearing, Missing, 300, 400);
    assert_eq!(unknown.phase(), AlertPhase::Firing);
    assert_eq!(unknown.candidate_since(), None);
    assert_eq!(
        step(unknown, NotMatched, 400, 500).phase(),
        AlertPhase::Firing
    );
    assert_eq!(step(clearing, Matched, 300, 400).candidate_since(), None);
}
#[test]
fn late_equal_and_invalid_input_cannot_advance_and_time_arithmetic_does_not_wrap() {
    use AlertCondition::Matched;
    let p = step(AlertLifecycle::empty(), Matched, 0, 100);
    assert!(p.advance(Matched, time(0), time(99), 0, 0, false).is_none());
    assert!(
        p.advance(Matched, time(0), time(100), 0, 0, false)
            .is_none()
    );
    assert!(
        p.advance(Matched, time(200), time(200), 0, 0, false)
            .is_none()
    );
    assert!(
        p.advance(Matched, time(100), time(200), 604_800_001, 0, false)
            .is_none()
    );
    assert!(AlertLifecycle::restore(AlertPhase::Pending, None, None).is_none());
    assert!(AlertLifecycle::restore(AlertPhase::Inactive, Some(time(1)), Some(time(2))).is_none());
    let old = AlertLifecycle::restore(
        AlertPhase::Pending,
        Some(time(i64::MIN)),
        Some(time(i64::MAX - 1)),
    )
    .unwrap();
    assert_eq!(
        old.advance(Matched, time(i64::MAX - 1), time(i64::MAX), 100, 100, false)
            .unwrap()
            .phase(),
        AlertPhase::Firing
    );
}
