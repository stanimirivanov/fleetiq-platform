//! Admission and typed export binding for protocol-package components.

use std::{error::Error, fmt, num::NonZeroUsize};

use wasmtime::component::{Component, Linker};

use super::bindings::ProtocolPackagePre;
use crate::{WasmSandbox, sandbox::InvocationLimiter};

/// Host-owned admission policy for one WebAssembly protocol component.
///
/// The byte ceiling is applied before Wasmtime parses or compiles untrusted
/// input. It does not bound compilation time, generated native code, package
/// metadata, or the resources used by later component instances.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComponentAdmissionPolicy {
    max_component_bytes: NonZeroUsize,
}

impl ComponentAdmissionPolicy {
    /// Creates a component admission policy with a nonzero raw-byte limit.
    pub const fn new(max_component_bytes: NonZeroUsize) -> Self {
        Self {
            max_component_bytes,
        }
    }

    /// Returns the maximum accepted component size in bytes.
    pub const fn max_component_bytes(self) -> NonZeroUsize {
        self.max_component_bytes
    }
}

/// Compiled component whose imports and protocol-package exports were bound.
///
/// Construction is restricted to [`WasmSandbox::compile_protocol_component`],
/// so later invocation code does not repeat string-based export discovery or
/// interface type checking on the message path.
#[derive(Clone)]
pub struct CompiledProtocolComponent {
    _bindings: ProtocolPackagePre<InvocationLimiter>,
}

impl WasmSandbox {
    /// Admits, compiles, and binds one version-compatible protocol component.
    ///
    /// Admission checks the raw byte limit before parsing, compiles a Component
    /// Model binary with the sandbox engine, rejects every host import, and
    /// creates typed pre-instantiation bindings for the exact WIT world compiled
    /// into this host. Compilation remains outside per-invocation fuel and epoch
    /// budgets and must eventually run on bounded activation capacity.
    ///
    /// # Errors
    ///
    /// Returns [`ComponentAdmissionError::ComponentTooLarge`] before Wasmtime
    /// receives oversized input, [`ComponentAdmissionError::Compilation`] when
    /// the bytes are not a valid supported component,
    /// [`ComponentAdmissionError::HostImportsNotAllowed`] when the component
    /// requests host capabilities, or
    /// [`ComponentAdmissionError::InterfaceMismatch`] when its exports do not
    /// implement the configured protocol-package WIT world.
    pub fn compile_protocol_component(
        &self,
        wasm: &[u8],
        policy: ComponentAdmissionPolicy,
    ) -> Result<CompiledProtocolComponent, ComponentAdmissionError> {
        let actual = wasm.len();
        let limit = policy.max_component_bytes().get();
        if actual > limit {
            return Err(ComponentAdmissionError::ComponentTooLarge { limit, actual });
        }

        let component =
            Component::new(self.engine(), wasm).map_err(ComponentAdmissionError::Compilation)?;
        let import_count = component.component_type().imports(self.engine()).len();
        if import_count > 0 {
            return Err(ComponentAdmissionError::HostImportsNotAllowed {
                count: import_count,
            });
        }

        let linker = Linker::<InvocationLimiter>::new(self.engine());
        let instance = linker
            .instantiate_pre(&component)
            .map_err(ComponentAdmissionError::InterfaceMismatch)?;
        let bindings = ProtocolPackagePre::new(instance)
            .map_err(ComponentAdmissionError::InterfaceMismatch)?;

        Ok(CompiledProtocolComponent {
            _bindings: bindings,
        })
    }
}

impl fmt::Debug for CompiledProtocolComponent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompiledProtocolComponent")
            .finish_non_exhaustive()
    }
}

/// Failure while admitting a protocol-package WebAssembly component.
#[derive(Debug)]
pub enum ComponentAdmissionError {
    /// Raw component input exceeds the host-owned admission limit.
    ComponentTooLarge {
        /// Maximum permitted component size in bytes.
        limit: usize,
        /// Actual raw component size in bytes.
        actual: usize,
    },
    /// The component requests host imports while the runtime grants none.
    HostImportsNotAllowed {
        /// Number of top-level imports declared by the component.
        count: usize,
    },
    /// Wasmtime could not validate or compile the component bytes.
    Compilation(wasmtime::Error),
    /// The component does not implement the exact configured WIT world.
    InterfaceMismatch(wasmtime::Error),
}

impl fmt::Display for ComponentAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ComponentTooLarge { limit, actual } => write!(
                formatter,
                "WebAssembly component contains {actual} bytes, exceeding the {limit}-byte admission limit"
            ),
            Self::HostImportsNotAllowed { count } => write!(
                formatter,
                "WebAssembly component declares {count} host imports, but this runtime exposes none"
            ),
            Self::Compilation(error) => write!(
                formatter,
                "failed to validate or compile WebAssembly component: {error}"
            ),
            Self::InterfaceMismatch(error) => write!(
                formatter,
                "WebAssembly component does not implement the required protocol-package world: {error}"
            ),
        }
    }
}

impl Error for ComponentAdmissionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ComponentTooLarge { .. } | Self::HostImportsNotAllowed { .. } => None,
            Self::Compilation(error) | Self::InterfaceMismatch(error) => Some(error.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests;
