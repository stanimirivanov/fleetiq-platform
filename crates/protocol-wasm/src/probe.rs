use std::fmt;

use wasmtime::{ExternType, Instance, Module, ModuleExport};

use crate::{
    InvocationBudget, InvocationError, InvocationReport, ModuleAdmissionError,
    ModuleAdmissionPolicy, WasmSandbox,
};

/// Core WebAssembly module compiled and bound to an admitted probe export.
#[derive(Clone)]
pub struct CompiledModule {
    module: Module,
    probe_export: ModuleExport,
}

impl fmt::Debug for CompiledModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompiledModule")
            .finish_non_exhaustive()
    }
}

impl WasmSandbox {
    /// Admits and compiles a module with its temporary probe entry point.
    ///
    /// The raw-byte limit is checked before Wasmtime parses or compiles the
    /// module. Compilation remains outside the per-invocation budget and must be
    /// isolated from message-processing capacity by the future activation flow.
    ///
    /// # Errors
    ///
    /// Returns [`ModuleAdmissionError::ModuleTooLarge`] before compilation when
    /// the input exceeds policy, [`ModuleAdmissionError::Compilation`] when
    /// Wasmtime rejects the input, or
    /// [`ModuleAdmissionError::HostImportsNotAllowed`] when the valid module
    /// requests capabilities unavailable in this runtime, or
    /// [`ModuleAdmissionError::TooManyTables`] when it cannot fit the runtime's
    /// aggregate table-element policy. Missing or incompatible probe exports are
    /// also rejected before this method returns a [`CompiledModule`].
    pub fn compile(
        &self,
        wasm: &[u8],
        policy: ModuleAdmissionPolicy,
        probe_export: &str,
    ) -> Result<CompiledModule, ModuleAdmissionError> {
        let module = self.admit_module(wasm, policy)?;

        let export_type = module.get_export(probe_export).ok_or_else(|| {
            ModuleAdmissionError::ProbeExportMissing {
                export: probe_export.into(),
            }
        })?;
        let ExternType::Func(function_type) = export_type else {
            return Err(ModuleAdmissionError::ProbeExportNotFunction {
                export: probe_export.into(),
            });
        };
        let parameters = function_type.params().len();
        let results = function_type.results().len();
        if parameters > 0 || results > 0 {
            return Err(ModuleAdmissionError::ProbeExportSignatureMismatch {
                export: probe_export.into(),
                parameters,
                results,
            });
        }
        let probe_export = module.get_export_index(probe_export).ok_or_else(|| {
            ModuleAdmissionError::ProbeExportMissing {
                export: probe_export.into(),
            }
        })?;

        Ok(CompiledModule {
            module,
            probe_export,
        })
    }

    /// Invokes a temporary no-argument probe export under its resource budget.
    ///
    /// This export shape exists only to verify runtime interruption. It is not
    /// the stable protocol package ABI.
    ///
    /// # Errors
    ///
    /// Returns [`InvocationError::FuelExhausted`] when deterministic fuel runs
    /// out, [`InvocationError::DeadlineExceeded`] when the epoch deadline traps,
    /// [`InvocationError::LinearMemoryLimitExceeded`] when guest allocation
    /// crosses its byte limit, [`InvocationError::TableElementLimitExceeded`]
    /// when guest table allocation crosses its element limit, or
    /// [`InvocationError::StackLimitExceeded`] when nested guest calls cross the
    /// sandbox stack limit, or [`InvocationError::Runtime`] for another
    /// instantiation, export, or guest failure.
    pub fn invoke_probe(
        &self,
        module: &CompiledModule,
        budget: InvocationBudget,
    ) -> Result<InvocationReport, InvocationError> {
        let mut store = self.prepare_store(budget)?;

        let result = (|| {
            let instance = Instance::new(&mut store, &module.module, &[])?;
            let function = instance
                .get_module_export(&mut store, &module.probe_export)
                .and_then(wasmtime::Extern::into_func)
                .ok_or_else(|| wasmtime::Error::msg("admitted probe export is unavailable"))?
                .typed::<(), ()>(&store)?;
            function.call(&mut store, ())
        })();

        let ((), report) = self.complete_invocation(&store, budget, result)?;
        Ok(report)
    }
}
