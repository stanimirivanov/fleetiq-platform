use super::*;
use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_telemetry::{
    AcceptedMessageId, AcceptedPayload, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
use std::{
    io,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
fn work() -> ReplayWork {
    ReplayWork {
        message: AcceptedMessage::new(
            AcceptedMessageId::new("m1").unwrap(),
            TenantId::new("tenant").unwrap(),
            AssetId::new("device").unwrap(),
            ProtocolPackageRef::new(
                ProtocolPackageId::new("custom").unwrap(),
                ProtocolPackageVersion::new("1").unwrap(),
            ),
            UnixTimeMillis::new(1),
            AcceptedPayload::new(vec![1]),
        ),
        session: "session".into(),
        sequence: 1,
        digest: ProtocolArtifactDigest::from_sha256([0; 32]),
        world: ProtocolAdmissionWorld::Identity,
        execution: fleetiq_protocol_sdk::replay_wire::replay_fingerprint(),
        pre_state: vec![],
    }
}
struct Repository {
    lease: Mutex<Option<ReplayLease>>,
    completed: AtomicBool,
    poisoned: AtomicBool,
    accept: bool,
}
impl ReplayRepository for Repository {
    type Error = io::Error;
    fn enqueue(
        &self,
        _: &ReplayWork,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        std::future::ready(Ok(true))
    }
    fn claim(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<ReplayLease>, Self::Error>> + Send {
        std::future::ready(Ok(self.lease.lock().unwrap().take()))
    }
    fn complete(
        &self,
        _: &ReplayLease,
        _: &DecodedObservationBatch,
        _: &[u8],
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        self.completed.store(self.accept, Ordering::SeqCst);
        std::future::ready(Ok(self.accept))
    }
    fn poison(
        &self,
        _: &ReplayLease,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        self.poisoned.store(true, Ordering::SeqCst);
        std::future::ready(Ok(true))
    }
}
struct Executor(u8);
impl ReplayExecutor for Executor {
    fn execute(
        &self,
        _: &ReplayWork,
    ) -> impl std::future::Future<Output = Result<ReplayEffects, ReplayFailure>> + Send {
        std::future::ready(match self.0 {
            0 => Ok(ReplayEffects {
                outputs: vec![],
                state: vec![1],
            }),
            1 => Err(ReplayFailure::Rejected),
            2 => Err(ReplayFailure::Unavailable),
            _ => Ok(ReplayEffects {
                outputs: vec![],
                state: vec![0; REPLAY_STATE_LIMIT + 1],
            }),
        })
    }
}
#[tokio::test]
async fn worker_never_advances_on_rejection_unavailability_invalid_effects_or_lost_lease() {
    for (mode, accept, expected) in [
        (0, true, ReplayTick::Completed),
        (0, false, ReplayTick::LostLease),
        (1, true, ReplayTick::Poisoned),
        (2, true, ReplayTick::Deferred),
        (3, true, ReplayTick::Poisoned),
    ] {
        let repo = Repository {
            lease: Mutex::new(Some(ReplayLease {
                work: work(),
                generation: 1,
            })),
            completed: AtomicBool::new(false),
            poisoned: AtomicBool::new(false),
            accept,
        };
        let executor = Executor(mode);
        let worker = ReplayWorker::new(&repo, &executor);
        assert_eq!(worker.run_once().await.unwrap(), expected);
        assert_eq!(
            repo.completed.load(Ordering::SeqCst),
            expected == ReplayTick::Completed
        );
        assert_eq!(
            repo.poisoned.load(Ordering::SeqCst),
            expected == ReplayTick::Poisoned
        );
        assert_eq!(worker.run_once().await.unwrap(), ReplayTick::Idle);
    }
}
#[test]
fn replay_input_bounds_and_debug_do_not_expose_frame_or_state() {
    let mut w = work();
    assert!(w.validate().is_ok());
    assert!(!format!("{w:?}").contains("pre_state"));
    w.sequence = 0;
    assert!(w.validate().is_err());
    w.sequence = 1;
    w.pre_state = vec![9];
    assert!(w.validate().is_err());
}
