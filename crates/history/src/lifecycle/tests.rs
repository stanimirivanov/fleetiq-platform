use super::*;
#[test]
fn policy_requires_explicit_consistent_limits() {
    assert!(LifecyclePolicy::new(0, 20, 1, 1).is_err());
    assert!(LifecyclePolicy::new(20, 10, 1, 1).is_err());
    assert!(LifecyclePolicy::new(10, 20, 0, 1).is_err());
    assert!(LifecyclePolicy::new(10, 20, 1, 0).is_err());
    let p = LifecyclePolicy::new(10, 20, 2, 1024).unwrap();
    assert_eq!(p.hot_days(), 10);
    assert_eq!(p.archive_days(), 20);
    assert_eq!(p.max_jobs(), 2);
    assert_eq!(p.max_bytes(), 1024);
}
#[test]
fn missing_policy_never_enables_lifecycle_work() {
    assert_eq!(LifecycleSettings::default(), LifecycleSettings::Disabled);
}
#[test]
fn protected_evidence_cannot_be_selected_as_payload() {
    for protection in [
        EvidenceProtection::Lineage,
        EvidenceProtection::Idempotency,
        EvidenceProtection::ActiveConsumer,
        EvidenceProtection::UnverifiedArchive,
    ] {
        assert!(!protection.permits_reclamation());
    }
    assert!(EvidenceProtection::VerifiedUnreferencedPayload.permits_reclamation());
}
