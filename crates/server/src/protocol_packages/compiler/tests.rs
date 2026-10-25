use super::*;

// Child-only fixture; never runs during an ordinary test suite. The parent
// starts this exact test in a separate process and must terminate/reap it.
#[test]
#[ignore = "child process fixture for supervisor timeout/cancellation tests"]
fn parked_fixture() {
    loop {
        std::thread::park();
    }
}
fn parked() -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--ignored",
        "--exact",
        "protocol_packages::compiler::tests::parked_fixture",
    ]);
    command
}
#[tokio::test]
async fn real_hung_child_is_killed_and_reaped_on_deadline_or_cancellation() {
    let (cancel, cancelled) = oneshot::channel();
    let outcome = supervise(parked(), vec![], cancelled, Duration::from_millis(100)).await;
    assert_eq!(outcome, Err(ProtocolCompilerError::Timeout));
    drop(cancel);
    let (cancel, cancelled) = oneshot::channel();
    drop(cancel);
    assert_eq!(
        supervise(parked(), vec![], cancelled, Duration::from_secs(30)).await,
        Err(ProtocolCompilerError::Unavailable)
    );
}
#[tokio::test]
async fn exhausted_capacity_never_spawns_or_queues_another_worker() {
    let compiler = IsolatedCompiler {
        slots: Arc::new(Semaphore::new(1)),
    };
    let permit = compiler.slots.clone().acquire_owned().await.unwrap();
    let bytes = ProtocolArtifact::new(vec![1]).unwrap();
    assert_eq!(
        compiler
            .validate(&bytes, ProtocolAdmissionWorld::Runtime)
            .await,
        Err(ProtocolCompilerError::Busy)
    );
    drop(permit);
    assert_eq!(compiler.slots.available_permits(), 1);
}
