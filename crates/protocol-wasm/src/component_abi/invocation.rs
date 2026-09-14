//! Bounded invocation of admitted protocol-package components.

use std::{error::Error, fmt};

use fleetiq_protocol_sdk::{
    FrameBoundary, FrameProcessingInput, FrameProcessingOutput, FramingInput,
};

use super::{
    CompiledProtocolComponent, bindings::exports::fleetiq::protocol_package::runtime as wit,
    conversion::ComponentOutputError,
};
use crate::{InvocationBudget, InvocationError, InvocationReport, WasmSandbox};

/// A semantically validated component value and its resource-usage report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ProtocolComponentOutput<T> {
    value: T,
    report: InvocationReport,
}

impl<T> ProtocolComponentOutput<T> {
    /// Consumes the result into its semantic value and resource report.
    pub(super) fn into_parts(self) -> (T, InvocationReport) {
        (self.value, self.report)
    }
}

/// Failure while invoking an admitted protocol component.
#[derive(Debug)]
pub enum ProtocolComponentInvocationError {
    /// The package rejected input according to its protocol rules.
    PackageRejected {
        /// Stable package-defined error code.
        code: u32,
        /// Resources consumed before the package returned the rejection.
        report: InvocationReport,
    },
    /// The package returned data that violates the semantic SDK contract.
    InvalidOutput {
        /// Semantic contract violation.
        source: ComponentOutputError,
        /// Resources consumed before the invalid value was returned.
        report: InvocationReport,
    },
    /// Component instantiation, execution, or resource enforcement failed.
    Invocation(InvocationError),
}

impl fmt::Display for ProtocolComponentInvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PackageRejected { code, .. } => {
                write!(
                    formatter,
                    "protocol package rejected input with code {code}"
                )
            }
            Self::InvalidOutput { source, .. } => {
                write!(
                    formatter,
                    "protocol package returned invalid output: {source}"
                )
            }
            Self::Invocation(error) => {
                write!(formatter, "protocol component invocation failed: {error}")
            }
        }
    }
}

impl Error for ProtocolComponentInvocationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidOutput { source, .. } => Some(source),
            Self::Invocation(error) => Some(error),
            Self::PackageRejected { .. } => None,
        }
    }
}

impl WasmSandbox {
    /// Locates the first frame by invoking an admitted component in fresh state.
    ///
    /// The host copies unread transport bytes and opaque session state through
    /// the typed WIT boundary. A new store and component instance are created
    /// for every call, so guest globals and memory cannot leak between sessions
    /// or invocations. Package-owned continuity must travel through the explicit
    /// session-state bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolComponentInvocationError::PackageRejected`] for a
    /// package-declared protocol error,
    /// [`ProtocolComponentInvocationError::InvalidOutput`] when the component
    /// returns a frame decision that violates SDK invariants, or
    /// [`ProtocolComponentInvocationError::Invocation`] when instantiation,
    /// execution, or resource enforcement fails.
    pub(super) fn invoke_protocol_framing(
        &self,
        component: &CompiledProtocolComponent,
        budget: InvocationBudget,
        input: FramingInput<'_>,
    ) -> Result<ProtocolComponentOutput<FrameBoundary>, ProtocolComponentInvocationError> {
        let input: wit::InvocationInput = input.into();
        let mut store = self
            .prepare_store(budget)
            .map_err(ProtocolComponentInvocationError::Invocation)?;
        let operation = (|| {
            let package = component.bindings.instantiate(&mut store)?;
            package
                .fleetiq_protocol_package_runtime()
                .call_locate_frame(&mut store, &input)
        })();
        let (result, report) = self
            .complete_invocation(&store, budget, operation)
            .map_err(ProtocolComponentInvocationError::Invocation)?;
        let boundary = result.map_err(|error| package_rejection(error, report))?;
        let value = boundary
            .try_into()
            .map_err(|source| ProtocolComponentInvocationError::InvalidOutput { source, report })?;

        Ok(ProtocolComponentOutput { value, report })
    }

    /// Processes one complete frame in a fresh bounded component instance.
    ///
    /// The returned value is converted into the semantic SDK model before it is
    /// exposed. The caller remains responsible for applying decoded facts,
    /// session-state changes, and replies as one host-controlled operation.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolComponentInvocationError::PackageRejected`] for a
    /// package-declared protocol error,
    /// [`ProtocolComponentInvocationError::InvalidOutput`] when proposed output
    /// violates SDK invariants, or
    /// [`ProtocolComponentInvocationError::Invocation`] when instantiation,
    /// execution, or resource enforcement fails.
    pub(super) fn invoke_protocol_processing(
        &self,
        component: &CompiledProtocolComponent,
        budget: InvocationBudget,
        input: FrameProcessingInput<'_>,
    ) -> Result<ProtocolComponentOutput<FrameProcessingOutput>, ProtocolComponentInvocationError>
    {
        let input: wit::InvocationInput = input.into();
        let mut store = self
            .prepare_store(budget)
            .map_err(ProtocolComponentInvocationError::Invocation)?;
        let operation = (|| {
            let package = component.bindings.instantiate(&mut store)?;
            package
                .fleetiq_protocol_package_runtime()
                .call_process_frame(&mut store, &input)
        })();
        let (result, report) = self
            .complete_invocation(&store, budget, operation)
            .map_err(ProtocolComponentInvocationError::Invocation)?;
        let output = result.map_err(|error| package_rejection(error, report))?;
        let value = output
            .try_into()
            .map_err(|source| ProtocolComponentInvocationError::InvalidOutput { source, report })?;

        Ok(ProtocolComponentOutput { value, report })
    }
}

const fn package_rejection(
    error: wit::PackageError,
    report: InvocationReport,
) -> ProtocolComponentInvocationError {
    ProtocolComponentInvocationError::PackageRejected {
        code: error.code,
        report,
    }
}

#[cfg(test)]
mod tests;
