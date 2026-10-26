//! Repeatable transport/storage measurements, not a protocol-execution benchmark.
use super::*;
use fleetiq_ingestion::IngestionDiagnosticsRepository;
use fleetiq_postgres::PostgresIngestionDiagnostics;
use std::time::Instant;

#[cfg(target_os = "linux")]
fn memory_kib(field: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix(field))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}
#[cfg(not(target_os = "linux"))]
fn memory_kib(_: &str) -> Option<u64> {
    None
}

async fn client(mut stream: TcpStream) -> Vec<u64> {
    let mut latencies = Vec::with_capacity(255);
    let sampling_started = Instant::now();
    for position in 0..255 {
        // Observe achievable throughput without imposing a fixed-count SLA on
        // every host. Finish the in-flight turn; production deadlines still apply.
        if position > 0 && sampling_started.elapsed() >= Duration::from_secs(10) {
            break;
        }
        let began = Instant::now();
        stream.write_all(b"D7").await.unwrap();
        let byte = tokio::time::timeout(
            transport::Deadlines::default().turn + transport::Deadlines::default().write,
            stream.read_u8(),
        )
        .await
        .expect("telemetry ACK fixture deadline")
        .expect("telemetry connection stays open");
        assert_eq!(byte, b'A');
        latencies.push(u64::try_from(began.elapsed().as_micros()).unwrap());
    }
    latencies
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn measured_bounded_tcp_envelope(pool: PgPool) {
    let channel = seed(&pool).await;
    let Running {
        address,
        stop,
        task,
        mut polls,
        mut done,
    } = start(&pool, channel.clone()).await;
    let draining = tokio::spawn(async move { while polls.recv().await.is_some() {} });
    let (completed, mut completion) = tokio::sync::watch::channel(0usize);
    let collecting = tokio::spawn(async move {
        while done.recv().await.is_some() {
            completed.send_modify(|count| *count += 1);
        }
    });
    let rss_before = memory_kib("VmRSS:");
    let began = Instant::now();
    let mut first = TcpStream::connect(address).await.unwrap();
    first.write_all(b"H0").await.unwrap();
    assert_eq!(
        tokio::time::timeout(
            transport::Deadlines::default().turn + transport::Deadlines::default().write,
            first.read_u8()
        )
        .await
        .expect("first identity fixture deadline")
        .expect("first identity connection"),
        b'P'
    );
    let mut second = TcpStream::connect(address).await.unwrap();
    second.write_all(b"H0").await.unwrap();
    assert_eq!(
        tokio::time::timeout(
            transport::Deadlines::default().turn + transport::Deadlines::default().write,
            second.read_u8()
        )
        .await
        .expect("second identity fixture deadline")
        .expect("second identity connection"),
        b'P'
    );
    // Both admitted sessions are held open: another peer connection must be
    // rejected rather than queued for future work or persisted as a frame.
    let mut excess = TcpStream::connect(address).await.unwrap();
    closed(&mut excess).await;
    let (mut a, b) = tokio::time::timeout(transport::Deadlines::default().lifetime, async {
        let (first, second) = tokio::join!(client(first), client(second));
        let expected = 2 + first.len() + second.len();
        completion
            .wait_for(|count| *count >= expected)
            .await
            .unwrap();
        assert_eq!(*completion.borrow(), expected);
        (first, second)
    })
    .await
    .expect("bounded synthetic workload must complete within the production connection lifetime");
    assert!(!a.is_empty() && !b.is_empty());
    a.extend(b);
    let frames = 2 + a.len();
    let elapsed = began.elapsed();
    let snapshot = PostgresIngestionDiagnostics::new(pool.clone())
        .snapshot(
            &channel.tenant_id,
            &fleetiq_domain::AssetId::new("device-a").unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.frames.len(), frames.min(64));
    assert_eq!(snapshot.frames_truncated, frames > 64);
    assert!(
        snapshot
            .frames
            .iter()
            .all(|f| f.processing == fleetiq_ingestion::ProcessingEvidence::Decoded)
    );
    let counts=sqlx::query_as::<_,(i64,i64,i64)>("SELECT (SELECT count(*) FROM ingestion.accepted_messages),(SELECT count(*) FROM ingestion.decoded_message_results),(SELECT count(*) FROM ingestion.replay_work WHERE status='pending')")
        .fetch_one(&pool).await.unwrap();
    let expected = i64::try_from(frames).unwrap();
    assert_eq!(counts, (expected, expected, 0));
    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    draining.await.unwrap();
    collecting.await.unwrap();
    a.sort_unstable();
    let report = serde_json::json!({
        "schema":"fleetiq.ingestion-measurement.v1",
        "boundary":"real TCP + durable PostgreSQL; deterministic decoder; excludes Wasm, child process and vendor wire costs",
        "frames":frames,"telemetry_frames":a.len(),"payload_bytes":frames*2,"connections":2,
        "workload":{"sampling_window_ms":10000,"maximum_frames":512},
        "rejected_excess_connections":1,"pending_after":counts.2,
        "elapsed_ms":elapsed.as_millis(),"completed_frames_per_second":f64::from(u32::try_from(frames).unwrap())/elapsed.as_secs_f64(),
        "ack_latency_us":{"p50":a[a.len()/2],"p95":a[a.len()*95/100],"p99":a[a.len()*99/100]},
        "resource_observations":{"process_rss_before_kib":rss_before,"process_rss_after_kib":memory_kib("VmRSS:"),"process_high_water_kib":memory_kib("VmHWM:"),"database_pool_size_after":pool.size()},
        "policy":{"global_connections":transport::MAX_CONNECTIONS,"per_peer_connections":2,"frames_per_session":256,"unread_bytes":65536},
        "limits":"functional deadlines only; timings and memory are observations, not calibrated hardware SLAs"
    });
    // One fixture per test process. Only synthetic aggregate measurements leave
    // the test; no payloads, connection addresses or database URLs are recorded.
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/ingestion-measurements");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!("tcp-{}.json", std::process::id())),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("INGESTION_MEASUREMENT {report}");
}
