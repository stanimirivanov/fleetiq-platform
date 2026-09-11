use super::*;

#[test]
fn snapshot_establishes_the_consumer_revision() {
    let snapshot = ProjectionSnapshot::new(revision(4), vec!["asset-a", "asset-b"]);
    let cursor = RevisionCursor::from_snapshot(&snapshot);

    assert_eq!(snapshot.revision(), revision(4));
    assert_eq!(snapshot.items(), ["asset-a", "asset-b"]);
    assert_eq!(cursor.current(), revision(4));
}

#[test]
fn cursor_accepts_a_delta_based_on_its_current_revision() {
    let mut cursor = RevisionCursor::new(revision(4));
    let delta = ProjectionDelta::new(revision(4), revision(7), vec!["change-a"])
        .expect("a coalesced forward delta should be valid");

    cursor
        .advance(&delta)
        .expect("delta base should match the cursor");

    assert_eq!(cursor.current(), revision(7));
    assert_eq!(delta.changes(), ["change-a"]);
}

#[test]
fn cursor_rejects_a_gap_without_changing_its_revision() {
    let mut cursor = RevisionCursor::new(revision(4));
    let delta = ProjectionDelta::new(revision(5), revision(6), vec!["change-a"])
        .expect("forward delta should be structurally valid");

    let result = cursor.advance(&delta);

    assert_eq!(
        result,
        Err(RevisionCursorError::RevisionMismatch {
            current: revision(4),
            delta_base: revision(5),
        })
    );
    assert_eq!(cursor.current(), revision(4));
}

#[test]
fn delta_must_advance_the_projection_revision() {
    assert_eq!(
        ProjectionDelta::<()>::new(revision(5), revision(5), Vec::new()),
        Err(ProjectionDeltaError::NonAdvancingRevision {
            base: revision(5),
            revision: revision(5),
        })
    );
    assert_eq!(
        ProjectionDelta::<()>::new(revision(5), revision(3), Vec::new()),
        Err(ProjectionDeltaError::NonAdvancingRevision {
            base: revision(5),
            revision: revision(3),
        })
    );
}

#[test]
fn revision_increment_is_checked_at_the_numeric_limit() {
    assert_eq!(
        ProjectionRevision::initial().checked_next(),
        Ok(revision(1))
    );
    assert_eq!(
        ProjectionRevision::new(u64::MAX).checked_next(),
        Err(RevisionOverflowError)
    );
}

#[test]
fn live_message_can_require_resnapshot_after_authorization_changes() {
    let instruction = ResnapshotRequired::new(ResnapshotReason::AuthorizationChanged, revision(12));
    let message: LiveMessage<&str> = LiveMessage::ResnapshotRequired(instruction);

    let LiveMessage::ResnapshotRequired(actual) = message else {
        panic!("expected a resnapshot instruction");
    };
    assert_eq!(actual.reason(), ResnapshotReason::AuthorizationChanged);
    assert_eq!(actual.available_revision(), revision(12));
}

const fn revision(value: u64) -> ProjectionRevision {
    ProjectionRevision::new(value)
}
