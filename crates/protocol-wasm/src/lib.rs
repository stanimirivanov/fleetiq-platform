//! Wasmtime resource-enforcement experiment for `FleetIQ` protocol packages.
//!
//! This crate proves that the host can stop runaway WebAssembly with a
//! deterministic fuel allowance, an external wall-clock deadline, an explicit
//! guest stack ceiling, and guest memory and table limits. Its temporary
//! no-argument probe export is validated and bound during admission. A second
//! experiment transfers bounded opaque bytes through fully validated guest
//! memory ranges. A versioned WIT world now defines the typed protocol-package
//! component boundary, admission, and bounded typed invocation.

mod component_abi;

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

pub use component_abi::{
    CompiledProtocolComponent, ComponentAdmissionError, ComponentAdmissionPolicy,
    ComponentOutputError, PROTOCOL_PACKAGE_ABI_VERSION, PROTOCOL_PACKAGE_WIT,
    PROTOCOL_PACKAGE_WORLD, ProcessedProtocolFrame, ProcessedProtocolSessionTurn,
    ProtocolComponentInvocationError, ProtocolFramePoll, ProtocolFramePollError,
    ProtocolFrameProcessingError, ProtocolSessionTurn, ProtocolSessionTurnBudgets,
    ProtocolSessionTurnError,
};

#[cfg(test)]
mod tests;
