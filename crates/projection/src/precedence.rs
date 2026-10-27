//! Policy v1: event time followed by a complete immutable identity tie-break.
use fleetiq_history::StoredPropertyChange;
use fleetiq_semantics::EventTimeBasis;
use std::cmp::Ordering;
#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct Key<'a> {
    event_at: i64,
    message: &'a str,
    package: &'a str,
    package_version: &'a str,
    output: u32,
    binding: &'a str,
    binding_revision: u64,
    basis: u8,
}
impl<'a> From<&'a StoredPropertyChange> for Key<'a> {
    fn from(change: &'a StoredPropertyChange) -> Self {
        let id = change.id();
        let source = id.observation_id();
        Self {
            event_at: change.event_at().get(),
            message: source.message_id().as_str(),
            package: source.decoded_by().id().as_str(),
            package_version: source.decoded_by().version().as_str(),
            output: source.output_index(),
            binding: id.binding_id().as_str(),
            binding_revision: id.binding_revision().get(),
            basis: match id.event_time_basis() {
                EventTimeBasis::DeviceRecorded => 0,
                EventTimeBasis::ReceivedAt => 1,
            },
        }
    }
}
/// Compares eligible interpretations of the same tenant/exact target.
///
/// Greater wins. Event time precedes message, package ID/version, output index,
/// binding ID/revision and time basis (`device_recorded` before `received_at`).
/// Strings compare as UTF-8 bytes, mirrored by PostgreSQL C collation. This is
/// deterministic arbitration, not a claim that lexical package IDs or arbitrary
/// source clocks are semantically better. Arrival time and quality never break
/// ties. Callers must establish equal tenant/target scope before comparing.
pub fn compare_precedence(left: &StoredPropertyChange, right: &StoredPropertyChange) -> Ordering {
    Key::from(left).cmp(&Key::from(right))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn key(event_at: i64, message: &str, revision: u64) -> Key<'_> {
        Key {
            event_at,
            message,
            package: "p",
            package_version: "1",
            output: 0,
            binding: "b",
            binding_revision: revision,
            basis: 0,
        }
    }
    #[test]
    fn event_time_dominates_identity_and_revisions_are_numeric() {
        assert!(key(2, "a", 1) > key(1, "z", 99));
        assert!(key(2, "z", 10) > key(2, "z", 9));
        assert!(key(2, "z", 1) > key(2, "a", 99));
    }
    #[test]
    fn exact_retry_is_equal_and_tie_break_is_order_independent() {
        assert_eq!(key(2, "a", 1).cmp(&key(2, "a", 1)), Ordering::Equal);
        assert_eq!(
            key(2, "a", 1).cmp(&key(2, "z", 1)),
            key(2, "z", 1).cmp(&key(2, "a", 1)).reverse()
        );
    }
}
