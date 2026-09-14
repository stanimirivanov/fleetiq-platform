//! One bounded framing-and-processing turn for a protocol session.

use std::{error::Error, fmt};

use fleetiq_protocol_host::{
    AppliedFrameProcessingOutput, FramePoll, FrameProcessingGate, SessionInputBuffer, SessionState,
    WireFrame,
};

use super::{CompiledProtocolComponent, ProtocolFramePollError, ProtocolFrameProcessingError};
use crate::{InvocationBudget, InvocationReport, WasmSandbox};

/// Independent resource budgets for the two component invocations in one turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolSessionTurnBudgets {
    framing: InvocationBudget,
    processing: InvocationBudget,
}

impl ProtocolSessionTurnBudgets {
    /// Creates budgets for frame discovery and complete-frame processing.
    pub const fn new(framing: InvocationBudget, processing: InvocationBudget) -> Self {
        Self {
            framing,
            processing,
        }
    }

    /// Returns the budget for the side-effect-free framing invocation.
    pub const fn framing(self) -> InvocationBudget {
        self.framing
    }

    /// Returns the budget for the complete-frame processing invocation.
    pub const fn processing(self) -> InvocationBudget {
        self.processing
    }
}

/// Result of giving one buffered protocol session a bounded execution turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolSessionTurn {
    /// The component needs more transport bytes before a frame can be processed.
    NeedMoreData {
        /// Resources consumed while inspecting the incomplete input.
        framing_report: InvocationReport,
    },
    /// One extracted frame was processed and its effects passed host validation.
    Processed(ProcessedProtocolSessionTurn),
}

impl ProtocolSessionTurn {
    /// Returns resource usage from the framing invocation.
    pub const fn framing_report(&self) -> InvocationReport {
        match self {
            Self::NeedMoreData { framing_report }
            | Self::Processed(ProcessedProtocolSessionTurn { framing_report, .. }) => {
                *framing_report
            }
        }
    }
}

/// One extracted frame and the host-validated result of processing it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessedProtocolSessionTurn {
    frame: WireFrame,
    output: AppliedFrameProcessingOutput,
    framing_report: InvocationReport,
    processing_report: InvocationReport,
}

impl ProcessedProtocolSessionTurn {
    /// Returns the exact wire frame removed from the session input buffer.
    pub const fn frame(&self) -> &WireFrame {
        &self.frame
    }

    /// Returns decoded outputs and replies accepted by the host processing gate.
    pub const fn output(&self) -> &AppliedFrameProcessingOutput {
        &self.output
    }

    /// Returns resource usage from frame discovery.
    pub const fn framing_report(&self) -> InvocationReport {
        self.framing_report
    }

    /// Returns resource usage from complete-frame processing.
    pub const fn processing_report(&self) -> InvocationReport {
        self.processing_report
    }

    /// Consumes the turn into its frame, accepted effects, and resource reports.
    pub fn into_parts(
        self,
    ) -> (
        WireFrame,
        AppliedFrameProcessingOutput,
        InvocationReport,
        InvocationReport,
    ) {
        (
            self.frame,
            self.output,
            self.framing_report,
            self.processing_report,
        )
    }
}

/// Failure while framing or processing one protocol session turn.
#[derive(Debug)]
pub enum ProtocolSessionTurnError {
    /// No frame was removed because component framing failed.
    Framing(ProtocolFramePollError),
    /// Framing succeeded, but processing or host validation failed.
    Processing {
        /// Processing failure after extracting the frame.
        source: ProtocolFrameProcessingError,
        /// Exact frame removed from the input buffer and presented to the component.
        frame: WireFrame,
        /// Resources consumed by the successful framing invocation.
        framing_report: InvocationReport,
    },
}

impl fmt::Display for ProtocolSessionTurnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(error) => write!(formatter, "protocol session framing failed: {error}"),
            Self::Processing { source, .. } => {
                write!(
                    formatter,
                    "protocol session frame processing failed: {source}"
                )
            }
        }
    }
}

impl Error for ProtocolSessionTurnError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Framing(error) => Some(error),
            Self::Processing { source, .. } => Some(source),
        }
    }
}

impl WasmSandbox {
    /// Polls and, when complete, processes at most one frame from a session.
    ///
    /// Framing sees the current unread input and current session state. A
    /// complete prefix is removed before processing, and the component then sees
    /// that exact frame with the same state framing observed. Only the trusted
    /// processing gate may apply the proposed state transition and expose
    /// decoded outputs or staged replies.
    ///
    /// A framing failure or need-more-data outcome leaves the input buffer
    /// unchanged. After successful framing, the extracted prefix remains
    /// consumed even if processing fails; the processing error owns that exact
    /// [`WireFrame`] so session policy can quarantine, retain, or retry it.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolSessionTurnError::Framing`] before any frame is
    /// extracted. Returns [`ProtocolSessionTurnError::Processing`] with the
    /// extracted frame and framing report when component processing or host
    /// validation fails. Processing failures preserve the pre-turn session
    /// state and do not expose partial component effects.
    pub fn run_component_session_turn(
        &self,
        component: &CompiledProtocolComponent,
        budgets: ProtocolSessionTurnBudgets,
        gate: &FrameProcessingGate,
        session_state: &mut SessionState,
        input: &mut SessionInputBuffer,
    ) -> Result<ProtocolSessionTurn, ProtocolSessionTurnError> {
        let polled = self
            .poll_component_frame(component, budgets.framing(), session_state, input)
            .map_err(ProtocolSessionTurnError::Framing)?;
        let (outcome, framing_report) = polled.into_parts();

        let FramePoll::Frame(frame) = outcome else {
            return Ok(ProtocolSessionTurn::NeedMoreData { framing_report });
        };

        let processed = self
            .process_component_frame(component, budgets.processing(), gate, session_state, &frame)
            .map_err(|source| ProtocolSessionTurnError::Processing {
                source,
                frame: frame.clone(),
                framing_report,
            })?;
        let (output, processing_report) = processed.into_parts();

        Ok(ProtocolSessionTurn::Processed(
            ProcessedProtocolSessionTurn {
                frame,
                output,
                framing_report,
                processing_report,
            },
        ))
    }
}

#[cfg(test)]
mod tests;
