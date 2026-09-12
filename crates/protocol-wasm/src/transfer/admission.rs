use wasmtime::{ExternType, Module, ModuleExport, ValType};

use super::{CompiledTransferModule, TransferExportNames};
use crate::{ModuleAdmissionError, ModuleAdmissionPolicy, TransferExportRole, WasmSandbox};

impl WasmSandbox {
    /// Admits a module and binds the selected opaque-transfer exports.
    ///
    /// This experimental contract transfers uninterpreted bytes. The selected
    /// names and function shapes are not the stable protocol-package ABI.
    ///
    /// # Errors
    ///
    /// Returns [`ModuleAdmissionError`] when ordinary module admission fails or
    /// a selected export is missing, has the wrong external kind, or exposes an
    /// incompatible core WebAssembly signature.
    pub fn compile_transfer(
        &self,
        wasm: &[u8],
        policy: ModuleAdmissionPolicy,
        exports: TransferExportNames<'_>,
    ) -> Result<CompiledTransferModule, ModuleAdmissionError> {
        let module = self.admit_module(wasm, policy)?;
        let memory = bind_memory(&module, exports.memory)?;
        let allocate = bind_function(
            &module,
            exports.allocate,
            TransferExportRole::Allocate,
            &[ValType::I32],
            &[ValType::I32],
        )?;
        let process = bind_function(
            &module,
            exports.process,
            TransferExportRole::Process,
            &[ValType::I32, ValType::I32],
            &[ValType::I64],
        )?;

        Ok(CompiledTransferModule {
            module,
            memory,
            allocate,
            process,
        })
    }
}

fn bind_memory(module: &Module, export: &str) -> Result<ModuleExport, ModuleAdmissionError> {
    let role = TransferExportRole::Memory;
    let export_type =
        module
            .get_export(export)
            .ok_or_else(|| ModuleAdmissionError::TransferExportMissing {
                role,
                export: export.into(),
            })?;
    if !matches!(export_type, ExternType::Memory(_)) {
        return Err(ModuleAdmissionError::TransferExportKindMismatch {
            role,
            export: export.into(),
        });
    }
    module
        .get_export_index(export)
        .ok_or_else(|| ModuleAdmissionError::TransferExportMissing {
            role,
            export: export.into(),
        })
}

fn bind_function(
    module: &Module,
    export: &str,
    role: TransferExportRole,
    expected_parameters: &[ValType],
    expected_results: &[ValType],
) -> Result<ModuleExport, ModuleAdmissionError> {
    let export_type =
        module
            .get_export(export)
            .ok_or_else(|| ModuleAdmissionError::TransferExportMissing {
                role,
                export: export.into(),
            })?;
    let ExternType::Func(function_type) = export_type else {
        return Err(ModuleAdmissionError::TransferExportKindMismatch {
            role,
            export: export.into(),
        });
    };
    let parameters = function_type.params().collect::<Vec<_>>();
    let results = function_type.results().collect::<Vec<_>>();
    if !same_types(&parameters, expected_parameters) || !same_types(&results, expected_results) {
        return Err(ModuleAdmissionError::TransferExportSignatureMismatch {
            role,
            export: export.into(),
            actual: format!("{parameters:?} -> {results:?}").into(),
        });
    }
    module
        .get_export_index(export)
        .ok_or_else(|| ModuleAdmissionError::TransferExportMissing {
            role,
            export: export.into(),
        })
}

fn same_types(actual: &[ValType], expected: &[ValType]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| ValType::eq(actual, expected))
}
