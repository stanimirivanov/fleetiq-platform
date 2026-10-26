use super::*;
use crate::*;
use fleetiq_domain::{ActorId, AssetId, TenantId, UnixTimeMillis};
use fleetiq_protocol_sdk::{DeviceIdentityClaim, ProposedReply, ReplyRelease};
use fleetiq_telemetry::{ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion};
use std::{
    io,
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Clone)]
struct Routing {
    snapshot: ProtocolSessionRoute,
    active: Arc<AtomicBool>,
}
impl ProtocolRoutingRepository for Routing {
    type Error = io::Error;
    fn find(
        &self,
        _: &TenantId,
        _: &AssetId,
        _: &ProtocolChannelId,
    ) -> impl std::future::Future<Output = io::Result<Option<ProtocolDeviceRoute>>> + Send {
        let result = { Ok(None) };
        std::future::ready(result)
    }
    fn change(
        &self,
        _: &ProtocolRouteCommand,
    ) -> impl std::future::Future<Output = io::Result<ProtocolRouteWrite>> + Send {
        let result = { Ok(ProtocolRouteWrite::Unavailable) };
        std::future::ready(result)
    }
    fn resolve(
        &self,
        channel: &RegisteredIdentityChannel,
        claim: &DeviceIdentityClaim,
    ) -> impl std::future::Future<Output = io::Result<Option<ProtocolSessionRoute>>> + Send {
        let result = {
            Ok((self.active.load(Ordering::SeqCst)
                && channel.tenant_id == self.snapshot.route.tenant_id
                && claim.kind() == "serial"
                && claim.value() == "registered")
                .then(|| self.snapshot.clone()))
        };
        std::future::ready(result)
    }
}
impl LiveRoutingRepository for Routing {
    fn bootstrap(
        &self,
        _: &RegisteredIdentityChannel,
    ) -> impl std::future::Future<Output = io::Result<Option<ProtocolPackageRelease>>> + Send {
        let result = {
            Ok(self
                .active
                .load(Ordering::SeqCst)
                .then(|| self.snapshot.release.clone()))
        };
        std::future::ready(result)
    }
}
#[derive(Default)]
struct Stored {
    events: Vec<&'static str>,
    work: Vec<ReplayWork>,
    completed: usize,
    fail_enqueue: bool,
    fail_complete: bool,
    lost_claim: bool,
    replies: Vec<Vec<u8>>,
}
#[derive(Clone, Default)]
struct Repository(Arc<Mutex<Stored>>);
impl ReplayRepository for Repository {
    type Error = io::Error;
    fn enqueue(
        &self,
        work: &ReplayWork,
    ) -> impl std::future::Future<Output = io::Result<bool>> + Send {
        let result = {
            let mut s = self.0.lock().unwrap();
            s.events.push("accept");
            s.work.push(work.clone());
            if s.fail_enqueue {
                Err(io::Error::other("private database diagnostic"))
            } else {
                Ok(true)
            }
        };
        std::future::ready(result)
    }
    fn claim(&self) -> impl std::future::Future<Output = io::Result<Option<ReplayLease>>> + Send {
        std::future::poll_fn(|_| panic!("live processing must never claim unrelated work"))
    }
    fn complete(
        &self,
        _: &ReplayLease,
        _: &DecodedObservationBatch,
        _: &[u8],
    ) -> impl std::future::Future<Output = io::Result<bool>> + Send {
        let result = {
            let mut s = self.0.lock().unwrap();
            s.events.push("complete");
            if s.fail_complete {
                Ok(false)
            } else {
                s.completed += 1;
                Ok(true)
            }
        };
        std::future::ready(result)
    }
    fn poison(
        &self,
        _: &ReplayLease,
    ) -> impl std::future::Future<Output = io::Result<bool>> + Send {
        let result = { Ok(false) };
        std::future::ready(result)
    }
}
impl LiveReplayRepository for Repository {
    fn claim_exact(
        &self,
        message: &AcceptedMessage,
    ) -> impl std::future::Future<Output = io::Result<Option<ReplayLease>>> + Send {
        let result = {
            let mut s = self.0.lock().unwrap();
            s.events.push("claim");
            Ok((!s.lost_claim).then(|| ReplayLease {
                work: s
                    .work
                    .iter()
                    .find(|w| &w.message == message)
                    .unwrap()
                    .clone(),
                generation: 1,
            }))
        };
        std::future::ready(result)
    }
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Behavior {
    Normal,
    Revoke,
    Invalid,
    Mixed,
    Stall,
}
#[derive(Clone)]
struct Executor {
    active: Arc<AtomicBool>,
    behavior: Behavior,
}
impl LiveExecutor for Executor {
    async fn prepare(
        &self,
        _: &ProtocolPackageRelease,
        unread: &[u8],
        state: &[u8],
        identify: bool,
    ) -> Result<Option<LivePrepared>, LiveExecutionFailure> {
        if self.behavior == Behavior::Stall {
            std::future::pending::<()>().await;
        }
        if unread.len() < 2 {
            return Ok(None);
        }
        if self.behavior == Behavior::Revoke {
            self.active.store(false, Ordering::SeqCst);
        }
        let ready = identify;
        let state = if self.behavior == Behavior::Invalid {
            vec![1; 16385]
        } else if identify {
            vec![1]
        } else {
            let mut s = state.to_vec();
            s.push(unread[1]);
            s
        };
        Ok(Some(LivePrepared {
            consumed: 2,
            identity: identify
                .then(|| DeviceIdentityClaim::new("serial".into(), "registered".into()).unwrap()),
            state,
            outputs: vec![],
            replies: if self.behavior == Behavior::Mixed {
                vec![
                    ProposedReply::new(b"A".as_slice(), ReplyRelease::AfterDurableAcceptance)
                        .unwrap(),
                    ProposedReply::new(b"P".as_slice(), ReplyRelease::AfterProcessing).unwrap(),
                ]
            } else {
                vec![
                    ProposedReply::new(
                        if ready { vec![b'P'] } else { vec![b'A'] },
                        if ready {
                            ReplyRelease::AfterProcessing
                        } else {
                            ReplyRelease::AfterDurableAcceptance
                        },
                    )
                    .unwrap(),
                ]
            },
        }))
    }
}
impl LiveReplySink for Repository {
    fn send(&mut self, bytes: &[u8]) -> impl std::future::Future<Output = io::Result<()>> + Send {
        let result = {
            let mut s = self.0.lock().unwrap();
            s.events.push(if bytes == b"P" {
                "processing-reply"
            } else {
                "durable-reply"
            });
            s.replies.push(bytes.to_vec());
            Ok(())
        };
        std::future::ready(result)
    }
}
fn setup() -> (
    LiveSession<Routing, Repository, Executor>,
    Repository,
    Routing,
    RegisteredIdentityChannel,
) {
    let tenant = TenantId::new("tenant-a").unwrap();
    let artifact = ProtocolArtifact::new(vec![1]).unwrap();
    let package = ProtocolPackageRef::new(
        ProtocolPackageId::new("custom").unwrap(),
        ProtocolPackageVersion::new("1").unwrap(),
    );
    let selection = ProtocolRouteSelection {
        package: package.clone(),
        world: ProtocolAdmissionWorld::Identity,
    };
    let channel = RegisteredIdentityChannel {
        tenant_id: tenant.clone(),
        channel_id: ProtocolChannelId::new("tcp").unwrap(),
        authority: None,
        selection: selection.clone(),
    };
    let routing = Routing {
        active: Arc::new(AtomicBool::new(true)),
        snapshot: ProtocolSessionRoute {
            release: ProtocolPackageRelease::new(
                tenant.clone(),
                package,
                artifact.digest(),
                artifact.size_bytes(),
            ),
            route: ProtocolDeviceRoute {
                tenant_id: tenant,
                device_asset_id: AssetId::new("device-a").unwrap(),
                channel_id: channel.channel_id.clone(),
                revision: NonZeroU32::new(1).unwrap(),
                selection: Some(selection),
                policy_version: Some(PROTOCOL_ADMISSION_POLICY),
                actor_id: ActorId::new("operator").unwrap(),
                recorded_at: UnixTimeMillis::new(1),
            },
        },
    };
    let repo = Repository::default();
    let executor = Executor {
        active: routing.active.clone(),
        behavior: Behavior::Normal,
    };
    (
        LiveSession::new(
            [1; 16],
            channel.clone(),
            routing.clone(),
            repo.clone(),
            executor,
        ),
        repo,
        routing,
        channel,
    )
}
#[tokio::test]
async fn fragments_and_coalesced_frames_preserve_gates_prestate_and_sequence() {
    let (mut session, mut repo, _, _) = setup();
    let now = UnixTimeMillis::new(10);
    session.push(b"H").unwrap();
    assert_eq!(
        session.step(now, &mut repo).await.unwrap(),
        LiveStep::NeedMore
    );
    assert_eq!(repo.0.lock().unwrap().events.len(), 0);
    session.push(&[0, b'D', 7]).unwrap();
    assert_eq!(
        session.step(now, &mut repo).await.unwrap(),
        LiveStep::Committed
    );
    assert_eq!(
        session.step(now, &mut repo).await.unwrap(),
        LiveStep::Committed
    );
    assert!(!session.has_input());
    let s = repo.0.lock().unwrap();
    assert_eq!(
        s.events,
        [
            "processing-reply",
            "accept",
            "claim",
            "complete",
            "accept",
            "durable-reply",
            "claim",
            "complete"
        ]
    );
    assert_eq!(s.completed, 2);
    assert_eq!(s.work[0].pre_state.len(), 0);
    assert_eq!(s.work[1].pre_state, [1]);
    assert_eq!(s.work[1].sequence, 2);
    assert_eq!(s.work[0].message.payload().as_bytes(), b"H\0");
}
#[tokio::test]
async fn ambiguous_acceptance_never_releases_durable_reply_or_reconstructs_retry() {
    let (mut session, mut repo, _, _) = setup();
    let now = UnixTimeMillis::new(10);
    session.push(b"H0").unwrap();
    session.step(now, &mut repo).await.unwrap();
    {
        let mut s = repo.0.lock().unwrap();
        s.fail_enqueue = true;
        s.replies.clear();
    }
    session.push(b"D1").unwrap();
    let error = session.step(now, &mut repo).await.unwrap_err();
    assert!(!format!("{error:?} {error}").contains("private"));
    assert_eq!(repo.0.lock().unwrap().replies.len(), 0);
    assert!(matches!(
        session.step(now, &mut repo).await,
        Err(LiveSessionError::Rejected)
    ));
    assert_eq!(repo.0.lock().unwrap().work.len(), 2);
}
#[tokio::test]
async fn completion_loss_closes_live_owner_without_rewinding_or_skipping_work() {
    let (mut session, mut repo, _, _) = setup();
    let now = UnixTimeMillis::new(10);
    session.push(b"H0").unwrap();
    session.step(now, &mut repo).await.unwrap();
    repo.0.lock().unwrap().fail_complete = true;
    session.push(b"D1").unwrap();
    assert!(matches!(
        session.step(now, &mut repo).await,
        Err(LiveSessionError::LostLease)
    ));
    assert_eq!(repo.0.lock().unwrap().completed, 1);
    assert_eq!(repo.0.lock().unwrap().replies.last().unwrap(), b"A");
    assert!(session.step(now, &mut repo).await.is_err());
}
#[tokio::test]
async fn revocation_and_invalid_effects_expose_no_partial_reply_or_persistence() {
    for revoke in [true, false] {
        let (mut session, mut repo, _, _) = setup();
        session.executor.behavior = if revoke {
            Behavior::Revoke
        } else {
            Behavior::Invalid
        };
        session.push(b"H0").unwrap();
        assert!(
            session
                .step(UnixTimeMillis::new(1), &mut repo)
                .await
                .is_err()
        );
        assert_eq!(repo.0.lock().unwrap().events.len(), 0);
    }
}
#[test]
fn oversized_buffer_latches_terminal_failure_without_allocating_it() {
    let (mut session, _, _, _) = setup();
    assert!(session.push(&vec![0; 65537]).is_err());
    assert_eq!(session.available(), 65536);
    assert!(session.push(b"x").is_err());
}

#[tokio::test]
async fn a_ready_reply_never_overtakes_an_earlier_deferred_reply() {
    let (mut session, mut repo, _, _) = setup();
    session.executor.behavior = Behavior::Mixed;
    session.push(b"H0").unwrap();
    session
        .step(UnixTimeMillis::new(1), &mut repo)
        .await
        .unwrap();
    let stored = repo.0.lock().unwrap();
    assert_eq!(
        stored.events,
        [
            "accept",
            "durable-reply",
            "processing-reply",
            "claim",
            "complete"
        ]
    );
    assert_eq!(stored.replies, [b"A".to_vec(), b"P".to_vec()]);
}
#[tokio::test]
async fn dropping_an_inflight_turn_permanently_invalidates_its_connection_owner() {
    let (mut session, mut repo, _, _) = setup();
    session.executor.behavior = Behavior::Stall;
    session.push(b"H0").unwrap();
    {
        let mut future = std::pin::pin!(session.step(UnixTimeMillis::new(1), &mut repo));
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
    }
    assert!(session.push(b"D1").is_err());
    assert_eq!(repo.0.lock().unwrap().events, [] as [&str; 0]);
}
