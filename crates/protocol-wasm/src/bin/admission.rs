//! Isolated admission worker. The parent supplies bounded bytes on stdin only.
//! No provider/database environment, package-controlled paths or native caches
//! are accepted. Exit 0 means typed import-free admission; 2 is rejection and
//! 78 means isolation/startup failure. This process never invokes guest exports.
use std::process::ExitCode;
fn main() -> ExitCode {
    ExitCode::from(run())
}
const MAX_BYTES: usize = 8 * 1024 * 1024;
fn run() -> u8 {
    use fleetiq_protocol_wasm::{ComponentAdmissionPolicy, WasmSandbox};
    use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
    use std::{io::Read, num::NonZeroUsize, time::Duration};
    if !install_limits() {
        return 78;
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 || !matches!(args[0].as_str(), "runtime-v1" | "identity-v1") {
        return 78;
    }
    let mut bytes = Vec::new();
    if std::io::stdin()
        .lock()
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.is_empty()
        || bytes.len() > MAX_BYTES
    {
        return 2;
    }
    let Ok(sandbox) = WasmSandbox::new(
        Duration::from_millis(10),
        NonZeroUsize::new(64 * 1024).expect("nonzero"),
    ) else {
        return 78;
    };
    let package = ProtocolPackageRef::new(
        ProtocolPackageId::new("admission.validation").expect("fixed ID"),
        ProtocolPackageVersion::new("1").expect("fixed version"),
    );
    let policy = ComponentAdmissionPolicy::new(NonZeroUsize::new(MAX_BYTES).expect("nonzero"));
    let admitted = if args[0] == "identity-v1" {
        sandbox
            .admit_identity_protocol_component(package, &bytes, policy)
            .is_ok()
    } else {
        sandbox
            .admit_protocol_component(package, &bytes, policy)
            .is_ok()
    };
    if admitted { 0 } else { 2 }
}

#[cfg(not(target_os = "linux"))]
fn install_limits() -> bool {
    false
}
#[cfg(target_os = "linux")]
fn install_limits() -> bool {
    // Per-child virtual-address and aggregate CPU ceilings, not reservations.
    // Apply them before reading/parsing untrusted bytes; never emit core dumps.
    [
        (rlimit::Resource::CORE, 0),
        (rlimit::Resource::AS, 2 * 1024 * 1024 * 1024),
        (rlimit::Resource::CPU, 10),
    ]
    .into_iter()
    .all(|(resource, ceiling)| resource.set(ceiling, ceiling).is_ok())
}
