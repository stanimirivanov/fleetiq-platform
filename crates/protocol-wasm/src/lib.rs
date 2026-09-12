//! Wasmtime resource-enforcement experiment for `FleetIQ` protocol packages.
//!
//! This crate proves that the host can stop runaway WebAssembly with a
//! deterministic fuel allowance, an external wall-clock deadline, an explicit
//! guest stack ceiling, and guest memory and table limits. Its temporary
//! no-argument probe export is validated and bound during admission. A second
//! experiment transfers bounded opaque bytes through fully validated guest
//! memory ranges. Neither path is the stable protocol package ABI.

mod artifact;
mod budget;
mod error;
mod probe;
mod sandbox;
mod transfer;

pub use artifact::ModuleAdmissionPolicy;
pub use budget::{InvocationBudget, InvocationBudgetError};
pub use error::{InvocationError, ModuleAdmissionError, SandboxCreationError, TransferExportRole};
pub use probe::CompiledModule;
pub use sandbox::{InvocationReport, WasmSandbox};
pub use transfer::{
    CompiledTransferModule, GuestMemoryRegion, OpaqueTransferError, OpaqueTransferLimits,
    OpaqueTransferOutput, TransferExportNames,
};

#[cfg(test)]
mod tests;
