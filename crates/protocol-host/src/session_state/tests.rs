use std::{convert::Infallible, error::Error, fmt, num::NonZeroUsize};

use fleetiq_protocol_sdk::SessionStateAction;

use super::{SessionState, SessionStateUpdateError};

fn limit(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test limits are nonzero")
}

fn replace(bytes: &[u8]) -> SessionStateAction {
    SessionStateAction::Replace(bytes.to_vec().into_boxed_slice())
}

#[test]
fn package_state_is_available_to_later_invocations_in_the_same_session() {
    let mut state = SessionState::new(limit(32));

    state
        .update(|current| {
            assert_eq!(current.len(), 0);
            Ok::<_, Infallible>(replace(b"awaiting-auth"))
        })
        .expect("initial state fits");

    state
        .update(|current| {
            assert_eq!(current, b"awaiting-auth");
            Ok::<_, Infallible>(replace(b"authenticated"))
        })
        .expect("transition state fits");

    assert_eq!(state.as_bytes(), b"authenticated");
}

#[test]
fn keep_preserves_the_current_state() {
    let mut state = SessionState::new(limit(16));
    state
        .update(|_| Ok::<_, Infallible>(replace(b"identified")))
        .expect("initial state fits");

    state
        .update(|current| {
            assert_eq!(current, b"identified");
            Ok::<_, Infallible>(SessionStateAction::Keep)
        })
        .expect("keeping state is valid");

    assert_eq!(state.as_bytes(), b"identified");
}

#[test]
fn an_empty_replacement_clears_the_state() {
    let mut state = SessionState::new(limit(16));
    state
        .update(|_| Ok::<_, Infallible>(replace(b"identified")))
        .expect("initial state fits");

    state
        .update(|_| Ok::<_, Infallible>(replace(b"")))
        .expect("empty state is valid");

    assert!(state.is_empty());
}

#[test]
fn an_oversized_replacement_is_rejected_atomically() {
    let mut state = SessionState::new(limit(4));
    state
        .update(|_| Ok::<_, Infallible>(replace(b"old")))
        .expect("initial state fits");

    assert_eq!(
        state.update(|_| Ok::<_, Infallible>(replace(b"large"))),
        Err(SessionStateUpdateError::LimitExceeded {
            limit: 4,
            attempted: 5,
        })
    );
    assert_eq!(state.as_bytes(), b"old");
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestPackageError;

impl fmt::Display for TestPackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid transition")
    }
}

impl Error for TestPackageError {}

#[test]
fn a_package_failure_preserves_the_current_state() {
    let mut state = SessionState::new(limit(16));
    state
        .update(|_| Ok::<_, Infallible>(replace(b"identified")))
        .expect("initial state fits");

    assert_eq!(
        state.update(|current| {
            assert_eq!(current, b"identified");
            Err(TestPackageError)
        }),
        Err(SessionStateUpdateError::Package(TestPackageError))
    );
    assert_eq!(state.as_bytes(), b"identified");
}

#[test]
fn debug_output_does_not_expose_opaque_state_bytes() {
    let mut state = SessionState::new(limit(16));
    let action = replace(b"secret");
    let action_debug = format!("{action:?}");

    state
        .update(|_| Ok::<_, Infallible>(action))
        .expect("state fits");
    let state_debug = format!("{state:?}");

    assert!(action_debug.contains("length: 6"));
    assert!(!action_debug.contains("secret"));
    assert!(state_debug.contains("length: 6"));
    assert!(state_debug.contains("limit: 16"));
    assert!(!state_debug.contains("secret"));
}
