use std::{convert::Infallible, error::Error, fmt, num::NonZeroUsize};

use fleetiq_protocol_sdk::FrameBoundary;

use super::{FramePoll, FramePollError, InputBufferError, SessionInputBuffer};

fn limit(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test limits are nonzero")
}

fn binary_boundary(input: &[u8]) -> FrameBoundary {
    if input.len() < 2 {
        return FrameBoundary::NeedMoreData;
    }

    let payload_length = usize::from(u16::from_be_bytes([input[0], input[1]]));
    let frame_length = payload_length + 2;
    if input.len() < frame_length {
        return FrameBoundary::NeedMoreData;
    }

    FrameBoundary::Frame {
        length: NonZeroUsize::new(frame_length).expect("the header makes the frame nonzero"),
    }
}

fn newline_boundary(input: &[u8]) -> FrameBoundary {
    let Some(terminator) = input.iter().position(|byte| *byte == b'\n') else {
        return FrameBoundary::NeedMoreData;
    };

    FrameBoundary::Frame {
        length: NonZeroUsize::new(terminator + 1).expect("the terminator is part of the frame"),
    }
}

#[test]
fn fragmented_binary_input_is_retained_until_the_frame_is_complete() {
    let mut buffer = SessionInputBuffer::new(limit(32));

    buffer.push(&[0]).expect("first fragment fits");
    assert_eq!(
        buffer.poll_frame(|input| Ok::<_, Infallible>(binary_boundary(input))),
        Ok(FramePoll::NeedMoreData)
    );
    assert_eq!(buffer.as_bytes(), &[0]);

    buffer.push(&[3, b'a']).expect("second fragment fits");
    assert_eq!(
        buffer.poll_frame(|input| Ok::<_, Infallible>(binary_boundary(input))),
        Ok(FramePoll::NeedMoreData)
    );

    buffer.push(b"bc").expect("final fragment fits");
    let FramePoll::Frame(frame) = buffer
        .poll_frame(|input| Ok::<_, Infallible>(binary_boundary(input)))
        .expect("the complete frame is valid")
    else {
        panic!("the package should locate the completed binary frame");
    };

    assert_eq!(frame.as_bytes(), &[0, 3, b'a', b'b', b'c']);
    assert!(buffer.is_empty());
}

#[test]
fn coalesced_structured_frames_are_extracted_one_at_a_time() {
    let mut buffer = SessionInputBuffer::new(limit(64));

    buffer.push(br#"{"value":1"#).expect("fragment fits");
    assert_eq!(
        buffer.poll_frame(|input| Ok::<_, Infallible>(newline_boundary(input))),
        Ok(FramePoll::NeedMoreData)
    );

    buffer
        .push(b"}\n{\"value\":2}\n")
        .expect("coalesced input fits");

    let FramePoll::Frame(first) = buffer
        .poll_frame(|input| Ok::<_, Infallible>(newline_boundary(input)))
        .expect("the first frame is valid")
    else {
        panic!("the first structured frame should be complete");
    };
    assert_eq!(first.as_bytes(), b"{\"value\":1}\n");
    assert_eq!(buffer.as_bytes(), b"{\"value\":2}\n");

    let FramePoll::Frame(second) = buffer
        .poll_frame(|input| Ok::<_, Infallible>(newline_boundary(input)))
        .expect("the second frame is valid")
    else {
        panic!("the coalesced structured frame should remain available");
    };
    assert_eq!(second.as_bytes(), b"{\"value\":2}\n");
    assert!(buffer.is_empty());
}

#[test]
fn input_that_crosses_the_limit_is_rejected_atomically() {
    let mut buffer = SessionInputBuffer::new(limit(4));
    buffer.push(b"abc").expect("initial input fits");

    assert_eq!(
        buffer.push(b"de"),
        Err(InputBufferError::LimitExceeded {
            limit: 4,
            attempted: 5,
        })
    );
    assert_eq!(buffer.as_bytes(), b"abc");
}

#[test]
fn a_package_cannot_consume_bytes_that_are_not_available() {
    let mut buffer = SessionInputBuffer::new(limit(8));
    buffer.push(b"abc").expect("input fits");

    let result = buffer.poll_frame::<Infallible>(|_| Ok(FrameBoundary::Frame { length: limit(4) }));

    assert_eq!(
        result,
        Err(FramePollError::FrameLengthExceedsBuffer {
            requested: 4,
            available: 3,
        })
    );
    assert_eq!(buffer.as_bytes(), b"abc");
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TestPackageError;

impl fmt::Display for TestPackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid test prefix")
    }
}

impl Error for TestPackageError {}

#[test]
fn a_package_failure_preserves_unread_input() {
    let mut buffer = SessionInputBuffer::new(limit(8));
    buffer.push(b"abc").expect("input fits");

    assert_eq!(
        buffer.poll_frame(|_| Err(TestPackageError)),
        Err(FramePollError::Package(TestPackageError))
    );
    assert_eq!(buffer.as_bytes(), b"abc");
}

#[test]
fn wire_frame_debug_output_does_not_expose_payload_bytes() {
    let mut buffer = SessionInputBuffer::new(limit(8));
    buffer.push(b"secret\n").expect("input fits");
    let FramePoll::Frame(frame) = buffer
        .poll_frame(|input| Ok::<_, Infallible>(newline_boundary(input)))
        .expect("the frame is valid")
    else {
        panic!("the newline-terminated frame should be complete");
    };

    let debug = format!("{frame:?}");
    assert!(debug.contains("length: 7"));
    assert!(!debug.contains("secret"));
}
