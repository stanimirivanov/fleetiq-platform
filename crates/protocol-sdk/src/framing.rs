use std::num::NonZeroUsize;

/// A protocol package's decision about the first frame in a borrowed input buffer.
///
/// The host owns the bytes and decides how much unread input a session may retain.
/// A package may only request more data or report the nonzero length of the first
/// complete frame. It cannot consume, replace, or retain the host's buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameBoundary {
    /// The current input prefix does not yet contain a complete frame.
    NeedMoreData,
    /// The first `length` bytes form one complete frame.
    Frame {
        /// The complete frame length, including protocol headers and terminators.
        length: NonZeroUsize,
    },
}
