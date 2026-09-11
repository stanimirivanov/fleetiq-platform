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

/// Failure during a bounded WebAssembly probe invocation.
#[derive(Debug)]
pub enum InvocationError {
    /// The invocation consumed its deterministic fuel allowance.
    FuelExhausted,
    /// The invocation reached its host-driven epoch deadline.
    DeadlineExceeded,
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
            Self::Runtime(error) => write!(formatter, "WebAssembly invocation failed: {error}"),
        }
    }
}

impl Error for InvocationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FuelExhausted | Self::DeadlineExceeded => None,
            Self::Runtime(error) => Some(error.as_ref()),
        }
    }
}

pub(crate) fn classify_invocation_error(error: wasmtime::Error) -> InvocationError {
    match error.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => InvocationError::FuelExhausted,
        Some(Trap::Interrupt) => InvocationError::DeadlineExceeded,
        _ => InvocationError::Runtime(error),
    }
}
