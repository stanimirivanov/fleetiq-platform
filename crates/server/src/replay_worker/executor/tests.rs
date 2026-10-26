use super::*;
#[cfg(target_os = "linux")]
use std::time::Duration;
#[tokio::test]
async fn missing_executor_is_retryable() {
    let (sender, receiver) = oneshot::channel();
    drop(sender);
    let path = PathBuf::from("missing-fleetiq-replay-fixture-executable");
    assert!(matches!(
        supervise::<ReplayResponse>(path, vec![], receiver).await,
        Err(ReplayFailure::Unavailable)
    ));
}
#[test]
fn response_envelope_cannot_smuggle_transport_replies() {
    let response = ReplayResponse {
        policy: 1,
        execution: fleetiq_protocol_sdk::replay_wire::replay_fingerprint(),
        state: vec![],
        outputs: vec![],
    };
    let mut envelope = serde_json::to_value(response).unwrap();
    assert!(serde_json::from_value::<ReplayResponse>(envelope.clone()).is_ok());
    envelope["replies"] = serde_json::json!([1]);
    assert!(serde_json::from_value::<ReplayResponse>(envelope).is_err());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn cancellation_reaps_a_running_executor_before_returning() {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let directory = std::env::temp_dir().join(format!(
        "fleetiq-replay-cancel-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    // create_dir refuses any pre-existing fixture; cleanup owns only this path.
    fs::create_dir(&directory).unwrap();
    let fixture = Fixture(directory);
    let started = fixture.0.join("started");
    let script = fixture.0.join("executor");
    let destination = started.to_str().unwrap().replace('\'', "'\\''");
    fs::write(
        &script,
        format!("#!/bin/sh\nprintf '%s' \"$$\" > '{destination}'\nexec /bin/sleep 60\n"),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let (cancel, cancelled) = oneshot::channel();
    let task = tokio::spawn(supervise::<ReplayResponse>(script, vec![], cancelled));
    // A process handshake, not a sleep-based guess about whether spawn happened.
    let pid = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match fs::read_to_string(&started) {
                Ok(text) if !text.is_empty() => break text.parse::<u32>().unwrap(),
                Ok(_) => tokio::task::yield_now().await,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tokio::task::yield_now().await;
                }
                Err(error) => panic!("fixture handshake failed: {error}"),
            }
        }
    })
    .await
    .unwrap();
    cancel.send(()).unwrap();
    assert!(matches!(
        task.await.unwrap(),
        Err(ReplayFailure::Unavailable)
    ));
    assert!(
        !PathBuf::from(format!("/proc/{pid}")).exists(),
        "executor must be killed and reaped before its slot can be reused"
    );
}
