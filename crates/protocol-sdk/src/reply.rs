use std::{error::Error, fmt};

/// The host condition a protocol package requires before releasing a reply.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyRelease {
    /// Release after the package invocation succeeds and host validation passes.
    AfterProcessing,
    /// Release only after the triggering frame becomes durably accepted.
    AfterDurableAcceptance,
}

/// Complete reply bytes proposed by a protocol package.
///
/// A package cannot write to the transport directly. It proposes opaque bytes
/// and the condition under which the host may release them. The host applies
/// output limits and controls the actual socket write.
#[derive(Clone, Eq, PartialEq)]
pub struct ProposedReply {
    bytes: Box<[u8]>,
    release: ReplyRelease,
}

impl ProposedReply {
    /// Creates a nonempty proposed reply with an explicit release condition.
    ///
    /// # Errors
    ///
    /// Returns [`ProposedReplyError::Empty`] when `bytes` contains no data.
    pub fn new(
        bytes: impl Into<Vec<u8>>,
        release: ReplyRelease,
    ) -> Result<Self, ProposedReplyError> {
        let bytes = bytes.into().into_boxed_slice();
        if bytes.is_empty() {
            return Err(ProposedReplyError::Empty);
        }

        Ok(Self { bytes, release })
    }

    /// Returns the proposed reply length in bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns whether the proposed reply has no bytes.
    ///
    /// A successfully constructed reply is always nonempty.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Returns the host condition required before release.
    pub const fn release(&self) -> ReplyRelease {
        self.release
    }

    /// Consumes the proposal into the parts required by a trusted host adapter.
    pub fn into_parts(self) -> (Box<[u8]>, ReplyRelease) {
        (self.bytes, self.release)
    }
}

impl fmt::Debug for ProposedReply {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProposedReply")
            .field("length", &self.len())
            .field("release", &self.release)
            .finish_non_exhaustive()
    }
}

/// Invalid reply proposed by a protocol package.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposedReplyError {
    /// A transport write cannot be represented by an empty reply.
    Empty,
}

impl fmt::Display for ProposedReplyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("a proposed protocol reply cannot be empty"),
        }
    }
}

impl Error for ProposedReplyError {}
