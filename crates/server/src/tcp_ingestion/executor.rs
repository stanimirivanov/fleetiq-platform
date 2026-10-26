//! Private live execution adapter. One shared slot includes cancellation cleanup.
use crate::protocol_process::{Cancellation, supervise};
use fleetiq_ingestion::{
    LiveExecutionFailure, LiveExecutor, LivePrepared, ProtocolArtifactStore, ProtocolPackageRelease,
};
use fleetiq_postgres::PostgresProtocolArtifactStore;
use fleetiq_protocol_sdk::{
    DeviceIdentityClaim, ProposedReply, ReplyRelease,
    live_wire::{LiveRequest, LiveResponse},
    replay_wire::{REPLAY_REQUEST_LIMIT, replay_fingerprint},
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{Semaphore, oneshot};
#[derive(Clone)]
pub(super) struct IsolatedLive {
    pool: sqlx::PgPool,
    slots: Arc<Semaphore>,
}
fn path() -> std::io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name(format!(
        "fleetiq-protocol-live{}",
        std::env::consts::EXE_SUFFIX
    )))
}
pub(super) fn check_installed() -> std::io::Result<()> {
    if !std::fs::symlink_metadata(path()?)?.is_file() {
        return Err(std::io::Error::other(
            "live executor must be a regular file",
        ));
    }
    Ok(())
}
impl IsolatedLive {
    pub(super) fn new(pool: sqlx::PgPool) -> Self {
        Self {
            pool,
            slots: Arc::new(Semaphore::new(1)),
        }
    }
    pub(super) async fn drain(&self) {
        let _permit = self.slots.acquire().await;
    }
}
impl LiveExecutor for IsolatedLive {
    async fn prepare(
        &self,
        release: &ProtocolPackageRelease,
        unread: &[u8],
        state: &[u8],
        identify: bool,
    ) -> Result<Option<LivePrepared>, LiveExecutionFailure> {
        use LiveExecutionFailure::{Rejected, Unavailable};
        if unread.is_empty() || unread.len() > 65536 || state.len() > 16384 {
            return Err(Rejected);
        }
        // The bounded connection set is the only waiting queue; cancellation
        // while waiting cannot reserve a child or leak its execution permit.
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Unavailable)?;
        let artifact = PostgresProtocolArtifactStore::new(self.pool.clone())
            .find(release)
            .await
            .map_err(|_| Unavailable)?
            .ok_or(Unavailable)?;
        let request = LiveRequest {
            policy: 1,
            execution: replay_fingerprint(),
            package_id: release.package().id().as_str().into(),
            package_version: release.package().version().as_str().into(),
            artifact: artifact.bytes().to_vec(),
            unread: unread.to_vec(),
            state: state.to_vec(),
            identify,
        };
        let bytes = serde_json::to_vec(&request).map_err(|_| Rejected)?;
        if bytes.len() as u64 > REPLAY_REQUEST_LIMIT {
            return Err(Rejected);
        }
        let path = path().map_err(|_| Unavailable)?;
        let (cancel, cancelled) = oneshot::channel();
        let (sender, receiver) = oneshot::channel();
        let _guard = Cancellation(Some(cancel));
        tokio::spawn(async move {
            let result = supervise::<LiveResponse>(path, bytes, cancelled).await;
            drop(permit);
            let _ = sender.send(result);
        });
        let response =
            receiver
                .await
                .map_err(|_| Unavailable)?
                .map_err(|failure| match failure {
                    fleetiq_ingestion::ReplayFailure::Rejected => Rejected,
                    fleetiq_ingestion::ReplayFailure::Unavailable => Unavailable,
                })?;
        validate_response(response, request.execution, unread.len(), identify)
    }
}
fn validate_response(
    response: LiveResponse,
    execution: [u8; 32],
    unread_len: usize,
    requires_claim: bool,
) -> Result<Option<LivePrepared>, LiveExecutionFailure> {
    use LiveExecutionFailure::{Rejected, Unavailable};
    if response.execution != execution {
        return Err(Unavailable);
    }
    response
        .turn
        .map(|turn| {
            if turn.consumed == 0
                || turn.consumed > unread_len
                || turn.state.len() > 16384
                || turn.outputs.len() > 1200
                || turn.replies.len() > 16
            {
                return Err(Rejected);
            }
            let identity = turn
                .identity
                .map(|(k, v)| DeviceIdentityClaim::new(k, v).map_err(|_| Rejected))
                .transpose()?;
            if identity.is_some() != requires_claim {
                return Err(Rejected);
            }
            let outputs = turn
                .outputs
                .into_iter()
                .map(|v| v.into_output().map_err(|_| Rejected))
                .collect::<Result<Vec<_>, _>>()?;
            let mut total = 0_usize;
            let replies = turn
                .replies
                .into_iter()
                .map(|r| {
                    total = total.saturating_add(r.bytes.len());
                    if r.bytes.len() > 4096 || total > 16384 {
                        return Err(Rejected);
                    }
                    ProposedReply::new(
                        r.bytes,
                        if r.durable {
                            ReplyRelease::AfterDurableAcceptance
                        } else {
                            ReplyRelease::AfterProcessing
                        },
                    )
                    .map_err(|_| Rejected)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(LivePrepared {
                consumed: turn.consumed,
                identity,
                state: turn.state,
                outputs,
                replies,
            })
        })
        .transpose()
}
