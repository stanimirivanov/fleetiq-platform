//! Wasmtime resource-enforcement experiment for `FleetIQ` protocol packages.
//!
//! This crate proves that the host can stop runaway WebAssembly with both a
//! deterministic fuel allowance and an external wall-clock deadline. Its
//! temporary no-argument probe export is not the protocol package ABI.

mod budget;
mod error;
mod sandbox;

pub use budget::{InvocationBudget, InvocationBudgetError};
pub use error::{InvocationError, SandboxCreationError};
pub use sandbox::{CompiledModule, InvocationReport, WasmSandbox};

#[cfg(test)]
mod tests;
