use std::{convert::Infallible, error::Error, fmt, future, num::NonZeroUsize};

use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_protocol_host::{
    FramePoll, FrameProcessingGate, FrameProcessingLimits, SessionInputBuffer, SessionState,
    WireFrame,
};
use fleetiq_protocol_sdk::{
    DecodedMeasurement, DecodedOutput, DecodedValue, DeviceEndpointId, FrameBoundary,
    FrameProcessingOutput, ObservationQuality, ProposedReply, ReplyRelease, SessionStateAction,
    SourceSignalId,
};
use fleetiq_telemetry::{
    AcceptedMessageId, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};

use super::*;
use crate::{
    AcceptedMessageAppendOutcome, AcceptedMessageLog, FrameAcceptanceContext,
    PendingAcceptedMessage, PreparedFrameAcceptance,
};

#[tokio::test]
async fn preparation_builds_batch_without_changing_reply_eligibility() {
    let pending = accepted_frame(true, true)
        .await
        .prepare_observation_persistence()
        .expect("accepted observations must form a batch");

    assert!(pending.accepted_message_was_appended());
    assert_eq!(pending.observation_count(), 1);
    assert_eq!(pending.message().id().as_str(), "message-1");
    assert_eq!(pending.released_replies()[0].as_bytes(), b"later");
    assert_eq!(
        pending.released_replies()[0].durable_acceptance(),
        Some(pending.message().id())
    );

    let debug = format!("{pending:?}");
    assert!(!debug.contains("private-frame"));
    assert!(!debug.contains("private.signal"));
    assert!(!debug.contains("private-value"));
    assert!(!debug.contains("later"));
}

#[tokio::test]
async fn persistence_returns_both_dispositions_and_durable_effects() {
    let pending = accepted_frame(false, true)
        .await
        .prepare_observation_persistence()
        .expect("accepted observations must form a batch");

    let persisted = pending
        .persist(&StubObservationLog::new(ObservationLogMode::AlreadyPresent))
        .await
        .expect("exact observation retry must succeed");

    assert!(!persisted.accepted_message_was_appended());
    assert_eq!(
        persisted.observation_append_outcome(),
        DecodedObservationAppendOutcome::AlreadyPresent
    );
    assert_eq!(persisted.observations().len(), 1);
    assert_eq!(
        persisted.observations()[0].value(),
        &DecodedValue::Text("private-value".into())
    );
    assert_eq!(persisted.released_replies()[0].as_bytes(), b"later");

    let parts = persisted.into_parts();
    assert!(!parts.0);
    assert_eq!(parts.1, DecodedObservationAppendOutcome::AlreadyPresent);
    assert_eq!(parts.2.len(), 1);
    assert_eq!(parts.3.len(), 1);
}

#[tokio::test]
async fn append_failure_returns_the_complete_pending_value_for_retry() {
    let pending = accepted_frame(true, true)
        .await
        .prepare_observation_persistence()
        .expect("accepted observations must form a batch");

    let error = pending
        .persist(&StubObservationLog::new(ObservationLogMode::Fail))
        .await
        .expect_err("stub append must fail");
    let ObservationPersistenceError::Append { source, pending } = error;
    assert_eq!(source, StubError);
    assert_eq!(pending.observation_count(), 1);
    assert_eq!(pending.released_replies()[0].as_bytes(), b"later");

    let persisted = pending
        .persist(&StubObservationLog::new(ObservationLogMode::Appended))
        .await
        .expect("retained pending value must be retryable");
    assert!(persisted.observation_append_outcome().was_appended());
    assert_eq!(persisted.message().id().as_str(), "message-1");
}

#[tokio::test]
async fn empty_decoding_result_crosses_the_same_durability_boundary() {
    let pending = accepted_frame(true, false)
        .await
        .prepare_observation_persistence()
        .expect("empty observations must form a completed batch");
    assert_eq!(pending.observation_count(), 0);

    let persisted = pending
        .persist(&StubObservationLog::new(ObservationLogMode::Appended))
        .await
        .expect("empty batch must be persisted");
    assert_eq!(persisted.observations().len(), 0);
    assert_eq!(persisted.released_replies().len(), 1);
}

#[derive(Clone, Copy)]
enum ObservationLogMode {
    Appended,
    AlreadyPresent,
    Fail,
}

struct StubObservationLog {
    mode: ObservationLogMode,
}

impl StubObservationLog {
    const fn new(mode: ObservationLogMode) -> Self {
        Self { mode }
    }
}

impl DecodedObservationLog for StubObservationLog {
    type Error = StubError;

    fn append<'a>(
        &'a self,
        _batch: &'a DecodedObservationBatch,
    ) -> impl Future<Output = Result<DecodedObservationAppendOutcome, Self::Error>> + Send + 'a
    {
        future::ready(match self.mode {
            ObservationLogMode::Appended => Ok(DecodedObservationAppendOutcome::Appended),
            ObservationLogMode::AlreadyPresent => {
                Ok(DecodedObservationAppendOutcome::AlreadyPresent)
            }
            ObservationLogMode::Fail => Err(StubError),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StubError;

impl fmt::Display for StubError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("stub append failure")
    }
}

impl Error for StubError {}

struct StubAcceptedMessageLog {
    appended: bool,
}

impl AcceptedMessageLog for StubAcceptedMessageLog {
    type Error = Infallible;

    fn append<'a>(
        &'a self,
        candidate: &'a PendingAcceptedMessage,
    ) -> impl Future<Output = Result<AcceptedMessageAppendOutcome, Self::Error>> + Send + 'a {
        let message = candidate.clone().into_accepted_message();
        future::ready(Ok(if self.appended {
            AcceptedMessageAppendOutcome::Appended(message)
        } else {
            AcceptedMessageAppendOutcome::AlreadyPresent(message)
        }))
    }
}

async fn accepted_frame(
    accepted_message_was_appended: bool,
    with_observation: bool,
) -> DurablyAcceptedFrame {
    let output = if with_observation {
        vec![DecodedOutput::new(
            DeviceEndpointId::new("can.primary").expect("valid endpoint ID"),
            SourceSignalId::new("private.signal").expect("valid signal ID"),
            DecodedMeasurement::new(
                DecodedValue::Text("private-value".into()),
                None,
                ObservationQuality::Good,
            )
            .expect("valid measurement"),
            Some(UnixTimeMillis::new(900)),
        )]
    } else {
        Vec::new()
    };
    let gate = FrameProcessingGate::new(FrameProcessingLimits::new(
        nonzero(2),
        nonzero(1),
        nonzero(16),
        nonzero(16),
    ));
    let mut state = SessionState::new(nonzero(8));
    let applied = gate
        .apply(
            &mut state,
            FrameProcessingOutput::new(
                output,
                SessionStateAction::Keep,
                vec![
                    ProposedReply::new(b"later".as_slice(), ReplyRelease::AfterDurableAcceptance)
                        .expect("nonempty reply"),
                ],
            ),
        )
        .expect("output must pass host limits");
    let prepared = PreparedFrameAcceptance::new(
        FrameAcceptanceContext::new(
            AcceptedMessageId::new("message-1").expect("valid message ID"),
            TenantId::new("tenant-a").expect("valid tenant ID"),
            AssetId::new("device-a").expect("valid asset ID"),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("vendor.protocol").expect("valid package ID"),
                ProtocolPackageVersion::new("1").expect("valid package version"),
            ),
            UnixTimeMillis::new(1_000),
        ),
        wire_frame(b"private-frame"),
        applied,
    );
    let (_, pending) = prepared.into_parts();
    pending
        .accept(&StubAcceptedMessageLog {
            appended: accepted_message_was_appended,
        })
        .await
        .expect("accepted-message append must succeed")
}

fn wire_frame(bytes: &[u8]) -> WireFrame {
    let mut input = SessionInputBuffer::new(nonzero(bytes.len()));
    input.push(bytes).expect("frame fits input buffer");
    match input
        .poll_frame(|_| {
            Ok::<_, Infallible>(FrameBoundary::Frame {
                length: nonzero(bytes.len()),
            })
        })
        .expect("valid frame decision")
    {
        FramePoll::Frame(frame) => frame,
        FramePoll::NeedMoreData => panic!("test frame must be complete"),
    }
}

fn nonzero(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test value is nonzero")
}
