//! Compile-time bindings for the versioned protocol-package component contract.

mod conversion;

/// Semantic version of the protocol-package WIT package implemented here.
///
/// Package releases declare compatibility with this interface version. Before
/// `1.0.0`, incompatible changes advance the minor version; after `1.0.0`, they
/// advance the major version.
pub const PROTOCOL_PACKAGE_ABI_VERSION: &str = "0.1.0";

/// Fully qualified WIT world implemented by a protocol package.
pub const PROTOCOL_PACKAGE_WORLD: &str = "fleetiq:protocol-package/protocol-package@0.1.0";

/// Authoritative WIT source for the current protocol-package interface.
///
/// Tooling may publish this document with a package SDK. Runtime compatibility
/// still comes from the component's embedded world type during admission.
pub const PROTOCOL_PACKAGE_WIT: &str = include_str!("../wit/protocol-package.wit");

// Generating typed host bindings as part of the normal build makes the checked-in
// WIT document executable specification: invalid syntax or types fail compilation.
// The bindings remain private until component admission and invocation establish
// the runtime policy for exposing them.
mod bindings {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "protocol-package",
    });
}

#[cfg(test)]
mod tests;
