//! Live-child execution. Recovery retains its separate reply-free envelope.
use crate::{
    ComponentAdmissionPolicy, InvocationBudget, ProtocolSessionTurn, ProtocolSessionTurnBudgets,
    ReplayExecutionError, WasmSandbox,
};
use fleetiq_protocol_host::{
    FrameProcessingGate, FrameProcessingLimits, SessionInputBuffer, SessionState, StagedReply,
};
use fleetiq_protocol_sdk::{
    FramingInput, ReplyRelease, SessionStateAction,
    live_wire::{LiveReply, LiveRequest, LiveResponse, LiveTurn},
    replay_wire::ReplayOutput,
};
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use std::{
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};
fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("fixed nonzero policy")
}
fn budget() -> Result<InvocationBudget, ReplayExecutionError> {
    InvocationBudget::new(
        NonZeroU64::new(50_000_000).expect("fixed fuel"),
        Duration::from_secs(5),
        nz(16 * 1024 * 1024),
        nz(1000),
    )
    .map_err(|_| ReplayExecutionError::Rejected)
}
/// Frames and processes one prefix with the admitted identity-v1 component.
/// # Errors
/// Rejects limits, identity, admission or execution failures without guest text.
/// Incomplete framing returns no identity, state, output or reply proposals.
pub fn live_turn(request: LiveRequest) -> Result<LiveResponse, ReplayExecutionError> {
    use ReplayExecutionError::{Rejected, Unavailable};
    if request.execution != fleetiq_protocol_sdk::replay_wire::replay_fingerprint() {
        return Err(Unavailable);
    }
    if request.policy != 1
        || request.artifact.is_empty()
        || request.artifact.len() > 8 * 1024 * 1024
        || request.unread.is_empty()
        || request.unread.len() > 65536
        || request.state.len() > 16384
        || (request.identify && !request.state.is_empty())
    {
        return Err(Rejected);
    }
    let package = ProtocolPackageRef::new(
        ProtocolPackageId::new(request.package_id).map_err(|_| Rejected)?,
        ProtocolPackageVersion::new(request.package_version).map_err(|_| Rejected)?,
    );
    let sandbox = WasmSandbox::for_replay().map_err(|_| Unavailable)?;
    let component = sandbox
        .admit_identity_protocol_component(
            package,
            &request.artifact,
            ComponentAdmissionPolicy::new(nz(8 * 1024 * 1024)),
        )
        .map_err(|_| Rejected)?;
    let mut state = SessionState::new(nz(16384));
    state
        .update::<()>(|_| {
            Ok(SessionStateAction::Replace(
                request.state.clone().into_boxed_slice(),
            ))
        })
        .map_err(|_| Rejected)?;
    let mut input = SessionInputBuffer::new(nz(65536));
    input.push(&request.unread).map_err(|_| Rejected)?;
    let gate = FrameProcessingGate::new(FrameProcessingLimits::new(
        nz(1200),
        nz(16),
        nz(4096),
        nz(16384),
    ));
    let result = sandbox
        .run_component_session_turn(
            component.protocol(),
            ProtocolSessionTurnBudgets::new(budget()?, budget()?),
            &gate,
            &mut state,
            &mut input,
        )
        .map_err(|_| Rejected)?;
    let ProtocolSessionTurn::Processed(turn) = result else {
        return Ok(LiveResponse {
            execution: request.execution,
            turn: None,
        });
    };
    let identity = if request.identify {
        let (claim, _) = sandbox
            .identify_component_device(
                &component,
                budget()?,
                FramingInput::new(turn.frame().as_bytes(), &request.state),
                nz(65536 + 16384),
            )
            .map_err(|_| Rejected)?;
        Some((claim.kind().to_owned(), claim.value().to_owned()))
    } else {
        None
    };
    let consumed = turn.frame().as_bytes().len();
    let (_, _, _, output, _, _) = turn.into_parts();
    let (outputs, replies) = output.into_parts();
    let replies = replies.into_iter().map(proposal).collect();
    Ok(LiveResponse {
        execution: request.execution,
        turn: Some(LiveTurn {
            consumed,
            identity,
            state: state.as_bytes().to_vec(),
            outputs: outputs.iter().map(ReplayOutput::from_output).collect(),
            replies,
        }),
    })
}

fn proposal(reply: StagedReply) -> LiveReply {
    match reply {
        StagedReply::Ready(reply) => LiveReply {
            bytes: reply.into_bytes().into_vec(),
            durable: false,
        },
        StagedReply::AwaitingDurableAcceptance(reply) => {
            let (bytes, release) = reply.into_proposal().into_parts();
            LiveReply {
                bytes: bytes.into_vec(),
                durable: release == ReplyRelease::AfterDurableAcceptance,
            }
        }
    }
}
