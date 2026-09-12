use std::{error::Error, fmt, num::NonZeroU32};

use wasmtime::{Module, ModuleExport};

use crate::{InvocationError, InvocationReport};

/// Host-selected exports for the opaque core WebAssembly transfer experiment.
///
/// The memory export must be a core WebAssembly memory. The allocator must have
/// signature `(i32) -> i32`, accepting an input length and returning its guest
/// pointer. The processor must have signature `(i32, i32) -> i64`, accepting
/// the input pointer and length and returning an output pointer in the high 32
/// bits and output length in the low 32 bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferExportNames<'a> {
    memory: &'a str,
    allocate: &'a str,
    process: &'a str,
}

impl<'a> TransferExportNames<'a> {
    /// Creates an export-name selection for module admission.
    pub const fn new(memory: &'a str, allocate: &'a str, process: &'a str) -> Self {
        Self {
            memory,
            allocate,
            process,
        }
    }

    /// Returns the selected guest-memory export name.
    pub const fn memory(self) -> &'a str {
        self.memory
    }

    /// Returns the selected input-allocator export name.
    pub const fn allocate(self) -> &'a str {
        self.allocate
    }

    /// Returns the selected processing-function export name.
    pub const fn process(self) -> &'a str {
        self.process
    }
}

/// Compiled module with all opaque-transfer exports bound during admission.
#[derive(Clone)]
pub struct CompiledTransferModule {
    module: Module,
    memory: ModuleExport,
    allocate: ModuleExport,
    process: ModuleExport,
}

impl fmt::Debug for CompiledTransferModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompiledTransferModule")
            .finish_non_exhaustive()
    }
}

/// Host-owned byte limits for one opaque payload exchange.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpaqueTransferLimits {
    input_bytes: NonZeroU32,
    output_bytes: NonZeroU32,
}

impl OpaqueTransferLimits {
    /// Creates explicit nonzero input and output byte ceilings.
    pub const fn new(input_bytes: NonZeroU32, output_bytes: NonZeroU32) -> Self {
        Self {
            input_bytes,
            output_bytes,
        }
    }

    /// Returns the maximum opaque input bytes supplied to the guest.
    pub const fn max_input_bytes(self) -> NonZeroU32 {
        self.input_bytes
    }

    /// Returns the maximum opaque output bytes copied from the guest.
    pub const fn max_output_bytes(self) -> NonZeroU32 {
        self.output_bytes
    }
}

/// Guest memory region being validated for a host transfer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuestMemoryRegion {
    /// Region returned by the guest allocator for host input.
    Input,
    /// Region returned by the guest processor for host output.
    Output,
}

impl fmt::Display for GuestMemoryRegion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input => formatter.write_str("input"),
            Self::Output => formatter.write_str("output"),
        }
    }
}

/// Opaque guest bytes and resource usage returned by a successful invocation.
#[derive(Clone, Eq, PartialEq)]
pub struct OpaqueTransferOutput {
    bytes: Box<[u8]>,
    report: InvocationReport,
}

impl OpaqueTransferOutput {
    /// Returns the copied opaque result bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns resource usage for the bounded invocation.
    pub const fn report(&self) -> InvocationReport {
        self.report
    }

    /// Consumes the result into its opaque bytes and resource report.
    pub fn into_parts(self) -> (Box<[u8]>, InvocationReport) {
        (self.bytes, self.report)
    }
}

impl fmt::Debug for OpaqueTransferOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpaqueTransferOutput")
            .field("length", &self.bytes.len())
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

/// Failure while exchanging an opaque payload with a WebAssembly guest.
#[derive(Debug)]
pub enum OpaqueTransferError {
    /// Host input exceeded the configured transfer limit.
    InputTooLarge {
        /// Maximum input bytes permitted.
        limit: u32,
        /// Actual host input bytes.
        actual: usize,
    },
    /// Guest output exceeded the configured transfer limit.
    OutputTooLarge {
        /// Maximum output bytes permitted.
        limit: u32,
        /// Output length reported by the guest.
        actual: u32,
    },
    /// A guest-provided pointer-length pair is outside current linear memory.
    GuestMemoryOutOfBounds {
        /// Whether input or output memory was invalid.
        region: GuestMemoryRegion,
        /// Unsigned 32-bit guest offset.
        offset: u32,
        /// Unsigned 32-bit guest region length.
        length: u32,
        /// Current guest linear-memory size in bytes.
        memory_size: usize,
    },
    /// The host could not reserve memory for the bounded output copy.
    HostOutputAllocationFailed {
        /// Number of output bytes requested by the guest.
        requested: u32,
    },
    /// Wasmtime instantiation, execution, or resource enforcement failed.
    Invocation(InvocationError),
}

impl fmt::Display for OpaqueTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge { limit, actual } => write!(
                formatter,
                "opaque WebAssembly input contains {actual} bytes, exceeding the {limit}-byte limit"
            ),
            Self::OutputTooLarge { limit, actual } => write!(
                formatter,
                "opaque WebAssembly output contains {actual} bytes, exceeding the {limit}-byte limit"
            ),
            Self::GuestMemoryOutOfBounds {
                region,
                offset,
                length,
                memory_size,
            } => write!(
                formatter,
                "guest {region} range at offset {offset} with length {length} exceeds the {memory_size}-byte linear memory"
            ),
            Self::HostOutputAllocationFailed { requested } => write!(
                formatter,
                "host could not reserve {requested} bytes for opaque WebAssembly output"
            ),
            Self::Invocation(error) => write!(formatter, "opaque transfer failed: {error}"),
        }
    }
}

impl Error for OpaqueTransferError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Invocation(error) => Some(error),
            _ => None,
        }
    }
}

mod admission;
mod invocation;

#[cfg(test)]
mod tests;
