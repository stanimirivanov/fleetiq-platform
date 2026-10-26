//! Validates untrusted turn proposals before releasing any transport/persistence effect.
use super::{
    LiveExecutor, LiveReplayRepository, LiveReplySink, LiveRoutingRepository, LiveSession,
    LiveSessionError, LiveStep, PinnedProtocolSession, REPLAY_STATE_LIMIT, UnixTimeMillis,
};
use crate::{LivePrepared, ReplayEffects, ReplayWork};
use fleetiq_protocol_host::{
    FrameProcessingGate, FrameProcessingLimits, SessionState, StagedReply,
};
use fleetiq_protocol_sdk::{FrameProcessingOutput, SessionStateAction};
use fleetiq_telemetry::{AcceptedMessage, AcceptedMessageId, AcceptedPayload};
use std::num::NonZeroUsize;
impl<R: LiveRoutingRepository, Q: LiveReplayRepository, E: LiveExecutor> LiveSession<R, Q, E> {
    pub(super) async fn process(
        &mut self,
        received_at: UnixTimeMillis,
        sink: &mut impl LiveReplySink,
    ) -> Result<LiveStep, LiveSessionError> {
        let release = self
            .routing
            .bootstrap(&self.channel)
            .await
            .map_err(storage)?
            .ok_or(LiveSessionError::Ineligible)?;
        if let Some(pin) = &self.pinned
            && (!self.routing.revalidate(pin).await.map_err(storage)?
                || pin.snapshot().release != release)
        {
            return Err(LiveSessionError::Ineligible);
        }
        let prepared = self
            .executor
            .prepare(&release, &self.input, &self.state, self.pinned.is_none())
            .await
            .map_err(LiveSessionError::Execution)?;
        let Some(LivePrepared {
            consumed,
            identity,
            state,
            outputs,
            replies,
        }) = prepared
        else {
            return Ok(LiveStep::NeedMore);
        };
        if consumed == 0 || consumed > self.input.len() {
            return Err(LiveSessionError::Rejected);
        }
        if self.pinned.is_none() {
            let claim = identity.ok_or(LiveSessionError::Rejected)?;
            self.pinned = self
                .routing
                .resolve(self.channel.clone(), claim)
                .await
                .map_err(storage)?;
        } else if identity.is_some() {
            return Err(LiveSessionError::Rejected);
        }
        let pinned = self.pinned.as_ref().ok_or(LiveSessionError::Ineligible)?;
        // Recheck immediately before any outbound effect. Concurrent later changes
        // may not cancel already-started bounded work; no future turn reuses this pin.
        if pinned.snapshot().release != release
            || !self.routing.revalidate(pinned).await.map_err(storage)?
        {
            return Err(LiveSessionError::Ineligible);
        }
        let (outputs, replies, successor) = stage(state, outputs, replies)?;
        let work = self.work(pinned, &release, consumed, received_at)?;
        let (batch, state) = ReplayEffects {
            outputs,
            state: successor.as_bytes().to_vec(),
        }
        .batch(&work)
        .map_err(|_| LiveSessionError::Rejected)?;
        let mut deferred = Vec::new();
        // Preserve package order across stages: a later ready reply cannot
        // overtake an earlier durability-gated proposal.
        let mut waiting = false;
        for reply in replies {
            match reply {
                StagedReply::Ready(reply) if !waiting => sink
                    .send(reply.as_bytes())
                    .await
                    .map_err(LiveSessionError::Transport)?,
                reply => {
                    waiting = true;
                    deferred.push(reply);
                }
            }
        }
        if !self.repository.enqueue(&work).await.map_err(storage)? {
            return Err(LiveSessionError::Rejected);
        }
        for reply in deferred {
            let released = match reply {
                StagedReply::Ready(reply) => reply,
                StagedReply::AwaitingDurableAcceptance(reply) => reply.release(&work.message),
            };
            sink.send(released.as_bytes())
                .await
                .map_err(LiveSessionError::Transport)?;
        }
        self.complete_turn(&work, &batch, &state).await?;
        self.state = state;
        self.sequence = work.sequence;
        self.input.drain(..consumed);
        Ok(LiveStep::Committed)
    }
    async fn complete_turn(
        &self,
        work: &ReplayWork,
        batch: &crate::DecodedObservationBatch,
        state: &[u8],
    ) -> Result<(), LiveSessionError> {
        let lease = self
            .repository
            .claim_exact(&work.message)
            .await
            .map_err(storage)?
            .ok_or(LiveSessionError::LostLease)?;
        if &lease.work != work
            || !self
                .repository
                .complete(&lease, batch, state)
                .await
                .map_err(storage)?
        {
            return Err(LiveSessionError::LostLease);
        }
        Ok(())
    }
    fn work(
        &self,
        pinned: &PinnedProtocolSession,
        release: &crate::ProtocolPackageRelease,
        consumed: usize,
        received_at: UnixTimeMillis,
    ) -> Result<ReplayWork, LiveSessionError> {
        Ok(ReplayWork {
            message: AcceptedMessage::new(
                AcceptedMessageId::new(format!("tcp.{}.{}", self.session, self.sequence + 1))
                    .map_err(|_| LiveSessionError::Rejected)?,
                self.channel.tenant_id.clone(),
                pinned.snapshot().route.device_asset_id.clone(),
                release.package().clone(),
                received_at,
                AcceptedPayload::new(self.input[..consumed].to_vec()),
            ),
            session: self.session.clone(),
            sequence: self.sequence + 1,
            digest: release.artifact_digest(),
            world: self.channel.selection.world,
            execution: fleetiq_protocol_sdk::replay_wire::replay_fingerprint(),
            pre_state: self.state.clone(),
        })
    }
}
fn storage(error: impl std::error::Error + Send + Sync + 'static) -> LiveSessionError {
    LiveSessionError::Storage(Box::new(error))
}

fn stage(
    state: Vec<u8>,
    outputs: Vec<fleetiq_protocol_sdk::DecodedOutput>,
    replies: Vec<fleetiq_protocol_sdk::ProposedReply>,
) -> Result<
    (
        Vec<fleetiq_protocol_sdk::DecodedOutput>,
        Vec<StagedReply>,
        SessionState,
    ),
    LiveSessionError,
> {
    let nz = |n| NonZeroUsize::new(n).expect("fixed nonzero policy");
    let mut successor = SessionState::new(nz(REPLAY_STATE_LIMIT));
    let validated = FrameProcessingGate::new(FrameProcessingLimits::new(
        nz(1200),
        nz(16),
        nz(4096),
        nz(16384),
    ))
    .apply(
        &mut successor,
        FrameProcessingOutput::new(
            outputs,
            SessionStateAction::Replace(state.into_boxed_slice()),
            replies,
        ),
    )
    .map_err(|_| LiveSessionError::Rejected)?;
    let (outputs, replies) = validated.into_parts();
    Ok((outputs, replies, successor))
}
