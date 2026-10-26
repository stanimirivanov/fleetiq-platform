use super::*;
use fleetiq_domain::{ActorId, DeviceEndpointId, SourceSignalId, TenantId, UnixTimeMillis};
use fleetiq_ingestion::{
    DecodedObservationBatch, ProtocolAdmission, ProtocolAdmissionWorld, ProtocolArtifact,
    ProtocolArtifactStore, ProtocolChannelId, ProtocolPackageCatalog, ProtocolPackageRelease,
    ProtocolRouteCommand, ProtocolRouteSelection, ProtocolRoutingRepository, ReplayEffects,
    ReplayExecutor, ReplayFailure, ReplayLease, ReplayRepository, ReplayWork,
};
use fleetiq_postgres::{
    PostgresProtocolArtifactStore, PostgresProtocolPackageCatalog, PostgresReplayError,
};
use fleetiq_protocol_sdk::{
    DecodedMeasurement, DecodedOutput, DecodedValue, DeviceIdentityClaim, ObservationQuality,
    ProposedReply, ReplyRelease,
};
use fleetiq_telemetry::{
    AcceptedMessage, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use tokio::sync::mpsc;
pub(super) fn channel() -> RegisteredIdentityChannel {
    RegisteredIdentityChannel {
        tenant_id: TenantId::new("tenant-a").unwrap(),
        channel_id: ProtocolChannelId::new("tcp").unwrap(),
        authority: None,
        selection: ProtocolRouteSelection {
            world: ProtocolAdmissionWorld::Identity,
            package: ProtocolPackageRef::new(
                ProtocolPackageId::new("fixture.tcp").unwrap(),
                ProtocolPackageVersion::new("1").unwrap(),
            ),
        },
    }
}
pub(super) async fn seed(pool: &sqlx::PgPool) -> RegisteredIdentityChannel {
    sqlx::raw_sql("INSERT INTO metadata.tenants(tenant_id) VALUES('tenant-a'),('tenant-b'); INSERT INTO identity.actors(actor_id) VALUES('operator-a'); INSERT INTO metadata.asset_type_definitions(tenant_id,type_id,version,display_name,property_count) VALUES('tenant-a','equipment',1,'Equipment',0); INSERT INTO metadata.assets(tenant_id,asset_id,type_id,type_version,display_name) VALUES('tenant-a','device-a','equipment',1,'Device'); INSERT INTO metadata.communication_devices(tenant_id,device_asset_id) VALUES('tenant-a','device-a'); INSERT INTO metadata.asset_external_identifiers(tenant_id,asset_id,identifier_kind,authority,identifier_value) VALUES('tenant-a','device-a','serial',NULL,'registered');").execute(pool).await.unwrap();
    let channel = channel();
    let artifact = ProtocolArtifact::new(b"synthetic-live-fixture".to_vec()).unwrap();
    let release = ProtocolPackageRelease::new(
        channel.tenant_id.clone(),
        channel.selection.package.clone(),
        artifact.digest(),
        artifact.size_bytes(),
    );
    PostgresProtocolPackageCatalog::new(pool.clone())
        .register(&release)
        .await
        .unwrap();
    let store = PostgresProtocolArtifactStore::new(pool.clone());
    store.put(&release, &artifact).await.unwrap();
    store
        .record(&ProtocolAdmission {
            release,
            world: ProtocolAdmissionWorld::Identity,
            policy_version: 1,
            actor_id: ActorId::new("operator-a").unwrap(),
            admitted_at: UnixTimeMillis::new(1),
        })
        .await
        .unwrap();
    PostgresProtocolRoutingRepository::new(pool.clone())
        .change(&ProtocolRouteCommand {
            tenant_id: channel.tenant_id.clone(),
            device_asset_id: fleetiq_domain::AssetId::new("device-a").unwrap(),
            channel_id: channel.channel_id.clone(),
            expected_revision: 0,
            selection: Some(channel.selection.clone()),
            actor_id: ActorId::new("operator-a").unwrap(),
            recorded_at: UnixTimeMillis::new(1),
        })
        .await
        .unwrap();
    channel
}
#[derive(Clone)]
pub(super) struct Execution {
    pub polls: mpsc::Sender<usize>,
}
fn outputs(value: u8) -> Vec<DecodedOutput> {
    vec![DecodedOutput::new(
        DeviceEndpointId::new("main").unwrap(),
        SourceSignalId::new("value").unwrap(),
        DecodedMeasurement::new(
            DecodedValue::Integer(i64::from(value)),
            None,
            ObservationQuality::Good,
        )
        .unwrap(),
        None,
    )]
}
impl LiveExecutor for Execution {
    async fn prepare(
        &self,
        _: &ProtocolPackageRelease,
        unread: &[u8],
        _: &[u8],
        identify: bool,
    ) -> Result<Option<fleetiq_ingestion::LivePrepared>, fleetiq_ingestion::LiveExecutionFailure>
    {
        self.polls.send(unread.len()).await.unwrap();
        if unread.len() < 2 {
            return Ok(None);
        }
        Ok(Some(fleetiq_ingestion::LivePrepared {
            consumed: 2,
            identity: identify.then(|| {
                DeviceIdentityClaim::new(
                    "serial".into(),
                    if unread[0] == b'X' {
                        "missing"
                    } else {
                        "registered"
                    }
                    .into(),
                )
                .unwrap()
            }),
            state: vec![1],
            outputs: if identify { vec![] } else { outputs(unread[1]) },
            replies: vec![
                ProposedReply::new(
                    if identify { vec![b'P'] } else { vec![b'A'] },
                    if identify {
                        ReplyRelease::AfterProcessing
                    } else {
                        ReplyRelease::AfterDurableAcceptance
                    },
                )
                .unwrap(),
            ],
        }))
    }
}
impl ReplayExecutor for Execution {
    fn execute(
        &self,
        work: &ReplayWork,
    ) -> impl std::future::Future<Output = Result<ReplayEffects, ReplayFailure>> + Send {
        let result = {
            Ok(ReplayEffects {
                outputs: outputs(work.message.payload().as_bytes()[1]),
                state: vec![1],
            })
        };
        std::future::ready(result)
    }
}
#[derive(Clone)]
pub(super) struct Observed {
    pub inner: PostgresReplayRepository,
    pub completed: mpsc::Sender<u32>,
}
impl ReplayRepository for Observed {
    type Error = PostgresReplayError;
    async fn enqueue(&self, w: &ReplayWork) -> Result<bool, Self::Error> {
        self.inner.enqueue(w).await
    }
    async fn claim(&self) -> Result<Option<ReplayLease>, Self::Error> {
        self.inner.claim().await
    }
    async fn complete(
        &self,
        l: &ReplayLease,
        b: &DecodedObservationBatch,
        s: &[u8],
    ) -> Result<bool, Self::Error> {
        let result = self.inner.complete(l, b, s).await?;
        if result {
            self.completed.send(l.work.sequence).await.unwrap();
        }
        Ok(result)
    }
    async fn poison(&self, l: &ReplayLease) -> Result<bool, Self::Error> {
        self.inner.poison(l).await
    }
}
impl LiveReplayRepository for Observed {
    async fn claim_exact(&self, m: &AcceptedMessage) -> Result<Option<ReplayLease>, Self::Error> {
        self.inner.claim_exact(m).await
    }
}
pub(super) async fn reply(client: &mut tokio::net::TcpStream, expected: u8) {
    let byte = tokio::time::timeout(Duration::from_secs(10), client.read_u8())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(byte, expected);
}
pub(super) async fn closed(client: &mut tokio::net::TcpStream) {
    let mut byte = [0];
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(10), client.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}
pub(super) async fn completed(receiver: &mut mpsc::Receiver<u32>, sequence: u32) {
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(10), receiver.recv())
            .await
            .unwrap()
            .unwrap(),
        sequence
    );
}
