use std::num::NonZeroUsize;

use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_protocol_sdk::{ProposedReply, ProposedReplyError, ReplyRelease};
use fleetiq_telemetry::{
    AcceptedMessage, AcceptedMessageId, AcceptedPayload, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};

use super::{ReplyGate, ReplyStagingError, StagedReply};

fn limit(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).expect("test limits are nonzero")
}

fn reply(bytes: &[u8], release: ReplyRelease) -> ProposedReply {
    ProposedReply::new(bytes, release).expect("test reply is nonempty")
}

fn accepted_message() -> AcceptedMessage {
    AcceptedMessage::new(
        AcceptedMessageId::new("message-1").expect("valid message ID"),
        TenantId::new("tenant-a").expect("valid tenant ID"),
        AssetId::new("device-1").expect("valid device asset ID"),
        ProtocolPackageRef::new(
            ProtocolPackageId::new("example.binary").expect("valid package ID"),
            ProtocolPackageVersion::new("1").expect("valid package version"),
        ),
        UnixTimeMillis::new(1_000),
        AcceptedPayload::new(b"frame".as_slice()),
    )
}

#[test]
fn empty_replies_are_rejected_at_the_package_contract() {
    assert_eq!(
        ProposedReply::new(Vec::new(), ReplyRelease::AfterProcessing),
        Err(ProposedReplyError::Empty)
    );
}

#[test]
fn processing_gated_reply_is_ready_after_host_validation() {
    let gate = ReplyGate::new(limit(8));

    let StagedReply::Ready(ready) = gate
        .stage(reply(b"hello", ReplyRelease::AfterProcessing))
        .expect("reply is within the limit")
    else {
        panic!("the reply should not require durable acceptance");
    };

    assert_eq!(ready.as_bytes(), b"hello");
    assert_eq!(ready.durable_acceptance(), None);
}

#[test]
fn acceptance_gated_reply_records_evidence_before_exposing_bytes() {
    let gate = ReplyGate::new(limit(8));
    let accepted = accepted_message();

    let StagedReply::AwaitingDurableAcceptance(deferred) = gate
        .stage(reply(b"ack", ReplyRelease::AfterDurableAcceptance))
        .expect("reply is within the limit")
    else {
        panic!("the reply should wait for durable acceptance");
    };

    assert_eq!(deferred.len(), 3);
    let ready = deferred.release(&accepted);
    assert_eq!(ready.as_bytes(), b"ack");
    assert_eq!(ready.durable_acceptance(), Some(accepted.id()));
}

#[test]
fn a_reply_at_the_limit_is_accepted() {
    let gate = ReplyGate::new(limit(3));

    let staged = gate.stage(reply(b"ack", ReplyRelease::AfterProcessing));

    assert!(matches!(staged, Ok(StagedReply::Ready(_))));
}

#[test]
fn an_oversized_reply_is_rejected() {
    let gate = ReplyGate::new(limit(3));

    assert_eq!(
        gate.stage(reply(b"large", ReplyRelease::AfterProcessing)),
        Err(ReplyStagingError::LimitExceeded {
            limit: 3,
            attempted: 5,
        })
    );
}

#[test]
fn debug_output_does_not_expose_reply_bytes() {
    let gate = ReplyGate::new(limit(16));
    let proposal = reply(b"secret", ReplyRelease::AfterDurableAcceptance);
    let proposal_debug = format!("{proposal:?}");
    let StagedReply::AwaitingDurableAcceptance(deferred) =
        gate.stage(proposal).expect("reply is within the limit")
    else {
        panic!("the reply should wait for durable acceptance");
    };
    let deferred_debug = format!("{deferred:?}");
    let outbound_debug = format!("{:?}", deferred.release(&accepted_message()));

    assert!(proposal_debug.contains("length: 6"));
    assert!(!proposal_debug.contains("secret"));
    assert!(deferred_debug.contains("length: 6"));
    assert!(!deferred_debug.contains("secret"));
    assert!(outbound_debug.contains("length: 6"));
    assert!(!outbound_debug.contains("secret"));
}

#[test]
fn ipc_reproposal_keeps_the_durable_release_condition() {
    let gate = ReplyGate::new(limit(8));
    let StagedReply::AwaitingDurableAcceptance(deferred) = gate
        .stage(reply(b"ack", ReplyRelease::AfterDurableAcceptance))
        .unwrap()
    else {
        panic!("must defer")
    };
    let proposal = deferred.into_proposal();
    let StagedReply::AwaitingDurableAcceptance(restaged) = gate.stage(proposal).unwrap() else {
        panic!("IPC cannot turn durability into processing readiness")
    };
    let accepted = accepted_message();
    let released = restaged.release(&accepted);
    assert_eq!(released.as_bytes(), b"ack");
    assert_eq!(released.durable_acceptance(), Some(accepted.id()));
}
