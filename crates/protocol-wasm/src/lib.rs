//! Wasmtime resource-enforcement experiment for `FleetIQ` protocol packages.
//!
//! This crate proves that the host can stop runaway WebAssembly with a
//! deterministic fuel allowance, an external wall-clock deadline, an explicit
//! guest stack ceiling, and guest memory and table limits. Its temporary
//! no-argument probe export is validated and bound during admission, but is not
//! the protocol package ABI.

mod artifact;
mod budget;
mod error;
mod sandbox;

pub use artifact::ModuleAdmissionPolicy;
pub use budget::{InvocationBudget, InvocationBudgetError};
pub use error::{InvocationError, ModuleAdmissionError, SandboxCreationError};
pub use sandbox::{CompiledModule, InvocationReport, WasmSandbox};

#[cfg(test)]
mod tests;
