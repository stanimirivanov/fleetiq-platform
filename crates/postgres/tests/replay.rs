//! Durable replay tests use real locks/transactions and explicit expiry, not sleeps.
use fleetiq_domain::{ActorId, AssetId, TenantId, UnixTimeMillis};
use fleetiq_ingestion::{
    PROTOCOL_ADMISSION_POLICY, ProtocolAdmission, ProtocolAdmissionWorld, ProtocolArtifact,
    ProtocolArtifactStore, ProtocolPackageCatalog, ProtocolPackageRelease, ReplayEffects,
    ReplayLease, ReplayRepository, ReplayWork,
};
use fleetiq_postgres::{
    PostgresProtocolArtifactStore, PostgresProtocolPackageCatalog, PostgresReplayRepository,
};
use fleetiq_telemetry::{
    AcceptedMessage, AcceptedMessageId, AcceptedPayload, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
use sqlx::PgPool;
async fn seed(pool: &PgPool) -> ReplayWork {
    sqlx::raw_sql("INSERT INTO metadata.tenants(tenant_id) VALUES('tenant-a'),('tenant-b'); INSERT INTO identity.actors(actor_id) VALUES('operator-a'); INSERT INTO metadata.asset_type_definitions(tenant_id,type_id,version,display_name,property_count) VALUES('tenant-a','equipment',1,'Equipment',0),('tenant-b','equipment',1,'Equipment',0); INSERT INTO metadata.assets(tenant_id,asset_id,type_id,type_version,display_name) VALUES('tenant-a','device-a','equipment',1,'Device'),('tenant-b','device-a','equipment',1,'Device'); INSERT INTO metadata.communication_devices(tenant_id,device_asset_id) VALUES('tenant-a','device-a'),('tenant-b','device-a');").execute(pool).await.unwrap();
    let artifact = ProtocolArtifact::new(b"replay-fixture-component".to_vec()).unwrap();
    let package = ProtocolPackageRef::new(
        ProtocolPackageId::new("fixture.custom").unwrap(),
        ProtocolPackageVersion::new("1").unwrap(),
    );
    let release = ProtocolPackageRelease::new(
        TenantId::new("tenant-a").unwrap(),
        package.clone(),
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
            policy_version: PROTOCOL_ADMISSION_POLICY,
            actor_id: ActorId::new("operator-a").unwrap(),
            admitted_at: UnixTimeMillis::new(1),
        })
        .await
        .unwrap();
    ReplayWork {
        message: AcceptedMessage::new(
            AcceptedMessageId::new("message-1").unwrap(),
            TenantId::new("tenant-a").unwrap(),
            AssetId::new("device-a").unwrap(),
            package,
            UnixTimeMillis::new(100),
            AcceptedPayload::new(b"handshake".as_slice()),
        ),
        session: "session-1".into(),
        sequence: 1,
        digest: artifact.digest(),
        world: ProtocolAdmissionWorld::Identity,
        execution: fleetiq_protocol_sdk::replay_wire::replay_fingerprint(),
        pre_state: vec![],
    }
}
fn next(work: &ReplayWork, state: Vec<u8>) -> ReplayWork {
    let m = &work.message;
    ReplayWork {
        message: AcceptedMessage::new(
            AcceptedMessageId::new(format!("message-{}", work.sequence + 1)).unwrap(),
            m.tenant_id().clone(),
            m.device_asset_id().clone(),
            m.protocol_package().clone(),
            m.received_at(),
            AcceptedPayload::new(b"sample".as_slice()),
        ),
        sequence: work.sequence + 1,
        pre_state: state,
        ..work.clone()
    }
}
async fn expire(pool: &PgPool, lease: &ReplayLease) {
    sqlx::query(
        "UPDATE ingestion.replay_work SET lease_until_ms=0 WHERE tenant_id=$1 AND message_id=$2",
    )
    .bind(lease.work.message.tenant_id().as_str())
    .bind(lease.work.message.id().as_str())
    .execute(pool)
    .await
    .unwrap();
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn durable_input_retry_and_restart_preserve_exact_context(pool: PgPool) {
    let work = seed(&pool).await;
    let repo = PostgresReplayRepository::new(pool.clone());
    assert!(repo.enqueue(&work).await.unwrap());
    assert!(repo.enqueue(&work).await.unwrap());
    let mut collision = work.clone();
    collision.pre_state = vec![9];
    assert!(repo.enqueue(&collision).await.is_err());
    collision = work.clone();
    collision.session = "another-session".into();
    assert!(!repo.enqueue(&collision).await.unwrap());
    assert!(!repo.enqueue(&next(&work, vec![1])).await.unwrap());
    let lease = PostgresReplayRepository::new(pool.clone())
        .claim()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.work, work);
    assert!(repo.claim().await.unwrap().is_none());
    expire(&pool, &lease).await;
    let recovered = repo.claim().await.unwrap().unwrap();
    assert_eq!(recovered.work, lease.work);
    assert!(recovered.generation > lease.generation);
    let (batch, state) = ReplayEffects {
        outputs: vec![],
        state: vec![1],
    }
    .batch(&work)
    .unwrap();
    assert!(!repo.complete(&lease, &batch, &state).await.unwrap());
    assert!(repo.complete(&recovered, &batch, &state).await.unwrap());
    // Simulate a lost successful completion response: same receipt is safe even
    // after the lease deadline and after a later turn commits.
    assert!(repo.complete(&recovered, &batch, &state).await.unwrap());
    let second = next(&work, state);
    assert!(repo.enqueue(&second).await.unwrap());
    let second_lease = repo.claim().await.unwrap().unwrap();
    let (batch2, state2) = ReplayEffects {
        outputs: vec![],
        state: vec![2],
    }
    .batch(&second)
    .unwrap();
    assert!(
        repo.complete(&second_lease, &batch2, &state2)
            .await
            .unwrap()
    );
    assert!(repo.complete(&recovered, &batch, &[1]).await.unwrap());
    assert!(!repo.complete(&recovered, &batch, &[3]).await.unwrap());
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn competing_claims_have_one_winner_and_poison_blocks_successors(pool: PgPool) {
    let work = seed(&pool).await;
    let repo = PostgresReplayRepository::new(pool.clone());
    repo.enqueue(&work).await.unwrap();
    let other = repo.clone();
    let (a, b) = tokio::join!(repo.claim(), other.claim());
    let claims = [a.unwrap(), b.unwrap()];
    assert_eq!(claims.iter().filter(|v| v.is_some()).count(), 1);
    let lease = claims.into_iter().flatten().next().unwrap();
    assert!(repo.poison(&lease).await.unwrap());
    assert!(repo.claim().await.unwrap().is_none());
    assert!(!repo.enqueue(&next(&work, vec![])).await.unwrap());
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn abandoned_attempts_are_bounded_and_preserve_poison_evidence(pool: PgPool) {
    let work = seed(&pool).await;
    let repo = PostgresReplayRepository::new(pool.clone());
    repo.enqueue(&work).await.unwrap();
    for generation in 1..=3 {
        let lease = repo.claim().await.unwrap().unwrap();
        assert_eq!(lease.generation, generation);
        expire(&pool, &lease).await;
    }
    assert!(repo.claim().await.unwrap().is_none());
    let status: String = sqlx::query_scalar("SELECT status FROM ingestion.replay_work")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "poisoned");
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn checkpoint_failure_rolls_back_all_outputs_and_allows_exact_recovery(pool: PgPool) {
    let work = seed(&pool).await;
    let repo = PostgresReplayRepository::new(pool.clone());
    repo.enqueue(&work).await.unwrap();
    let lease = repo.claim().await.unwrap().unwrap();
    // Fault after inserting a nonempty result, before checkpoint commit: neither
    // the observation nor its completion marker may survive the failed commit.
    sqlx::raw_sql("CREATE FUNCTION ingestion.fail_replay_fixture() RETURNS TRIGGER LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture checkpoint refusal'; END $$; CREATE TRIGGER fixture_checkpoint_refusal BEFORE UPDATE ON ingestion.replay_sessions FOR EACH ROW EXECUTE FUNCTION ingestion.fail_replay_fixture();").execute(&pool).await.unwrap();
    let output = fleetiq_protocol_sdk::DecodedOutput::new(
        fleetiq_domain::DeviceEndpointId::new("main").unwrap(),
        fleetiq_domain::SourceSignalId::new("sample").unwrap(),
        fleetiq_telemetry::DecodedMeasurement::new(
            fleetiq_telemetry::DecodedValue::Integer(42),
            None,
            fleetiq_telemetry::ObservationQuality::Good,
        )
        .unwrap(),
        None,
    );
    let (batch, state) = ReplayEffects {
        outputs: vec![output],
        state: vec![1],
    }
    .batch(&work)
    .unwrap();
    assert!(repo.complete(&lease, &batch, &state).await.is_err());
    let observations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ingestion.decoded_observations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(observations, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ingestion.decoded_message_results")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    sqlx::raw_sql("DROP TRIGGER fixture_checkpoint_refusal ON ingestion.replay_sessions;")
        .execute(&pool)
        .await
        .unwrap();
    assert!(repo.complete(&lease, &batch, &state).await.unwrap());
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn tenant_and_immutable_context_cannot_be_rebound(pool: PgPool) {
    let work = seed(&pool).await;
    let repo = PostgresReplayRepository::new(pool.clone());
    repo.enqueue(&work).await.unwrap();
    assert!(
        sqlx::query("UPDATE ingestion.replay_work SET pre_state=$1")
            .bind(vec![1_u8])
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE ingestion.replay_sessions SET artifact_sha256=$1")
            .bind(vec![0_u8; 32])
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE ingestion.replay_work SET status='completed',next_state=''::bytea")
            .execute(&pool)
            .await
            .is_err()
    );
    let m = &work.message;
    let mut foreign = work.clone();
    foreign.message = AcceptedMessage::new(
        m.id().clone(),
        TenantId::new("tenant-b").unwrap(),
        m.device_asset_id().clone(),
        m.protocol_package().clone(),
        m.received_at(),
        m.payload().clone(),
    );
    assert!(repo.enqueue(&foreign).await.is_err());
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn incompatible_execution_is_neither_claimed_nor_quarantined(pool: PgPool) {
    let work = seed(&pool).await;
    let repo = PostgresReplayRepository::new(pool.clone());
    let mut incompatible = work.clone();
    incompatible.execution = [0; 32];
    assert!(repo.enqueue(&incompatible).await.is_err());
    // Model a session written by a prior immutable runtime, through normal INSERT
    // constraints. A new worker must leave both pending and exhausted work alone.
    sqlx::query("INSERT INTO ingestion.replay_sessions(tenant_id,session_id,device_asset_id,protocol_package_id,protocol_package_version,artifact_sha256,wit_world,admission_policy,execution_policy,execution_sha256) VALUES('tenant-a','session-1','device-a','fixture.custom','1',$1,'identity-v1',1,1,$2)")
        .bind(work.digest.as_bytes().as_slice()).bind([0_u8; 32].as_slice()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ingestion.accepted_messages(tenant_id,message_id,device_asset_id,protocol_package_id,protocol_package_version,received_at_ms,payload) VALUES('tenant-a','message-1','device-a','fixture.custom','1',100,$1)")
        .bind(work.message.payload().as_bytes()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ingestion.replay_work(tenant_id,message_id,session_id,sequence,pre_state) VALUES('tenant-a','message-1','session-1',1,''::bytea)")
        .execute(&pool).await.unwrap();
    assert!(repo.claim().await.unwrap().is_none());
    sqlx::query("UPDATE ingestion.replay_work SET generation=3,lease_until_ms=0")
        .execute(&pool)
        .await
        .unwrap();
    assert!(repo.claim().await.unwrap().is_none());
    let status: String = sqlx::query_scalar("SELECT status FROM ingestion.replay_work")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "pending");
    assert!(!repo.enqueue(&work).await.unwrap());
}
