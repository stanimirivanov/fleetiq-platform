//! A short-lived native child contains compiler crashes and resource exhaustion.
//! Request cancellation signals a supervisor that retains the capacity permit
//! until the child is killed and reaped; dropping HTTP futures cannot free it early.
use fleetiq_ingestion::{
    ProtocolAdmissionWorld, ProtocolArtifact, ProtocolCompiler, ProtocolCompilerError,
};
use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    sync::{Semaphore, oneshot},
};

#[cfg(test)]
mod tests;

pub(super) struct IsolatedCompiler {
    slots: Arc<Semaphore>,
}
impl IsolatedCompiler {
    pub(super) fn new() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(1)),
        }
    }
}
struct Cancellation(Option<oneshot::Sender<()>>);
impl Drop for Cancellation {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl ProtocolCompiler for IsolatedCompiler {
    async fn validate(
        &self,
        artifact: &ProtocolArtifact,
        world: ProtocolAdmissionWorld,
    ) -> Result<(), ProtocolCompilerError> {
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| ProtocolCompilerError::Busy)?;
        let executable = worker_path().ok_or(ProtocolCompilerError::Unavailable)?;
        let bytes = artifact.bytes().to_vec();
        let (cancel, cancelled) = oneshot::channel();
        let (result, receive) = oneshot::channel();
        let _cancellation = Cancellation(Some(cancel));
        tokio::spawn(async move {
            let outcome =
                run_worker(executable, world, bytes, cancelled, Duration::from_secs(15)).await;
            // Publish completion only after freeing the reaped worker slot.
            drop(permit);
            let _ = result.send(outcome);
        });
        receive
            .await
            .unwrap_or(Err(ProtocolCompilerError::Unavailable))
    }
}
fn worker_path() -> Option<PathBuf> {
    // Only the installed sibling binary is trusted; request/environment input
    // cannot choose an executable. A missing worker fails closed.
    Some(std::env::current_exe().ok()?.with_file_name(format!(
        "fleetiq-protocol-admission{}",
        std::env::consts::EXE_SUFFIX
    )))
}
async fn run_worker(
    executable: PathBuf,
    world: ProtocolAdmissionWorld,
    bytes: Vec<u8>,
    cancelled: oneshot::Receiver<()>,
    timeout: Duration,
) -> Result<(), ProtocolCompilerError> {
    let mut command = Command::new(executable);
    command.arg(world.as_str());
    supervise(command, bytes, cancelled, timeout).await
}

async fn supervise(
    mut command: Command,
    bytes: Vec<u8>,
    mut cancelled: oneshot::Receiver<()>,
    timeout: Duration,
) -> Result<(), ProtocolCompilerError> {
    let mut child = command
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| ProtocolCompilerError::Unavailable)?;
    let work = async {
        let mut input = child
            .stdin
            .take()
            .ok_or(ProtocolCompilerError::Unavailable)?;
        input
            .write_all(&bytes)
            .await
            .map_err(|_| ProtocolCompilerError::Unavailable)?;
        drop(input);
        let status = child
            .wait()
            .await
            .map_err(|_| ProtocolCompilerError::Unavailable)?;
        match status.code() {
            Some(0) => Ok(()),
            Some(2) => Err(ProtocolCompilerError::Rejected),
            _ => Err(ProtocolCompilerError::Unavailable),
        }
    };
    let result = tokio::select! {
        result = tokio::time::timeout(timeout, work) => result.unwrap_or(Err(ProtocolCompilerError::Timeout)),
        _ = &mut cancelled => Err(ProtocolCompilerError::Unavailable),
    };
    if result.is_err() {
        // kill() waits for termination; wait() also covers an already-exited
        // child. The permit stays owned through both cleanup attempts.
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}
