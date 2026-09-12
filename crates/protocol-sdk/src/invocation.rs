//! Semantic inputs and outputs for protocol-package invocation.

use std::fmt;

use crate::{DecodedOutput, ProposedReply, SessionStateAction};

/// Borrowed host-owned context for locating the first complete frame.
///
/// A package may inspect all currently unread transport bytes and its opaque
/// session state. It returns a [`crate::FrameBoundary`] separately and cannot
/// propose state changes, replies, or decoded values during framing. This
/// restriction keeps repeated polls of the same incomplete input free of
/// package-requested side effects.
///
/// Tenant, device, package-release, and receipt-time provenance remain trusted
/// host context and are deliberately absent.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct FramingInput<'a> {
    unread: &'a [u8],
    session_state: &'a [u8],
}

impl<'a> FramingInput<'a> {
    /// Creates a framing view over host-owned unread bytes and session state.
    pub const fn new(unread: &'a [u8], session_state: &'a [u8]) -> Self {
        Self {
            unread,
            session_state,
        }
    }

    /// Returns all unread transport bytes, beginning at the next frame.
    pub const fn unread(&self) -> &'a [u8] {
        self.unread
    }

    /// Returns the current opaque state for this connection session.
    pub const fn session_state(&self) -> &'a [u8] {
        self.session_state
    }
}

impl fmt::Debug for FramingInput<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FramingInput")
            .field("unread_length", &self.unread.len())
            .field("session_state_length", &self.session_state.len())
            .finish_non_exhaustive()
    }
}

/// Borrowed host-owned context for processing one complete wire frame.
///
/// `frame` contains exactly the bytes selected by a successful framing
/// decision. Session state is the value in effect before this invocation. A
/// package returns all proposed effects together as a [`FrameProcessingOutput`].
///
/// The input carries no trusted admission or provenance fields. The host owns
/// those fields and adds them only after it validates the complete result.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct FrameProcessingInput<'a> {
    frame: &'a [u8],
    session_state: &'a [u8],
}

impl<'a> FrameProcessingInput<'a> {
    /// Creates a processing view over one complete frame and current state.
    pub const fn new(frame: &'a [u8], session_state: &'a [u8]) -> Self {
        Self {
            frame,
            session_state,
        }
    }

    /// Returns the complete protocol frame selected by the framing stage.
    pub const fn frame(&self) -> &'a [u8] {
        self.frame
    }

    /// Returns the opaque state in effect before processing this frame.
    pub const fn session_state(&self) -> &'a [u8] {
        self.session_state
    }
}

impl fmt::Debug for FrameProcessingInput<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameProcessingInput")
            .field("frame_length", &self.frame.len())
            .field("session_state_length", &self.session_state.len())
            .finish_non_exhaustive()
    }
}

/// Complete set of effects proposed after processing one frame.
///
/// Decoded outputs and replies retain package order. Stable decoded-output
/// order is required for deterministic observation identities; stable reply
/// order determines transport write order after each proposal becomes eligible.
/// Empty sequences are valid, allowing handshake and keepalive frames that do
/// not contain telemetry or require a response.
///
/// A host must validate the whole result against its resource and semantic
/// policies before applying any part. If invocation or validation fails, it
/// preserves the previous session state, emits no observations, and releases no
/// replies. This semantic aggregate does not define serialization or memory
/// transfer across an isolated-runtime boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct FrameProcessingOutput {
    decoded_outputs: Vec<DecodedOutput>,
    session_state_action: SessionStateAction,
    replies: Vec<ProposedReply>,
}

impl FrameProcessingOutput {
    /// Creates the complete proposed result of processing one frame.
    pub fn new(
        decoded_outputs: Vec<DecodedOutput>,
        session_state_action: SessionStateAction,
        replies: Vec<ProposedReply>,
    ) -> Self {
        Self {
            decoded_outputs,
            session_state_action,
            replies,
        }
    }

    /// Returns decoded values in deterministic package-emission order.
    pub fn decoded_outputs(&self) -> &[DecodedOutput] {
        &self.decoded_outputs
    }

    /// Returns the proposed whole-state action for this session.
    pub const fn session_state_action(&self) -> &SessionStateAction {
        &self.session_state_action
    }

    /// Returns reply proposals in deterministic transport-write order.
    pub fn replies(&self) -> &[ProposedReply] {
        &self.replies
    }

    /// Consumes the result into the parts required by a trusted host.
    pub fn into_parts(self) -> (Vec<DecodedOutput>, SessionStateAction, Vec<ProposedReply>) {
        (
            self.decoded_outputs,
            self.session_state_action,
            self.replies,
        )
    }
}

impl fmt::Debug for FrameProcessingOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameProcessingOutput")
            .field("decoded_output_count", &self.decoded_outputs.len())
            .field("session_state_action", &self.session_state_action)
            .field("reply_count", &self.replies.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
