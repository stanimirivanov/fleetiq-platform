//! Shared bounded child supervision. Callers supply only fixed installed sibling paths.
use fleetiq_ingestion::ReplayFailure;
use fleetiq_protocol_sdk::replay_wire::REPLAY_RESPONSE_LIMIT;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::oneshot,
};
pub(crate) struct Cancellation(pub(crate) Option<oneshot::Sender<()>>);
impl Drop for Cancellation {
    fn drop(&mut self) {
        if let Some(s) = self.0.take() {
            let _ = s.send(());
        }
    }
}
pub(crate) async fn supervise<T: serde::de::DeserializeOwned>(
    path: PathBuf,
    bytes: Vec<u8>,
    mut cancelled: oneshot::Receiver<()>,
) -> Result<T, ReplayFailure> {
    let mut child = Command::new(path)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| ReplayFailure::Unavailable)?;
    let work = async {
        let mut stdin = child.stdin.take().ok_or(ReplayFailure::Unavailable)?;
        stdin
            .write_all(&bytes)
            .await
            .map_err(|_| ReplayFailure::Unavailable)?;
        drop(stdin);
        let stdout = child.stdout.take().ok_or(ReplayFailure::Unavailable)?;
        let mut output = Vec::new();
        stdout
            .take(REPLAY_RESPONSE_LIMIT + 1)
            .read_to_end(&mut output)
            .await
            .map_err(|_| ReplayFailure::Unavailable)?;
        if output.len() as u64 > REPLAY_RESPONSE_LIMIT {
            return Err(ReplayFailure::Rejected);
        }
        let status = child.wait().await.map_err(|_| ReplayFailure::Unavailable)?;
        match status.code() {
            Some(0) => serde_json::from_slice(&output).map_err(|_| ReplayFailure::Rejected),
            Some(2) => Err(ReplayFailure::Rejected),
            _ => Err(ReplayFailure::Unavailable),
        }
    };
    let result = tokio::select! {biased; _=&mut cancelled=>Err(ReplayFailure::Unavailable),result=tokio::time::timeout(Duration::from_secs(15),work)=>result.unwrap_or(Err(ReplayFailure::Unavailable))};
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}
