//! Optional identity handoff, isolated from trusted routing and authorization.
use super::{AdmittedProtocolComponent, ComponentAdmissionError, ComponentAdmissionPolicy};
use crate::{
    InvocationBudget, InvocationError, InvocationReport, WasmSandbox, sandbox::InvocationLimiter,
};
use fleetiq_protocol_sdk::{DeviceIdentityClaim, FramingInput};
use fleetiq_telemetry::ProtocolPackageRef;
use std::{error::Error, fmt, num::NonZeroUsize};
use wasmtime::component::Linker;
mod bindings {
    wasmtime::component::bindgen!({ path: "wit", world: "identity-protocol-package" });
}
/// Versioned world used by packages that propose external device identities.
pub const IDENTITY_PROTOCOL_PACKAGE_WORLD: &str =
    "fleetiq:protocol-package/identity-protocol-package@0.1.0";
/// Optional identity extension source, published alongside the original WIT.
pub const IDENTITY_PROTOCOL_PACKAGE_WIT: &str = include_str!("../../wit/identity.wit");
/// An admitted runtime component with an explicitly bound identity export.
/// Legacy components remain usable through the original admission API.
pub struct AdmittedIdentityProtocolComponent {
    protocol: AdmittedProtocolComponent,
    bindings: bindings::IdentityProtocolPackagePre<InvocationLimiter>,
}
impl AdmittedIdentityProtocolComponent {
    /// Returns the same artifact/release-bound runtime for ordinary session turns.
    pub const fn protocol(&self) -> &AdmittedProtocolComponent {
        &self.protocol
    }
}
/// Identification failed without establishing any trusted device context.
#[derive(Debug)]
pub enum IdentityInvocationError {
    /// Host input byte policy was exceeded before guest invocation.
    InputLimitExceeded,
    /// The package rejected the handshake with a stable bounded code.
    PackageRejected {
        /// Package-defined code; never package-controlled text.
        code: u32,
    },
    /// The proposal violated the SDK identity syntax or byte limits.
    InvalidClaim,
    /// Instantiation or bounded guest execution failed.
    Invocation(InvocationError),
}
impl fmt::Display for IdentityInvocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputLimitExceeded => f.write_str("identity input exceeds host policy"),
            Self::PackageRejected { code } => {
                write!(f, "identity package rejected input with code {code}")
            }
            Self::InvalidClaim => f.write_str("package proposed an invalid bounded identity"),
            Self::Invocation(e) => write!(f, "identity invocation failed: {e}"),
        }
    }
}
impl Error for IdentityInvocationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invocation(e) => Some(e),
            _ => None,
        }
    }
}
impl WasmSandbox {
    /// Admits the original runtime and explicit identity extension with no imports.
    ///
    /// # Errors
    /// Returns the ordinary admission error for limits, imports, invalid components
    /// or missing runtime/identification exports. No capabilities are granted.
    pub fn admit_identity_protocol_component(
        &self,
        package: ProtocolPackageRef,
        bytes: &[u8],
        policy: ComponentAdmissionPolicy,
    ) -> Result<AdmittedIdentityProtocolComponent, ComponentAdmissionError> {
        let (protocol, component) = self.bind_protocol_component(package, bytes, policy)?;
        let pre = Linker::<InvocationLimiter>::new(self.engine())
            .instantiate_pre(&component)
            .map_err(ComponentAdmissionError::InterfaceMismatch)?;
        let bindings = bindings::IdentityProtocolPackagePre::new(pre)
            .map_err(ComponentAdmissionError::InterfaceMismatch)?;
        Ok(AdmittedIdentityProtocolComponent { protocol, bindings })
    }
    /// Inspects a handshake in a fresh bounded store, returning only untrusted
    /// identifier evidence and resource usage. Does not alter session state,
    /// authenticate, resolve a device, select a tenant or release transport bytes.
    /// `max_input_bytes` bounds payload plus state before either is copied.
    ///
    /// # Errors
    /// Returns [`IdentityInvocationError`] for host limits, package rejection,
    /// invalid identity proposals or sandbox failures.
    pub fn identify_component_device(
        &self,
        component: &AdmittedIdentityProtocolComponent,
        budget: InvocationBudget,
        input: FramingInput<'_>,
        max_input_bytes: NonZeroUsize,
    ) -> Result<(DeviceIdentityClaim, InvocationReport), IdentityInvocationError> {
        if input
            .unread()
            .len()
            .saturating_add(input.session_state().len())
            > max_input_bytes.get()
        {
            return Err(IdentityInvocationError::InputLimitExceeded);
        }
        let input = bindings::exports::fleetiq::protocol_package::runtime::InvocationInput {
            payload: input.unread().to_vec(),
            session_state: input.session_state().to_vec(),
        };
        let mut store = self
            .prepare_store(budget)
            .map_err(IdentityInvocationError::Invocation)?;
        let operation = (|| {
            let package = component.bindings.instantiate(&mut store)?;
            package
                .fleetiq_protocol_package_identification()
                .call_identify(&mut store, &input)
        })();
        let (result, report) = self
            .complete_invocation(&store, budget, operation)
            .map_err(IdentityInvocationError::Invocation)?;
        let claim =
            result.map_err(|e| IdentityInvocationError::PackageRejected { code: e.code })?;
        let claim = DeviceIdentityClaim::new(claim.kind, claim.value)
            .map_err(|_| IdentityInvocationError::InvalidClaim)?;
        Ok((claim, report))
    }
}

#[cfg(test)]
mod tests;
