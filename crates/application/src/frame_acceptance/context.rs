use std::fmt;

use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_protocol_host::{
    AppliedFrameProcessingOutput, DeferredReply, OutboundReply, StagedReply, WireFrame,
};
use fleetiq_telemetry::{AcceptedMessageId, AcceptedPayload, ProtocolPackageRef};

use super::PendingFrameAcceptance;
use crate::PendingAcceptedMessage;

/// Trusted provenance assigned to one complete frame before durable append.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameAcceptanceContext {
    message_id: AcceptedMessageId,
    tenant_id: TenantId,
    device_asset_id: AssetId,
    protocol_package: ProtocolPackageRef,
    received_at: UnixTimeMillis,
}

impl FrameAcceptanceContext {
    /// Creates the trusted identity and provenance for one processed frame.
    pub const fn new(
        message_id: AcceptedMessageId,
        tenant_id: TenantId,
        device_asset_id: AssetId,
        protocol_package: ProtocolPackageRef,
        received_at: UnixTimeMillis,
    ) -> Self {
        Self {
            message_id,
            tenant_id,
            device_asset_id,
            protocol_package,
            received_at,
        }
    }

    /// Returns the stable identity assigned before durable append.
    pub const fn message_id(&self) -> &AcceptedMessageId {
        &self.message_id
    }

    /// Returns the tenant established during device admission.
    pub const fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    /// Returns the admitted communication-device asset.
    pub const fn device_asset_id(&self) -> &AssetId {
        &self.device_asset_id
    }

    /// Returns the immutable package release pinned to the session.
    pub const fn protocol_package(&self) -> &ProtocolPackageRef {
        &self.protocol_package
    }

    /// Returns when the host received the complete frame.
    pub const fn received_at(&self) -> UnixTimeMillis {
        self.received_at
    }

    fn into_parts(
        self,
    ) -> (
        AcceptedMessageId,
        TenantId,
        AssetId,
        ProtocolPackageRef,
        UnixTimeMillis,
    ) {
        (
            self.message_id,
            self.tenant_id,
            self.device_asset_id,
            self.protocol_package,
            self.received_at,
        )
    }
}

/// Immediate replies and durability-gated work prepared from one processed frame.
#[derive(Clone, Eq, PartialEq)]
pub struct PreparedFrameAcceptance {
    ready_replies: Vec<OutboundReply>,
    pending: PendingFrameAcceptance,
}

impl PreparedFrameAcceptance {
    /// Binds a processed frame and all of its effects to trusted provenance.
    ///
    /// Processing-gated replies become available immediately in package order.
    /// Decoded outputs and durability-gated replies remain inaccessible inside
    /// [`PendingFrameAcceptance`] until accepted-message append succeeds.
    pub fn new(
        context: FrameAcceptanceContext,
        frame: WireFrame,
        output: AppliedFrameProcessingOutput,
    ) -> Self {
        let (message_id, tenant_id, device_asset_id, protocol_package, received_at) =
            context.into_parts();
        let (decoded_outputs, staged_replies) = output.into_parts();
        let (ready_replies, deferred_replies) = partition_replies(staged_replies);
        let candidate = PendingAcceptedMessage::new(
            message_id,
            tenant_id,
            device_asset_id,
            protocol_package,
            received_at,
            AcceptedPayload::new(frame.into_bytes().into_vec()),
        );

        Self {
            ready_replies,
            pending: PendingFrameAcceptance::new(candidate, decoded_outputs, deferred_replies),
        }
    }

    /// Returns replies eligible after processing and host validation.
    pub fn ready_replies(&self) -> &[OutboundReply] {
        &self.ready_replies
    }

    /// Returns the opaque durability-gated work for inspection and metrics.
    pub const fn pending(&self) -> &PendingFrameAcceptance {
        &self.pending
    }

    /// Separates immediately eligible replies from the pending durable append.
    pub fn into_parts(self) -> (Vec<OutboundReply>, PendingFrameAcceptance) {
        (self.ready_replies, self.pending)
    }
}

impl fmt::Debug for PreparedFrameAcceptance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedFrameAcceptance")
            .field("ready_reply_count", &self.ready_replies.len())
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

fn partition_replies(replies: Vec<StagedReply>) -> (Vec<OutboundReply>, Vec<DeferredReply>) {
    let mut ready = Vec::new();
    let mut deferred = Vec::new();
    for reply in replies {
        match reply {
            StagedReply::Ready(reply) => ready.push(reply),
            StagedReply::AwaitingDurableAcceptance(reply) => deferred.push(reply),
        }
    }
    (ready, deferred)
}
