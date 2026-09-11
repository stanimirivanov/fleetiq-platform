use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use wasmtime::{Config, Engine, Instance, Module, Store};

use crate::{
    InvocationBudget, InvocationError, SandboxCreationError, error::classify_invocation_error,
};

/// WebAssembly compiled once during package validation or activation.
#[derive(Clone)]
pub struct CompiledModule(Module);

impl fmt::Debug for CompiledModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompiledModule")
            .finish_non_exhaustive()
    }
}

/// Host runtime configured for fuel consumption and epoch interruption.
pub struct WasmSandbox {
    engine: Engine,
    epoch_tick: Duration,
    stop_ticker: Arc<AtomicBool>,
    ticker: Option<JoinHandle<()>>,
}

impl WasmSandbox {
    /// Creates a sandbox and starts its shared epoch ticker.
    ///
    /// The tick controls deadline resolution. A shorter tick improves precision
    /// but wakes the ticker more frequently.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxCreationError::ZeroEpochTick`] for a zero tick,
    /// [`SandboxCreationError::Engine`] when Wasmtime rejects the configuration,
    /// or [`SandboxCreationError::Ticker`] when the ticker thread cannot start.
    pub fn new(epoch_tick: Duration) -> Result<Self, SandboxCreationError> {
        if epoch_tick.is_zero() {
            return Err(SandboxCreationError::ZeroEpochTick);
        }

        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
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
            stop_ticker,
            ticker: Some(ticker),
        })
    }

    /// Compiles validated WebAssembly bytes for later bounded invocations.
    ///
    /// Compilation is intentionally outside the per-invocation budget. Package
    /// validation and activation must bound compilation concurrency and artifact
    /// size separately.
    ///
    /// # Errors
    ///
    /// Returns the Wasmtime validation or compilation error.
    pub fn compile(&self, wasm: &[u8]) -> Result<CompiledModule, wasmtime::Error> {
        Module::new(&self.engine, wasm).map(CompiledModule)
    }

    /// Invokes a temporary no-argument probe export under both compute limits.
    ///
    /// This export shape exists only to verify runtime interruption. It is not
    /// the stable protocol package ABI.
    ///
    /// # Errors
    ///
    /// Returns [`InvocationError::FuelExhausted`] when deterministic fuel runs
    /// out, [`InvocationError::DeadlineExceeded`] when the epoch deadline traps,
    /// or [`InvocationError::Runtime`] for another instantiation, export, or guest
    /// failure.
    pub fn invoke_probe(
        &self,
        module: &CompiledModule,
        export: &str,
        budget: InvocationBudget,
    ) -> Result<InvocationReport, InvocationError> {
        let mut store = Store::new(&self.engine, ());
        store
            .set_fuel(budget.fuel().get())
            .map_err(InvocationError::Runtime)?;
        store.epoch_deadline_trap();
        store.set_epoch_deadline(self.deadline_ticks(budget.wall_time()));

        let result = (|| {
            let instance = Instance::new(&mut store, &module.0, &[])?;
            let function = instance.get_typed_func::<(), ()>(&mut store, export)?;
            function.call(&mut store, ())
        })();

        if let Err(error) = result {
            return Err(classify_invocation_error(error));
        }

        let remaining = store.get_fuel().map_err(InvocationError::Runtime)?;
        Ok(InvocationReport {
            fuel_consumed: budget.fuel().get().saturating_sub(remaining),
        })
    }

    fn deadline_ticks(&self, wall_time: Duration) -> u64 {
        let ticks = wall_time.as_nanos().div_ceil(self.epoch_tick.as_nanos());
        u64::try_from(ticks).unwrap_or(u64::MAX).max(1)
    }
}

impl fmt::Debug for WasmSandbox {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WasmSandbox")
            .field("epoch_tick", &self.epoch_tick)
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
