use std::{convert::Infallible, error::Error, fmt, future, num::NonZeroUsize};

use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_protocol_host::{
    AppliedFrameProcessingOutput, FramePoll, FrameProcessingGate, FrameProcessingLimits,
    SessionInputBuffer, SessionState, WireFrame,
};
use fleetiq_protocol_sdk::{
    DecodedMeasurement, DecodedOutput, DecodedValue, DeviceEndpointId, FrameBoundary,
    FrameProcessingOutput, ObservationQuality, ProposedReply, ReplyRelease, SessionStateAction,
    SourceSignalId,
};
use fleetiq_telemetry::{
    AcceptedMessage, AcceptedMessageId, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};

use super::*;
use crate::{AcceptedMessageAppendOutcome, AcceptedMessageLog, PendingAcceptedMessage};

#[test]
fn preparation_binds_exact_frame_and_withholds_durable_effects() {
    let prepared = prepared_frame();

    assert_eq!(prepared.ready_replies().len(), 1);
    assert_eq!(prepared.ready_replies()[0].as_bytes(), b"send-now");
    assert_eq!(prepared.pending().decoded_output_count(), 1);
    assert_eq!(prepared.pending().deferred_reply_count(), 1);
    assert_eq!(
        prepared.pending().candidate().payload().as_bytes(),
        b"private-frame"
    );
    let debug = format!("{prepared:?}");
    assert!(!debug.contains("private-frame"));
    assert!(!debug.contains("private.signal"));
    assert!(!debug.contains("send-now"));
    assert!(!debug.contains("later"));
}

#[tokio::test]
async fn durable_acceptance_assembles_observations_and_releases_deferred_replies() {
    let (_, pending) = prepared_frame().into_parts();

    let accepted = pending
        .accept(&StubLog::new(StubMode::Append))
        .await
        .unwrap();

    assert!(accepted.append_outcome().was_appended());
    assert_eq!(accepted.message().payload().as_bytes(), b"private-frame");
    assert_eq!(accepted.observations().len(), 1);
    let observation = &accepted.observations()[0];
    assert_eq!(
        observation.id(),
        &accepted.message().decoded_observation_id(0)
    );
    assert_eq!(observation.tenant_id(), accepted.message().tenant_id());
    assert_eq!(
        observation.source().device_asset_id(),
        accepted.message().device_asset_id()
    );
    assert_eq!(observation.source().signal_id().as_str(), "private.signal");
    assert_eq!(accepted.released_replies().len(), 1);
    assert_eq!(accepted.released_replies()[0].as_bytes(), b"later");
    assert_eq!(
        accepted.released_replies()[0].durable_acceptance(),
        Some(accepted.message().id())
    );
}

#[tokio::test]
async fn exact_retry_retains_the_log_disposition() {
    let (_, pending) = prepared_frame().into_parts();

    let accepted = pending
        .accept(&StubLog::new(StubMode::AlreadyPresent))
        .await
        .unwrap();

    assert!(!accepted.append_outcome().was_appended());
    assert_eq!(accepted.observations().len(), 1);
    assert_eq!(accepted.released_replies().len(), 1);
}

#[tokio::test]
async fn append_failure_returns_all_pending_work_for_retry() {
    let (ready, pending) = prepared_frame().into_parts();
    assert_eq!(ready.len(), 1);

    let error = pending
        .accept(&StubLog::new(StubMode::Fail))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        FrameAcceptanceError::Append {
            source: StubError,
            ref pending,
        } if pending.candidate().payload().as_bytes() == b"private-frame"
            && pending.decoded_output_count() == 1
            && pending.deferred_reply_count() == 1
    ));
}

#[tokio::test]
async fn mismatched_log_result_releases_no_gated_effect() {
    let (_, pending) = prepared_frame().into_parts();

    let error = pending
        .accept(&StubLog::new(StubMode::Mismatched))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        FrameAcceptanceError::LogContractViolation {
            ref pending,
            ref returned,
        } if pending.decoded_output_count() == 1
            && pending.deferred_reply_count() == 1
            && matches!(
                returned.as_ref(),
                AcceptedMessageAppendOutcome::Appended(message)
                    if message.id().as_str() == "other-message"
            )
    ));
}

#[derive(Clone, Copy)]
enum StubMode {
    Append,
    AlreadyPresent,
    Fail,
    Mismatched,
}

struct StubLog {
    mode: StubMode,
}

impl StubLog {
    const fn new(mode: StubMode) -> Self {
        Self { mode }
    }
}

impl AcceptedMessageLog for StubLog {
    type Error = StubError;

    fn append<'a>(
        &'a self,
        candidate: &'a PendingAcceptedMessage,
    ) -> impl Future<Output = Result<AcceptedMessageAppendOutcome, Self::Error>> + Send + 'a {
        let result = match self.mode {
            StubMode::Append => Ok(AcceptedMessageAppendOutcome::Appended(
                candidate.clone().into_accepted_message(),
            )),
            StubMode::AlreadyPresent => Ok(AcceptedMessageAppendOutcome::AlreadyPresent(
                candidate.clone().into_accepted_message(),
            )),
            StubMode::Fail => Err(StubError),
            StubMode::Mismatched => Ok(AcceptedMessageAppendOutcome::Appended(mismatched_message(
                candidate,
            ))),
        };
        future::ready(result)
    }
}

#[derive(Debug)]
struct StubError;

impl fmt::Display for StubError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("stub append failure")
    }
}

impl Error for StubError {}

fn prepared_frame() -> PreparedFrameAcceptance {
    PreparedFrameAcceptance::new(context(), wire_frame(b"private-frame"), applied_output())
}

fn context() -> FrameAcceptanceContext {
    FrameAcceptanceContext::new(
        AcceptedMessageId::new("message-1").unwrap(),
        TenantId::new("tenant-a").unwrap(),
        AssetId::new("device-a").unwrap(),
        package_ref(),
        UnixTimeMillis::new(1_000),
    )
}

fn package_ref() -> ProtocolPackageRef {
    ProtocolPackageRef::new(
        ProtocolPackageId::new("vendor.protocol").unwrap(),
        ProtocolPackageVersion::new("2026.09.1").unwrap(),
    )
}

fn mismatched_message(candidate: &PendingAcceptedMessage) -> AcceptedMessage {
    AcceptedMessage::new(
        AcceptedMessageId::new("other-message").unwrap(),
        candidate.tenant_id().clone(),
        candidate.device_asset_id().clone(),
        candidate.protocol_package().clone(),
        candidate.received_at(),
        candidate.payload().clone(),
    )
}

fn applied_output() -> AppliedFrameProcessingOutput {
    let gate = FrameProcessingGate::new(FrameProcessingLimits::new(
        nonzero(2),
        nonzero(2),
        nonzero(16),
        nonzero(32),
    ));
    let mut state = SessionState::new(nonzero(8));
    gate.apply(
        &mut state,
        FrameProcessingOutput::new(
            vec![DecodedOutput::new(
                DeviceEndpointId::new("can.primary").unwrap(),
                SourceSignalId::new("private.signal").unwrap(),
                DecodedMeasurement::new(DecodedValue::Integer(42), None, ObservationQuality::Good)
                    .unwrap(),
                Some(UnixTimeMillis::new(900)),
            )],
            SessionStateAction::Keep,
            vec![
                ProposedReply::new(b"send-now".as_slice(), ReplyRelease::AfterProcessing).unwrap(),
                ProposedReply::new(b"later".as_slice(), ReplyRelease::AfterDurableAcceptance)
                    .unwrap(),
            ],
        ),
    )
    .unwrap()
}

fn wire_frame(bytes: &[u8]) -> WireFrame {
    let mut input = SessionInputBuffer::new(nonzero(bytes.len()));
    input.push(bytes).unwrap();
    match input
        .poll_frame(|_| {
            Ok::<_, Infallible>(FrameBoundary::Frame {
                length: nonzero(bytes.len()),
            })
        })
        .unwrap()
    {
        FramePoll::Frame(frame) => frame,
        FramePoll::NeedMoreData => panic!("test frame must be complete"),
    }
}

fn nonzero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test value is nonzero")
}
