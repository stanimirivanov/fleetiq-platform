use std::{convert::Infallible, fmt};

use wasmtime::{Instance, Store};

use super::{
    CompiledTransferModule, GuestMemoryRegion, OpaqueTransferError, OpaqueTransferLimits,
    OpaqueTransferOutput,
};
use crate::{InvocationBudget, WasmSandbox, sandbox::InvocationLimiter};

impl WasmSandbox {
    /// Invokes an admitted opaque-transfer module in a fresh bounded instance.
    ///
    /// Input is copied into the range returned by the guest allocator. The
    /// processor returns an output pointer and length packed into one `i64`.
    /// Every range is validated against current memory before access. Dropping
    /// the fresh store and instance reclaims all guest allocation on every path.
    ///
    /// # Errors
    ///
    /// Returns [`OpaqueTransferError`] when a transfer limit or guest memory
    /// range is invalid, host output allocation fails, or bounded WebAssembly
    /// invocation fails.
    pub fn invoke_transfer(
        &self,
        module: &CompiledTransferModule,
        budget: InvocationBudget,
        limits: OpaqueTransferLimits,
        input: &[u8],
    ) -> Result<OpaqueTransferOutput, OpaqueTransferError> {
        if input.len() > limits.input_bytes.get() as usize {
            return Err(OpaqueTransferError::InputTooLarge {
                limit: limits.input_bytes.get(),
                actual: input.len(),
            });
        }

        let mut store = self
            .prepare_store(budget)
            .map_err(OpaqueTransferError::Invocation)?;
        let operation = perform_transfer(&mut store, module, limits, input);

        match operation {
            Ok(bytes) => {
                let (bytes, report) = self
                    .complete_invocation(&store, budget, Ok(bytes))
                    .map_err(OpaqueTransferError::Invocation)?;
                Ok(OpaqueTransferOutput { bytes, report })
            }
            Err(TransferStepError::Boundary(error)) => Err(error),
            Err(TransferStepError::Runtime(error)) => {
                match self.complete_invocation::<Infallible>(&store, budget, Err(error)) {
                    Err(error) => Err(OpaqueTransferError::Invocation(error)),
                    Ok((never, _)) => match never {},
                }
            }
        }
    }
}

fn perform_transfer(
    store: &mut Store<InvocationLimiter>,
    module: &CompiledTransferModule,
    limits: OpaqueTransferLimits,
    input: &[u8],
) -> Result<Box<[u8]>, TransferStepError> {
    let instance =
        Instance::new(&mut *store, &module.module, &[]).map_err(TransferStepError::Runtime)?;
    let memory = instance
        .get_module_export(&mut *store, &module.memory)
        .and_then(wasmtime::Extern::into_memory)
        .ok_or_else(|| runtime_error("admitted transfer memory is unavailable"))?;
    let allocate = instance
        .get_module_export(&mut *store, &module.allocate)
        .and_then(wasmtime::Extern::into_func)
        .ok_or_else(|| runtime_error("admitted transfer allocator is unavailable"))?
        .typed::<i32, i32>(&*store)
        .map_err(TransferStepError::Runtime)?;
    let process = instance
        .get_module_export(&mut *store, &module.process)
        .and_then(wasmtime::Extern::into_func)
        .ok_or_else(|| runtime_error("admitted transfer processor is unavailable"))?
        .typed::<(i32, i32), i64>(&*store)
        .map_err(TransferStepError::Runtime)?;

    let input_length = u32::try_from(input.len()).map_err(|_| {
        TransferStepError::Boundary(OpaqueTransferError::InputTooLarge {
            limit: limits.input_bytes.get(),
            actual: input.len(),
        })
    })?;
    let input_pointer = allocate
        .call(&mut *store, input_length.cast_signed())
        .map(i32::cast_unsigned)
        .map_err(TransferStepError::Runtime)?;
    let input_offset = validate_range(
        GuestMemoryRegion::Input,
        input_pointer,
        input_length,
        memory.data_size(&*store),
    )?;
    memory
        .write(&mut *store, input_offset, input)
        .map_err(|error| runtime_error(error.to_string()))?;

    let descriptor = process
        .call(
            &mut *store,
            (input_pointer.cast_signed(), input_length.cast_signed()),
        )
        .map(i64::cast_unsigned)
        .map_err(TransferStepError::Runtime)?
        .to_be_bytes();
    let output_pointer = u32::from_be_bytes(
        descriptor[..4]
            .try_into()
            .map_err(|_| runtime_error("opaque transfer descriptor has no pointer field"))?,
    );
    let output_length = u32::from_be_bytes(
        descriptor[4..]
            .try_into()
            .map_err(|_| runtime_error("opaque transfer descriptor has no length field"))?,
    );
    if output_length > limits.output_bytes.get() {
        return Err(TransferStepError::Boundary(
            OpaqueTransferError::OutputTooLarge {
                limit: limits.output_bytes.get(),
                actual: output_length,
            },
        ));
    }
    let output_offset = validate_range(
        GuestMemoryRegion::Output,
        output_pointer,
        output_length,
        memory.data_size(&*store),
    )?;
    let output_size = output_length as usize;
    let mut output = Vec::new();
    output.try_reserve_exact(output_size).map_err(|_| {
        TransferStepError::Boundary(OpaqueTransferError::HostOutputAllocationFailed {
            requested: output_length,
        })
    })?;
    output.resize(output_size, 0);
    memory
        .read(&*store, output_offset, &mut output)
        .map_err(|error| runtime_error(error.to_string()))?;
    Ok(output.into_boxed_slice())
}

fn validate_range(
    region: GuestMemoryRegion,
    offset: u32,
    length: u32,
    memory_size: usize,
) -> Result<usize, TransferStepError> {
    let end = u64::from(offset) + u64::from(length);
    if end > memory_size as u64 {
        return Err(TransferStepError::Boundary(
            OpaqueTransferError::GuestMemoryOutOfBounds {
                region,
                offset,
                length,
                memory_size,
            },
        ));
    }
    usize::try_from(offset).map_err(|_| {
        TransferStepError::Boundary(OpaqueTransferError::GuestMemoryOutOfBounds {
            region,
            offset,
            length,
            memory_size,
        })
    })
}

fn runtime_error(message: impl fmt::Display) -> TransferStepError {
    TransferStepError::Runtime(wasmtime::Error::msg(message.to_string()))
}

enum TransferStepError {
    Boundary(OpaqueTransferError),
    Runtime(wasmtime::Error),
}
