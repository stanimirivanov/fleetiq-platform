//! Database bytes and private supervised child I/O stay at the outbound boundary.
use crate::protocol_process::{Cancellation, supervise};
use fleetiq_ingestion::{
    ProtocolArtifactStore, ProtocolPackageCatalog, ReplayEffects, ReplayExecutor, ReplayFailure,
    ReplayWork,
};
use fleetiq_postgres::{PostgresProtocolArtifactStore, PostgresProtocolPackageCatalog};
use fleetiq_protocol_sdk::replay_wire::{REPLAY_REQUEST_LIMIT, ReplayRequest, ReplayResponse};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{Semaphore, oneshot};
#[cfg(test)]
mod tests;
fn executor_path() -> std::io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name(format!(
        "fleetiq-protocol-decode{}",
        std::env::consts::EXE_SUFFIX
    )))
}
pub(super) fn check_installed() -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(executor_path()?)?;
    if !metadata.is_file() {
        return Err(std::io::Error::other("executor must be a regular file"));
    }
    Ok(())
}
pub(super) struct IsolatedReplay {
    pool: sqlx::PgPool,
    slots: Arc<Semaphore>,
}
impl IsolatedReplay {
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
impl ReplayExecutor for IsolatedReplay {
    async fn execute(&self, work: &ReplayWork) -> Result<ReplayEffects, ReplayFailure> {
        work.validate().map_err(|_| ReplayFailure::Rejected)?;
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| ReplayFailure::Unavailable)?;
        let release = PostgresProtocolPackageCatalog::new(self.pool.clone())
            .find(work.message.tenant_id(), work.message.protocol_package())
            .await
            .map_err(|_| ReplayFailure::Unavailable)?
            .ok_or(ReplayFailure::Rejected)?;
        if release.artifact_digest() != work.digest {
            return Err(ReplayFailure::Rejected);
        }
        let artifact = PostgresProtocolArtifactStore::new(self.pool.clone())
            .find(&release)
            .await
            .map_err(|_| ReplayFailure::Unavailable)?
            .ok_or(ReplayFailure::Unavailable)?;
        let request = ReplayRequest {
            policy: 1,
            execution: work.execution,
            package_id: work.message.protocol_package().id().as_str().into(),
            package_version: work.message.protocol_package().version().as_str().into(),
            world: work.world.as_str().into(),
            artifact: artifact.bytes().to_vec(),
            frame: work.message.payload().as_bytes().to_vec(),
            state: work.pre_state.clone(),
        };
        let bytes = serde_json::to_vec(&request).map_err(|_| ReplayFailure::Rejected)?;
        if bytes.len() as u64 > REPLAY_REQUEST_LIMIT {
            return Err(ReplayFailure::Rejected);
        }
        let path = executor_path().map_err(|_| ReplayFailure::Unavailable)?;
        let (cancel, cancelled) = oneshot::channel();
        let (send, receive) = oneshot::channel();
        let _guard = Cancellation(Some(cancel));
        tokio::spawn(async move {
            let result = supervise(path, bytes, cancelled).await;
            drop(permit);
            let _ = send.send(result);
        });
        let response: ReplayResponse = receive.await.unwrap_or(Err(ReplayFailure::Unavailable))?;
        if response.execution != work.execution {
            return Err(ReplayFailure::Unavailable);
        }
        if response.policy != 1 || response.state.len() > 16384 || response.outputs.len() > 1200 {
            return Err(ReplayFailure::Rejected);
        }
        let outputs = response
            .outputs
            .into_iter()
            .map(|v| v.into_output().map_err(|_| ReplayFailure::Rejected))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ReplayEffects {
            outputs,
            state: response.state,
        })
    }
}
