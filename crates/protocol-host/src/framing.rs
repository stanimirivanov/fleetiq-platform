use std::{error::Error, fmt, num::NonZeroUsize};

use fleetiq_protocol_sdk::FrameBoundary;

/// Host-owned unread transport input for one protocol session.
///
/// The buffer has a hard byte limit. Appending is atomic: input that would cross
/// the limit is rejected without changing the bytes already retained. A package
/// only receives a borrowed view when the host asks it to locate the first frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionInputBuffer {
    bytes: Vec<u8>,
    limit: NonZeroUsize,
}

impl SessionInputBuffer {
    /// Creates an empty session buffer with the given hard unread-byte limit.
    pub fn new(limit: NonZeroUsize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }

    /// Appends bytes received from the transport.
    ///
    /// # Errors
    ///
    /// Returns [`InputBufferError::LimitExceeded`] and leaves the buffer
    /// unchanged when the combined unread input would exceed the configured
    /// limit.
    pub fn push(&mut self, input: &[u8]) -> Result<(), InputBufferError> {
        let attempted = self.bytes.len().saturating_add(input.len());
        if attempted > self.limit.get() {
            return Err(InputBufferError::LimitExceeded {
                limit: self.limit.get(),
                attempted,
            });
        }

        self.bytes.extend_from_slice(input);
        Ok(())
    }

    /// Lets a protocol package inspect unread input and extracts at most one frame.
    ///
    /// A package decision that requests more bytes leaves the buffer unchanged. A
    /// reported frame length is validated against the available bytes before the
    /// host removes that prefix. Package failures and invalid lengths also preserve
    /// the buffer so the caller can apply its session failure policy.
    ///
    /// # Errors
    ///
    /// Returns [`FramePollError::Package`] when the package rejects the input, or
    /// [`FramePollError::FrameLengthExceedsBuffer`] when it reports more bytes than
    /// are available. Both failures leave the buffer unchanged.
    pub fn poll_frame<E>(
        &mut self,
        locate: impl FnOnce(&[u8]) -> Result<FrameBoundary, E>,
    ) -> Result<FramePoll, FramePollError<E>> {
        let boundary = locate(&self.bytes).map_err(FramePollError::Package)?;

        let FrameBoundary::Frame { length } = boundary else {
            return Ok(FramePoll::NeedMoreData);
        };

        let requested = length.get();
        let available = self.bytes.len();
        if requested > available {
            return Err(FramePollError::FrameLengthExceedsBuffer {
                requested,
                available,
            });
        }

        let frame = self.bytes.drain(..requested).collect::<Vec<_>>();
        Ok(FramePoll::Frame(WireFrame(frame.into_boxed_slice())))
    }

    /// Returns the unread bytes currently owned by the host.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the number of unread bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns whether the buffer has no unread bytes.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Returns the configured hard unread-byte limit.
    pub fn limit(&self) -> NonZeroUsize {
        self.limit
    }
}

/// The result of asking a protocol package to locate one frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FramePoll {
    /// The package needs more transport input.
    NeedMoreData,
    /// The host extracted one complete frame and retained any following bytes.
    Frame(WireFrame),
}

/// One complete protocol frame extracted from host-owned session input.
#[derive(Clone, Eq, PartialEq)]
pub struct WireFrame(Box<[u8]>);

impl WireFrame {
    /// Returns the complete wire bytes, including protocol headers and terminators.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consumes the frame and returns its owned wire bytes.
    pub fn into_bytes(self) -> Box<[u8]> {
        self.0
    }

    /// Returns the frame length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the frame has no bytes.
    ///
    /// A `WireFrame` produced by [`SessionInputBuffer`] is always nonempty because
    /// [`FrameBoundary`] requires a nonzero length.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for WireFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WireFrame")
            .field("length", &self.len())
            .finish()
    }
}

/// Failure to append transport input to a bounded session buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputBufferError {
    /// The append would exceed the configured hard byte limit.
    LimitExceeded {
        /// Maximum unread bytes allowed for the session.
        limit: usize,
        /// Unread bytes that would exist after the rejected append.
        attempted: usize,
    },
}

impl fmt::Display for InputBufferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded { limit, attempted } => write!(
                formatter,
                "session input would contain {attempted} bytes, exceeding the {limit}-byte limit"
            ),
        }
    }
}

impl Error for InputBufferError {}

/// Failure while a protocol package locates a frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FramePollError<E> {
    /// The package could not classify the current input.
    Package(E),
    /// The package reported a frame longer than the bytes available to it.
    FrameLengthExceedsBuffer {
        /// Length reported by the package.
        requested: usize,
        /// Unread bytes available when the package made the decision.
        available: usize,
    },
}

impl<E: fmt::Display> fmt::Display for FramePollError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => write!(formatter, "protocol package framing failed: {error}"),
            Self::FrameLengthExceedsBuffer {
                requested,
                available,
            } => write!(
                formatter,
                "protocol package reported a {requested}-byte frame with only {available} bytes available"
            ),
        }
    }
}

impl<E: Error + 'static> Error for FramePollError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Package(error) => Some(error),
            Self::FrameLengthExceedsBuffer { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;
