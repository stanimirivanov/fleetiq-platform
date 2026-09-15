//! Host-gated processing of frames through admitted protocol components.

use std::{error::Error, fmt};

use fleetiq_protocol_host::{
    AppliedFrameProcessingOutput, FrameProcessingGate, FrameProcessingValidationError,
    SessionState, WireFrame,
};
use fleetiq_protocol_sdk::FrameProcessingInput;
use fleetiq_telemetry::ProtocolPackageRef;

use super::{AdmittedProtocolComponent, ProtocolArtifactDigest, ProtocolComponentInvocationError};
use crate::{InvocationBudget, InvocationReport, WasmSandbox};

/// Host-validated effects and resource usage from one processed wire frame.
#[derive(Clone, Eq, PartialEq)]
pub struct ProcessedProtocolFrame {
    package: ProtocolPackageRef,
    artifact_digest: ProtocolArtifactDigest,
    output: AppliedFrameProcessingOutput,
    report: InvocationReport,
}

impl ProcessedProtocolFrame {
    /// Returns the immutable package release that produced this result.
    pub const fn package(&self) -> &ProtocolPackageRef {
        &self.package
    }

    /// Returns the digest of the exact component artifact that was invoked.
    pub const fn artifact_digest(&self) -> ProtocolArtifactDigest {
        self.artifact_digest
    }

    /// Returns the decoded outputs and replies accepted by the host gate.
    pub const fn output(&self) -> &AppliedFrameProcessingOutput {
        &self.output
    }

    /// Returns resource usage for component instantiation and execution.
    pub const fn report(&self) -> InvocationReport {
        self.report
    }

    /// Consumes the result into its execution identity, effects, and report.
    pub fn into_parts(
        self,
    ) -> (
        ProtocolPackageRef,
        ProtocolArtifactDigest,
        AppliedFrameProcessingOutput,
        InvocationReport,
    ) {
        (self.package, self.artifact_digest, self.output, self.report)
    }
}

impl fmt::Debug for ProcessedProtocolFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProcessedProtocolFrame")
            .field("package", &self.package)
            .field("artifact_digest", &self.artifact_digest)
            .field("output", &self.output)
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

/// Failure while invoking and host-validating one complete wire frame.
#[derive(Debug)]
pub enum ProtocolFrameProcessingError {
    /// Component invocation failed or the package rejected the frame.
    Invocation(ProtocolComponentInvocationError),
    /// The component completed, but its proposed effects violated a host limit.
    Validation {
        /// Host processing-gate failure.
        source: FrameProcessingValidationError,
        /// Resources consumed before host validation rejected the proposal.
        report: InvocationReport,
    },
}

impl fmt::Display for ProtocolFrameProcessingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invocation(error) => {
                write!(formatter, "protocol frame invocation failed: {error}")
            }
            Self::Validation { source, .. } => {
                write!(formatter, "protocol frame output was rejected: {source}")
            }
        }
    }
}

impl Error for ProtocolFrameProcessingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invocation(error) => Some(error),
            Self::Validation { source, .. } => Some(source),
        }
    }
}

impl WasmSandbox {
    /// Invokes a component for one extracted frame and applies its complete
    /// proposal through the trusted host processing gate.
    ///
    /// The invocation receives the bytes from `session_state` immediately
    /// before execution. The same state object is passed to the gate afterward,
    /// preventing callers from supplying stale or unrelated state to either
    /// half of the operation. Component failure and host validation failure
    /// preserve the previous state and expose no decoded outputs or replies.
    ///
    /// This method does not persist decoded observations, consume bytes from a
    /// session input buffer, or release replies to transport.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolFrameProcessingError::Invocation`] when component
    /// instantiation or execution fails, the package rejects the frame, or its
    /// lifted output violates SDK invariants. Returns
    /// [`ProtocolFrameProcessingError::Validation`] when the complete semantic
    /// proposal violates a host processing limit.
    pub fn process_component_frame(
        &self,
        component: &AdmittedProtocolComponent,
        budget: InvocationBudget,
        gate: &FrameProcessingGate,
        session_state: &mut SessionState,
        frame: &WireFrame,
    ) -> Result<ProcessedProtocolFrame, ProtocolFrameProcessingError> {
        let invocation = self
            .invoke_protocol_processing(
                component,
                budget,
                FrameProcessingInput::new(frame.as_bytes(), session_state.as_bytes()),
            )
            .map_err(ProtocolFrameProcessingError::Invocation)?;
        let (proposed, report) = invocation.into_parts();
        let output = gate
            .apply(session_state, proposed)
            .map_err(|source| ProtocolFrameProcessingError::Validation { source, report })?;

        Ok(ProcessedProtocolFrame {
            package: component.package().clone(),
            artifact_digest: component.artifact_digest(),
            output,
            report,
        })
    }
}

#[cfg(test)]
mod tests;
