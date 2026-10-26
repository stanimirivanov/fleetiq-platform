//! Trusted replay policy version one; transport replies are discarded here.
use crate::{
    ComponentAdmissionPolicy, InvocationBudget, ProtocolSessionTurn, ProtocolSessionTurnBudgets,
    WasmSandbox,
};
use fleetiq_protocol_host::{
    FrameProcessingGate, FrameProcessingLimits, SessionInputBuffer, SessionState,
};
use fleetiq_protocol_sdk::{
    SessionStateAction,
    replay_wire::{ReplayOutput, ReplayRequest, ReplayResponse},
};
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use std::{
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};
fn nz(v: usize) -> NonZeroUsize {
    NonZeroUsize::new(v).expect("fixed policy limits are nonzero")
}
/// Executes one exact frame using bounded pre-state and import-free admission.
/// # Errors
/// Returns [`ReplayExecutionError::Rejected`] for unsupported input, framing,
/// admission, budgets or host validation; [`ReplayExecutionError::Unavailable`]
/// for an incompatible execution identity or host startup failure. No proposed
/// reply is returned or delivered.
pub fn replay_frame(request: ReplayRequest) -> Result<ReplayResponse, ReplayExecutionError> {
    if request.execution != fleetiq_protocol_sdk::replay_wire::replay_fingerprint() {
        return Err(ReplayExecutionError::Unavailable);
    }
    if request.policy != 1
        || request.artifact.len() > 8 * 1024 * 1024
        || request.artifact.is_empty()
        || request.frame.len() > 65536
        || request.state.len() > 16384
    {
        return Err(ReplayExecutionError::Rejected);
    }
    let package = ProtocolPackageRef::new(
        ProtocolPackageId::new(request.package_id).map_err(|_| ReplayExecutionError::Rejected)?,
        ProtocolPackageVersion::new(request.package_version)
            .map_err(|_| ReplayExecutionError::Rejected)?,
    );
    let sandbox = WasmSandbox::for_replay().map_err(|_| ReplayExecutionError::Unavailable)?;
    let policy = ComponentAdmissionPolicy::new(nz(8 * 1024 * 1024));
    let component = match request.world.as_str() {
        "runtime-v1" => sandbox
            .admit_protocol_component(package, &request.artifact, policy)
            .map_err(|_| ReplayExecutionError::Rejected)?,
        "identity-v1" => sandbox
            .admit_identity_protocol_component(package, &request.artifact, policy)
            .map_err(|_| ReplayExecutionError::Rejected)?
            .protocol()
            .clone(),
        _ => return Err(ReplayExecutionError::Rejected),
    };
    let budget = || {
        InvocationBudget::new(
            NonZeroU64::new(50_000_000).ok_or(ReplayExecutionError::Rejected)?,
            Duration::from_secs(5),
            nz(16 * 1024 * 1024),
            nz(1000),
        )
        .map_err(|_| ReplayExecutionError::Rejected)
    };
    let mut state = SessionState::new(nz(16384));
    state
        .update::<()>(|_| {
            Ok(SessionStateAction::Replace(
                request.state.into_boxed_slice(),
            ))
        })
        .map_err(|_| ReplayExecutionError::Rejected)?;
    let mut input = SessionInputBuffer::new(nz(65536));
    input
        .push(&request.frame)
        .map_err(|_| ReplayExecutionError::Rejected)?;
    let gate = FrameProcessingGate::new(FrameProcessingLimits::new(
        nz(1200),
        nz(16),
        nz(4096),
        nz(16384),
    ));
    let ProtocolSessionTurn::Processed(turn) = sandbox
        .run_component_session_turn(
            &component,
            ProtocolSessionTurnBudgets::new(budget()?, budget()?),
            &gate,
            &mut state,
            &mut input,
        )
        .map_err(|_| ReplayExecutionError::Rejected)?
    else {
        return Err(ReplayExecutionError::Rejected);
    };
    if !input.as_bytes().is_empty() || turn.frame().as_bytes() != request.frame {
        return Err(ReplayExecutionError::Rejected);
    }
    // Ready and deferred replies die with `turn`; recovery has no socket handle.
    Ok(ReplayResponse {
        policy: 1,
        execution: request.execution,
        state: state.as_bytes().to_vec(),
        outputs: turn
            .output()
            .decoded_outputs()
            .iter()
            .map(ReplayOutput::from_output)
            .collect(),
    })
}
/// Closed replay failure; guest diagnostic trees never cross IPC.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayExecutionError {
    /// Input, admission, execution budget or validated output was rejected.
    Rejected,
    /// Host engine/ticker startup failed; retain the lease for a later attempt.
    Unavailable,
}
impl std::fmt::Display for ReplayExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rejected => "protocol replay rejected",
            Self::Unavailable => "protocol replay runtime unavailable",
        })
    }
}
impl std::error::Error for ReplayExecutionError {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incompatible_runtime_is_retryable_before_parsing_guest_bytes() {
        let request = ReplayRequest {
            policy: 1,
            execution: [0; 32],
            package_id: String::new(),
            package_version: String::new(),
            world: String::new(),
            artifact: vec![],
            frame: vec![],
            state: vec![],
        };
        assert!(matches!(
            replay_frame(request),
            Err(ReplayExecutionError::Unavailable)
        ));
    }
}
