//! Host-controlled framing through admitted protocol components.

use std::{convert::Infallible, error::Error, fmt};

use fleetiq_protocol_host::{FramePoll, FramePollError, SessionInputBuffer, SessionState};
use fleetiq_protocol_sdk::FramingInput;

use super::{AdmittedProtocolComponent, ProtocolComponentInvocationError};
use crate::{InvocationBudget, InvocationReport, WasmSandbox};

/// One host framing decision and its component resource usage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolFramePoll {
    outcome: FramePoll,
    report: InvocationReport,
}

impl ProtocolFramePoll {
    /// Returns whether more bytes are needed or one frame was extracted.
    pub const fn outcome(&self) -> &FramePoll {
        &self.outcome
    }

    /// Returns resource usage for component instantiation and framing execution.
    pub const fn report(&self) -> InvocationReport {
        self.report
    }

    /// Consumes the result into its framing outcome and invocation report.
    pub fn into_parts(self) -> (FramePoll, InvocationReport) {
        (self.outcome, self.report)
    }
}

/// Failure while locating a frame through an admitted protocol component.
#[derive(Debug)]
pub enum ProtocolFramePollError {
    /// Component invocation failed or the package rejected the unread input.
    Invocation(ProtocolComponentInvocationError),
    /// The component reported a frame longer than the unread host buffer.
    FrameLengthExceedsBuffer {
        /// Frame length returned by the component.
        requested: usize,
        /// Unread bytes available when the component inspected the buffer.
        available: usize,
        /// Resources consumed before the invalid decision was rejected.
        report: InvocationReport,
    },
}

impl fmt::Display for ProtocolFramePollError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invocation(error) => {
                write!(formatter, "protocol framing invocation failed: {error}")
            }
            Self::FrameLengthExceedsBuffer {
                requested,
                available,
                ..
            } => write!(
                formatter,
                "protocol component reported a {requested}-byte frame with only {available} unread bytes"
            ),
        }
    }
}

impl Error for ProtocolFramePollError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invocation(error) => Some(error),
            Self::FrameLengthExceedsBuffer { .. } => None,
        }
    }
}

impl WasmSandbox {
    /// Invokes component framing against host-owned unread input and extracts at
    /// most one complete frame.
    ///
    /// The component receives a borrowed snapshot of the current unread buffer
    /// and current host-owned session state. A need-more-data decision preserves
    /// the buffer. A frame decision removes exactly the validated prefix and
    /// retains coalesced following frames. Invocation failure and an impossible
    /// frame length also preserve all unread bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolFramePollError::Invocation`] when component invocation
    /// fails, the package rejects the input, or lifted output violates SDK
    /// invariants. Returns [`ProtocolFramePollError::FrameLengthExceedsBuffer`]
    /// when the component reports more bytes than were available to it.
    pub fn poll_component_frame(
        &self,
        component: &AdmittedProtocolComponent,
        budget: InvocationBudget,
        session_state: &SessionState,
        input: &mut SessionInputBuffer,
    ) -> Result<ProtocolFramePoll, ProtocolFramePollError> {
        let invocation = self
            .invoke_protocol_framing(
                component,
                budget,
                FramingInput::new(input.as_bytes(), session_state.as_bytes()),
            )
            .map_err(ProtocolFramePollError::Invocation)?;
        let (boundary, report) = invocation.into_parts();
        let outcome = match input.poll_frame(|_| Ok::<_, Infallible>(boundary)) {
            Ok(outcome) => outcome,
            Err(FramePollError::FrameLengthExceedsBuffer {
                requested,
                available,
            }) => {
                return Err(ProtocolFramePollError::FrameLengthExceedsBuffer {
                    requested,
                    available,
                    report,
                });
            }
            Err(FramePollError::Package(never)) => match never {},
        };

        Ok(ProtocolFramePoll { outcome, report })
    }
}

#[cfg(test)]
mod tests;
