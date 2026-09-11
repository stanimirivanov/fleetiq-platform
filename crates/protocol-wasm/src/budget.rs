use std::{error::Error, fmt, num::NonZeroU64, time::Duration};

/// Host-controlled compute limits for one WebAssembly invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationBudget {
    fuel: NonZeroU64,
    wall_time: Duration,
}

impl InvocationBudget {
    /// Creates an invocation budget with deterministic fuel and wall-clock limits.
    ///
    /// # Errors
    ///
    /// Returns [`InvocationBudgetError::ZeroWallTime`] when `wall_time` is zero.
    pub fn new(fuel: NonZeroU64, wall_time: Duration) -> Result<Self, InvocationBudgetError> {
        if wall_time.is_zero() {
            return Err(InvocationBudgetError::ZeroWallTime);
        }
        Ok(Self { fuel, wall_time })
    }

    /// Returns the maximum Wasmtime fuel available to the invocation.
    pub const fn fuel(&self) -> NonZeroU64 {
        self.fuel
    }

    /// Returns the maximum requested wall-clock execution time.
    pub const fn wall_time(&self) -> Duration {
        self.wall_time
    }
}

/// Invalid WebAssembly invocation budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvocationBudgetError {
    /// A zero wall-clock allowance would make every invocation immediately fail.
    ZeroWallTime,
}

impl fmt::Display for InvocationBudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroWallTime => {
                formatter.write_str("WebAssembly invocation wall time must be nonzero")
            }
        }
    }
}

impl Error for InvocationBudgetError {}
