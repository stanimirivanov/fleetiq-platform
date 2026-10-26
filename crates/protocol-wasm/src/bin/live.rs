//! Isolated live child: bounded private stdin/stdout only, no environment.
use fleetiq_protocol_sdk::live_wire::LiveRequest;
use fleetiq_protocol_sdk::replay_wire::{REPLAY_REQUEST_LIMIT, REPLAY_RESPONSE_LIMIT};
use std::{
    io::{Read, Write},
    process::ExitCode,
};
fn main() -> ExitCode {
    ExitCode::from(run())
}
fn run() -> u8 {
    if !install_limits() || std::env::args_os().len() != 1 {
        return 78;
    }
    let mut bytes = Vec::new();
    if std::io::stdin()
        .lock()
        .take(REPLAY_REQUEST_LIMIT + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > REPLAY_REQUEST_LIMIT
    {
        return 2;
    }
    let Ok(request) = serde_json::from_slice::<LiveRequest>(&bytes) else {
        return 2;
    };
    let response = match fleetiq_protocol_wasm::live_turn(request) {
        Ok(response) => response,
        Err(fleetiq_protocol_wasm::ReplayExecutionError::Rejected) => return 2,
        Err(fleetiq_protocol_wasm::ReplayExecutionError::Unavailable) => return 78,
    };
    let Ok(encoded) = serde_json::to_vec(&response) else {
        return 2;
    };
    if encoded.len() as u64 > REPLAY_RESPONSE_LIMIT {
        return 2;
    }
    if std::io::stdout().lock().write_all(&encoded).is_err() {
        return 78;
    }
    0
}
#[cfg(not(target_os = "linux"))]
fn install_limits() -> bool {
    false
}
#[cfg(target_os = "linux")]
fn install_limits() -> bool {
    [
        (rlimit::Resource::CORE, 0),
        (rlimit::Resource::AS, 2 * 1024 * 1024 * 1024),
        (rlimit::Resource::CPU, 10),
    ]
    .into_iter()
    .all(|(r, n)| r.set(n, n).is_ok())
}
