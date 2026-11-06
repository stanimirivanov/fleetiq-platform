use super::retention::{RetentionCandidate, RetentionConsumer};
#[test]
fn only_old_verified_unreferenced_payload_is_a_candidate() {
    let assess = |recorded, days, verified, consumers| {
        RetentionCandidate::assess(
            "sample".into(),
            Some(recorded),
            864_000_000,
            days,
            verified,
            consumers,
        )
    };
    let eligible = assess(0, Some(7), true, vec![]);
    assert!(eligible.payload_eligible());
    assert_eq!(eligible.estimated_reclaimable_bytes(), 24);
    assert!(assess(259_200_000, Some(7), true, vec![]).payload_eligible());
    assert!(!assess(259_200_001, Some(7), true, vec![]).payload_eligible());
    assert!(
        !RetentionCandidate::assess("sample".into(), None, 864_000_000, Some(7), true, vec![])
            .payload_eligible()
    );
    assert!(!assess(864_000_000, Some(7), true, vec![]).payload_eligible());
    assert!(!assess(0, None, true, vec![]).payload_eligible());
    assert!(!assess(0, Some(7), false, vec![]).payload_eligible());
    for consumer in [
        RetentionConsumer::Replay,
        RetentionConsumer::Semantic,
        RetentionConsumer::Calculator,
        RetentionConsumer::Restore,
    ] {
        let protected = assess(0, Some(7), true, vec![consumer]);
        assert!(!protected.payload_eligible());
        assert_eq!(protected.estimated_reclaimable_bytes(), 0);
    }
    assert!(!assess(i64::MAX, Some(i32::MAX), true, vec![]).payload_eligible());
}
