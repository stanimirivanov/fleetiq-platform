use std::{error::Error, fmt};

use wasmtime::Trap;

/// Failure to create a WebAssembly sandbox.
#[derive(Debug)]
pub enum SandboxCreationError {
    /// A zero epoch tick cannot provide a deadline resolution.
    ZeroEpochTick,
    /// Wasmtime rejected the engine configuration.
    Engine(wasmtime::Error),
    /// The operating system could not create the epoch ticker thread.
    Ticker(std::io::Error),
}

impl fmt::Display for SandboxCreationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroEpochTick => formatter.write_str("WebAssembly epoch tick must be nonzero"),
            Self::Engine(error) => write!(formatter, "failed to configure Wasmtime: {error}"),
            Self::Ticker(error) => {
                write!(formatter, "failed to start Wasmtime epoch ticker: {error}")
            }
        }
    }
}

impl Error for SandboxCreationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ZeroEpochTick => None,
            Self::Engine(error) => Some(error.as_ref()),
            Self::Ticker(error) => Some(error),
        }
    }
}

/// Failure while admitting and compiling an untrusted WebAssembly module.
#[derive(Debug)]
pub enum ModuleAdmissionError {
    /// The raw module is larger than the host-owned admission limit.
    ModuleTooLarge {
        /// Maximum permitted module size in bytes.
        limit: usize,
        /// Actual raw module size in bytes.
        actual: usize,
    },
    /// The module requests host imports, but the current runtime exposes none.
    HostImportsNotAllowed {
        /// Number of imports declared by the module.
        count: usize,
    },
    /// The module declares more tables than the current runtime supports.
    TooManyTables {
        /// Maximum supported number of tables.
        limit: u32,
        /// Number of tables declared by the module.
        actual: u32,
    },
    /// The module does not export the host-selected probe entry point.
    ProbeExportMissing {
        /// Host-selected export name.
        export: Box<str>,
    },
    /// The host-selected probe export exists but is not a function.
    ProbeExportNotFunction {
        /// Host-selected export name.
        export: Box<str>,
    },
    /// The probe function does not have the temporary no-argument signature.
    ProbeExportSignatureMismatch {
        /// Host-selected export name.
        export: Box<str>,
        /// Number of parameters declared by the function.
        parameters: usize,
        /// Number of results declared by the function.
        results: usize,
    },
    /// Wasmtime could not validate or compile the module.
    Compilation(wasmtime::Error),
}

impl fmt::Display for ModuleAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModuleTooLarge { limit, actual } => write!(
                formatter,
                "WebAssembly module contains {actual} bytes, exceeding the {limit}-byte admission limit"
            ),
            Self::HostImportsNotAllowed { count } => write!(
                formatter,
                "WebAssembly module declares {count} host imports, but this runtime exposes none"
            ),
            Self::TooManyTables { limit, actual } => write!(
                formatter,
                "WebAssembly module declares {actual} tables, exceeding the supported count of {limit}"
            ),
            Self::ProbeExportMissing { export } => {
                write!(
                    formatter,
                    "WebAssembly module does not export probe entry point {export:?}"
                )
            }
            Self::ProbeExportNotFunction { export } => write!(
                formatter,
                "WebAssembly probe entry point {export:?} is not a function"
            ),
            Self::ProbeExportSignatureMismatch {
                export,
                parameters,
                results,
            } => write!(
                formatter,
                "WebAssembly probe entry point {export:?} has {parameters} parameters and {results} results; expected no parameters or results"
            ),
            Self::Compilation(error) => {
                write!(
                    formatter,
                    "failed to validate or compile WebAssembly module: {error}"
                )
            }
        }
    }
}

impl Error for ModuleAdmissionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ModuleTooLarge { .. }
            | Self::HostImportsNotAllowed { .. }
            | Self::TooManyTables { .. }
            | Self::ProbeExportMissing { .. }
            | Self::ProbeExportNotFunction { .. }
            | Self::ProbeExportSignatureMismatch { .. } => None,
            Self::Compilation(error) => Some(error.as_ref()),
        }
    }
}

/// Failure during a bounded WebAssembly probe invocation.
#[derive(Debug)]
pub enum InvocationError {
    /// The invocation consumed its deterministic fuel allowance.
    FuelExhausted,
    /// The invocation reached its host-driven epoch deadline.
    DeadlineExceeded,
    /// Guest calls exceeded the sandbox's approximate WebAssembly stack limit.
    StackLimitExceeded {
        /// Approximate WebAssembly stack ceiling configured on the engine.
        configured_limit: usize,
    },
    /// Initial allocation or growth exceeded the linear-memory limit.
    LinearMemoryLimitExceeded {
        /// Maximum permitted linear-memory size in bytes.
        limit: usize,
        /// Linear-memory size requested by the guest in bytes.
        attempted: usize,
    },
    /// Initial allocation or growth exceeded the table-element limit.
    TableElementLimitExceeded {
        /// Maximum permitted number of elements in the guest table.
        limit: usize,
        /// Number of table elements requested by the guest.
        attempted: usize,
    },
    /// Wasmtime reported another instantiation, export, or guest failure.
    Runtime(wasmtime::Error),
}

impl fmt::Display for InvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FuelExhausted => formatter.write_str("WebAssembly invocation exhausted its fuel"),
            Self::DeadlineExceeded => {
                formatter.write_str("WebAssembly invocation exceeded its wall-clock deadline")
            }
            Self::StackLimitExceeded { configured_limit } => write!(
                formatter,
                "WebAssembly invocation exceeded its approximately {configured_limit}-byte guest stack limit"
            ),
            Self::LinearMemoryLimitExceeded { limit, attempted } => write!(
                formatter,
                "WebAssembly requested {attempted} linear-memory bytes, exceeding the {limit}-byte limit"
            ),
            Self::TableElementLimitExceeded { limit, attempted } => write!(
                formatter,
                "WebAssembly requested {attempted} table elements, exceeding the {limit}-element limit"
            ),
            Self::Runtime(error) => write!(formatter, "WebAssembly invocation failed: {error}"),
        }
    }
}

impl Error for InvocationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FuelExhausted
            | Self::DeadlineExceeded
            | Self::StackLimitExceeded { .. }
            | Self::LinearMemoryLimitExceeded { .. }
            | Self::TableElementLimitExceeded { .. } => None,
            Self::Runtime(error) => Some(error.as_ref()),
        }
    }
}

pub(crate) fn classify_invocation_error(
    error: wasmtime::Error,
    max_wasm_stack_bytes: usize,
) -> InvocationError {
    match error.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => InvocationError::FuelExhausted,
        Some(Trap::Interrupt) => InvocationError::DeadlineExceeded,
        Some(Trap::StackOverflow) => InvocationError::StackLimitExceeded {
            configured_limit: max_wasm_stack_bytes,
        },
        _ => InvocationError::Runtime(error),
    }
}
