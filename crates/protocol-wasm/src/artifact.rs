use std::num::NonZeroUsize;

/// Host-owned admission policy for one core WebAssembly module.
///
/// This policy is applied before untrusted bytes reach Wasmtime. It limits the
/// raw module input rather than the package manifest, signatures, compiled code,
/// or the memory used while compiling the module.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleAdmissionPolicy {
    max_module_bytes: NonZeroUsize,
}

impl ModuleAdmissionPolicy {
    /// Creates a module admission policy with a nonzero raw-byte limit.
    pub const fn new(max_module_bytes: NonZeroUsize) -> Self {
        Self { max_module_bytes }
    }

    /// Returns the maximum accepted core WebAssembly module size in bytes.
    pub const fn max_module_bytes(self) -> NonZeroUsize {
        self.max_module_bytes
    }
}
