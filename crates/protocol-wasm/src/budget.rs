use std::{
    error::Error,
    fmt,
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};

/// Host-controlled compute and guest-resource limits for one WebAssembly invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationBudget {
    fuel: NonZeroU64,
    wall_time: Duration,
    max_linear_memory_bytes: NonZeroUsize,
    max_table_elements: NonZeroUsize,
}

impl InvocationBudget {
    /// Creates an invocation budget with compute, time, memory, and table limits.
    ///
    /// # Errors
    ///
    /// Returns [`InvocationBudgetError::ZeroWallTime`] when `wall_time` is zero.
    pub fn new(
        fuel: NonZeroU64,
        wall_time: Duration,
        max_linear_memory_bytes: NonZeroUsize,
        max_table_elements: NonZeroUsize,
    ) -> Result<Self, InvocationBudgetError> {
        if wall_time.is_zero() {
            return Err(InvocationBudgetError::ZeroWallTime);
        }
        Ok(Self {
            fuel,
            wall_time,
            max_linear_memory_bytes,
            max_table_elements,
        })
    }

    /// Returns the maximum Wasmtime fuel available to the invocation.
    pub const fn fuel(&self) -> NonZeroU64 {
        self.fuel
    }

    /// Returns the maximum requested wall-clock execution time.
    pub const fn wall_time(&self) -> Duration {
        self.wall_time
    }

    /// Returns the maximum bytes available to the guest's linear memory.
    pub const fn max_linear_memory_bytes(&self) -> NonZeroUsize {
        self.max_linear_memory_bytes
    }

    /// Returns the maximum number of elements available to the guest table.
    pub const fn max_table_elements(&self) -> NonZeroUsize {
        self.max_table_elements
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
