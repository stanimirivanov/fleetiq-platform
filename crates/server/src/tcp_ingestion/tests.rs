//! Real socket/SQL tests compose production policy and adapters with a deterministic executor.
mod fixtures;
use super::*;
use fixtures::*;
use fleetiq_ingestion::{
    LiveExecutor, LiveReplayRepository, RegisteredIdentityChannel, ReplayTick, ReplayWorker,
};
use sqlx::PgPool;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
};
struct Running {
    address: std::net::SocketAddr,
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
    polls: mpsc::Receiver<usize>,
    done: mpsc::Receiver<u32>,
}
async fn start(pool: &PgPool, channel: RegisteredIdentityChannel) -> Running {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, shutdown) = oneshot::channel();
    let (polls, received) = mpsc::channel(32);
    let (done, completed) = mpsc::channel(32);
    let repo = Observed {
        inner: PostgresReplayRepository::new(pool.clone()),
        completed: done,
    };
    let task = tokio::spawn(transport::listen(
        listener,
        channel,
        PostgresProtocolRoutingRepository::new(pool.clone()),
        repo,
        Execution { polls },
        async {
            let _ = shutdown.await;
        },
    ));
    Running {
        address,
        stop,
        task,
        polls: received,
        done: completed,
    }
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn real_tcp_fragments_coalescing_reconnect_and_trusted_provenance(pool: PgPool) {
    let channel = seed(&pool).await;
    let Running {
        address,
        stop,
        task,
        mut polls,
        mut done,
    } = start(&pool, channel).await;
    let mut client = TcpStream::connect(address).await.unwrap();
    client.write_all(b"H").await.unwrap();
    assert_eq!(polls.recv().await, Some(1));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ingestion.accepted_messages")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    client.write_all(&[0, b'D', 7, b'D', 8]).await.unwrap();
    reply(&mut client, b'P').await;
    reply(&mut client, b'A').await;
    reply(&mut client, b'A').await;
    for sequence in 1..=3 {
        completed(&mut done, sequence).await;
    }
    drop(client);
    let mut reconnected = TcpStream::connect(address).await.unwrap();
    reconnected.write_all(b"H0D7").await.unwrap();
    reply(&mut reconnected, b'P').await;
    reply(&mut reconnected, b'A').await;
    completed(&mut done, 1).await;
    completed(&mut done, 2).await;
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM ingestion.accepted_messages WHERE tenant_id='tenant-a' AND device_asset_id='device-a'").fetch_one(&pool).await.unwrap(),5);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ingestion.decoded_observations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ingestion.replay_sessions")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    closed(&mut reconnected).await;
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn failed_checkpoint_keeps_raw_input_for_reply_free_recovery(pool: PgPool) {
    let channel = seed(&pool).await;
    let Running {
        address,
        stop,
        task,
        polls: _polls,
        mut done,
    } = start(&pool, channel).await;
    let mut client = TcpStream::connect(address).await.unwrap();
    client.write_all(b"H0").await.unwrap();
    reply(&mut client, b'P').await;
    completed(&mut done, 1).await;
    // Failure after result insertion must roll back the complete result/checkpoint transaction.
    sqlx::raw_sql("CREATE FUNCTION ingestion.reject_test_checkpoint() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'synthetic checkpoint failure'; END $$; CREATE TRIGGER reject_test_checkpoint BEFORE UPDATE ON ingestion.replay_sessions FOR EACH ROW WHEN (NEW.current_sequence > 1) EXECUTE FUNCTION ingestion.reject_test_checkpoint();").execute(&pool).await.unwrap();
    client.write_all(b"D7").await.unwrap();
    reply(&mut client, b'A').await;
    closed(&mut client).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_sequence FROM ingestion.replay_sessions")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ingestion.decoded_observations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM ingestion.replay_work WHERE status='pending'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    sqlx::raw_sql("DROP TRIGGER reject_test_checkpoint ON ingestion.replay_sessions; UPDATE ingestion.replay_work SET lease_until_ms=0 WHERE status='pending'").execute(&pool).await.unwrap();
    let (polls, _) = mpsc::channel(1);
    let execution = Execution { polls };
    let repository = PostgresReplayRepository::new(pool.clone());
    assert_eq!(
        ReplayWorker::new(&repository, &execution)
            .run_once()
            .await
            .unwrap(),
        ReplayTick::Completed
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_sequence FROM ingestion.replay_sessions")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ingestion.decoded_observations")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn unknown_claims_overload_and_shutdown_are_fail_closed(pool: PgPool) {
    let channel = seed(&pool).await;
    let Running {
        address,
        stop,
        task,
        polls: _polls,
        mut done,
    } = start(&pool, channel).await;
    let mut unknown = TcpStream::connect(address).await.unwrap();
    unknown.write_all(b"X0").await.unwrap();
    closed(&mut unknown).await;
    let mut one = TcpStream::connect(address).await.unwrap();
    one.write_all(b"H0").await.unwrap();
    reply(&mut one, b'P').await;
    completed(&mut done, 1).await;
    let mut two = TcpStream::connect(address).await.unwrap();
    two.write_all(b"H0").await.unwrap();
    reply(&mut two, b'P').await;
    completed(&mut done, 1).await;
    let mut excess = TcpStream::connect(address).await.unwrap();
    closed(&mut excess).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ingestion.accepted_messages")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    closed(&mut one).await;
    closed(&mut two).await;
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn tenant_suspension_prevents_an_existing_socket_next_turn(pool: PgPool) {
    let channel = seed(&pool).await;
    let Running {
        address,
        stop,
        task,
        polls: _polls,
        mut done,
    } = start(&pool, channel).await;
    let mut client = TcpStream::connect(address).await.unwrap();
    client.write_all(b"H0").await.unwrap();
    reply(&mut client, b'P').await;
    completed(&mut done, 1).await;
    sqlx::query("UPDATE metadata.tenants SET is_active=false WHERE tenant_id='tenant-a'")
        .execute(&pool)
        .await
        .unwrap();
    client.write_all(b"D1").await.unwrap();
    closed(&mut client).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ingestion.accepted_messages")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
}
#[tokio::test(start_paused = true)]
async fn slow_reader_hits_the_write_deadline_without_exposing_reply_bytes() {
    let (mut writer, _reader) = tokio::io::duplex(1);
    let before = tokio::time::Instant::now();
    let error = transport::write_with_deadline(&mut writer, &[1; 64], Duration::from_secs(2))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(before.elapsed(), Duration::from_secs(2));
}
#[tokio::test(start_paused = true)]
async fn silent_peer_hits_read_deadline_without_contacting_storage() {
    let pool = PgPoolOptions::new()
        .min_connections(0)
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    let session = fleetiq_ingestion::LiveSession::new(
        [1; 16],
        channel(),
        PostgresProtocolRoutingRepository::new(pool.clone()),
        PostgresReplayRepository::new(pool.clone()),
        executor::IsolatedLive::new(pool.clone()),
    );
    let (server, mut peer) = tokio::io::duplex(8);
    let before = tokio::time::Instant::now();
    transport::connection(server, session, transport::Deadlines::default()).await;
    assert_eq!(before.elapsed(), Duration::from_secs(15));
    assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
    assert_eq!(pool.size(), 0);
    pool.close().await;
}

#[cfg(target_os = "linux")]
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn global_limit_cannot_be_bypassed_with_multiple_peer_addresses(pool: PgPool) {
    let channel = seed(&pool).await;
    let Running {
        address,
        stop,
        task,
        polls: _polls,
        mut done,
    } = start(&pool, channel).await;
    let mut clients = Vec::new();
    for index in 0..transport::MAX_CONNECTIONS {
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        let octet = u8::try_from(index / 2 + 2).unwrap();
        socket
            .bind(std::net::SocketAddr::from(([127, 0, 0, octet], 0)))
            .unwrap();
        let mut client = socket.connect(address).await.unwrap();
        client.write_all(b"H0").await.unwrap();
        reply(&mut client, b'P').await;
        completed(&mut done, 1).await;
        clients.push(client);
    }
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket
        .bind(std::net::SocketAddr::from(([127, 0, 0, 10], 0)))
        .unwrap();
    let mut excess = socket.connect(address).await.unwrap();
    closed(&mut excess).await;
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    for mut client in clients {
        closed(&mut client).await;
    }
}

mod measured;
