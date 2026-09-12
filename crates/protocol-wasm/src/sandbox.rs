use std::{
    fmt,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use wasmtime::{Config, Engine, Module, ResourceLimiter, Store};

use crate::{
    InvocationBudget, InvocationError, ModuleAdmissionError, ModuleAdmissionPolicy,
    SandboxCreationError, error::classify_invocation_error,
};

const MAX_TABLES_PER_MODULE: u32 = 1;

/// Host runtime configured for fuel consumption and epoch interruption.
pub struct WasmSandbox {
    engine: Engine,
    epoch_tick: Duration,
    max_wasm_stack_bytes: NonZeroUsize,
    stop_ticker: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

impl WasmSandbox {
    /// Creates a sandbox with an explicit guest stack limit and starts its ticker.
    ///
    /// The tick controls deadline resolution. A shorter tick improves precision
    /// but wakes the ticker more frequently. The stack limit is approximate and
    /// applies to WebAssembly execution across every store using this engine.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxCreationError::ZeroEpochTick`] for a zero tick,
    /// [`SandboxCreationError::Engine`] when Wasmtime rejects the configuration,
    /// or [`SandboxCreationError::Ticker`] when the ticker thread cannot start.
    pub fn new(
        epoch_tick: Duration,
        max_wasm_stack_bytes: NonZeroUsize,
    ) -> Result<Self, SandboxCreationError> {
        if epoch_tick.is_zero() {
            return Err(SandboxCreationError::ZeroEpochTick);
        }

        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        config.max_wasm_stack(max_wasm_stack_bytes.get());
        config.wasm_memory64(false);
        let engine = Engine::new(&config).map_err(SandboxCreationError::Engine)?;
        let stop_ticker = Arc::new(AtomicBool::new(false));
        let ticker_engine = engine.clone();
        let ticker_stop = Arc::clone(&stop_ticker);
        let ticker = thread::Builder::new()
            .name("fleetiq-wasm-epoch".to_owned())
            .spawn(move || {
                loop {
                    thread::sleep(epoch_tick);
                    if ticker_stop.load(Ordering::Acquire) {
                        break;
                    }
                    ticker_engine.increment_epoch();
                }
            })
            .map_err(SandboxCreationError::Ticker)?;

        Ok(Self {
            engine,
            epoch_tick,
            max_wasm_stack_bytes,
            stop_ticker,
            ticker: Some(ticker),
        })
    }

    /// Returns the approximate guest stack limit configured for this sandbox.
    pub const fn max_wasm_stack_bytes(&self) -> NonZeroUsize {
        self.max_wasm_stack_bytes
    }

    pub(crate) fn admit_module(
        &self,
        wasm: &[u8],
        policy: ModuleAdmissionPolicy,
    ) -> Result<Module, ModuleAdmissionError> {
        let actual = wasm.len();
        let limit = policy.max_module_bytes().get();
        if actual > limit {
            return Err(ModuleAdmissionError::ModuleTooLarge { limit, actual });
        }

        let module = Module::new(&self.engine, wasm).map_err(ModuleAdmissionError::Compilation)?;
        let import_count = module.imports().len();
        if import_count > 0 {
            return Err(ModuleAdmissionError::HostImportsNotAllowed {
                count: import_count,
            });
        }

        let table_count = module.resources_required().num_tables;
        if table_count > MAX_TABLES_PER_MODULE {
            return Err(ModuleAdmissionError::TooManyTables {
                limit: MAX_TABLES_PER_MODULE,
                actual: table_count,
            });
        }

        Ok(module)
    }

    pub(crate) fn prepare_store(
        &self,
        budget: InvocationBudget,
    ) -> Result<Store<InvocationLimiter>, InvocationError> {
        let mut store = Store::new(
            &self.engine,
            InvocationLimiter::new(
                budget.max_linear_memory_bytes().get(),
                budget.max_table_elements().get(),
            ),
        );
        store.limiter(|limiter| limiter);
        store
            .set_fuel(budget.fuel().get())
            .map_err(InvocationError::Runtime)?;
        store.epoch_deadline_trap();
        store.set_epoch_deadline(self.deadline_ticks(budget.wall_time()));
        Ok(store)
    }

    pub(crate) fn complete_invocation<T>(
        &self,
        store: &Store<InvocationLimiter>,
        budget: InvocationBudget,
        result: Result<T, wasmtime::Error>,
    ) -> Result<(T, InvocationReport), InvocationError> {
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                if let Some(attempted) = store.data().denied_memory_size {
                    return Err(InvocationError::LinearMemoryLimitExceeded {
                        limit: budget.max_linear_memory_bytes().get(),
                        attempted,
                    });
                }
                if let Some(attempted) = store.data().denied_table_elements {
                    return Err(InvocationError::TableElementLimitExceeded {
                        limit: budget.max_table_elements().get(),
                        attempted,
                    });
                }
                return Err(classify_invocation_error(
                    error,
                    self.max_wasm_stack_bytes.get(),
                ));
            }
        };

        let remaining = store.get_fuel().map_err(InvocationError::Runtime)?;
        Ok((
            value,
            InvocationReport {
                fuel_consumed: budget.fuel().get().saturating_sub(remaining),
            },
        ))
    }

    fn deadline_ticks(&self, wall_time: Duration) -> u64 {
        let ticks = wall_time.as_nanos().div_ceil(self.epoch_tick.as_nanos());
        u64::try_from(ticks).unwrap_or(u64::MAX).max(1)
    }
}

pub(crate) struct InvocationLimiter {
    max_linear_memory_bytes: usize,
    max_table_elements: usize,
    denied_memory_size: Option<usize>,
    denied_table_elements: Option<usize>,
}

impl InvocationLimiter {
    const fn new(max_linear_memory_bytes: usize, max_table_elements: usize) -> Self {
        Self {
            max_linear_memory_bytes,
            max_table_elements,
            denied_memory_size: None,
            denied_table_elements: None,
        }
    }
}

impl ResourceLimiter for InvocationLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        if desired > self.max_linear_memory_bytes {
            self.denied_memory_size = Some(desired);
            return Err(wasmtime::Error::msg("guest linear-memory limit exceeded"));
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool, wasmtime::Error> {
        if desired > self.max_table_elements {
            self.denied_table_elements = Some(desired);
            return Err(wasmtime::Error::msg("guest table-element limit exceeded"));
        }
        Ok(true)
    }

    fn tables(&self) -> usize {
        usize::try_from(MAX_TABLES_PER_MODULE).expect("the table count limit fits usize")
    }

    fn memories(&self) -> usize {
        1
    }

    fn instances(&self) -> usize {
        1
    }
}

impl fmt::Debug for WasmSandbox {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WasmSandbox")
            .field("epoch_tick", &self.epoch_tick)
            .field("max_wasm_stack_bytes", &self.max_wasm_stack_bytes)
            .finish_non_exhaustive()
    }
}

impl Drop for WasmSandbox {
    fn drop(&mut self) {
        self.stop_ticker.store(true, Ordering::Release);
        if let Some(ticker) = self.ticker.take() {
            let _ = ticker.join();
        }
    }
}

/// Resource usage reported after a successful probe invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvocationReport {
    fuel_consumed: u64,
}

impl InvocationReport {
    /// Returns the Wasmtime fuel consumed by instantiation and execution.
    pub const fn fuel_consumed(&self) -> u64 {
        self.fuel_consumed
    }
}
