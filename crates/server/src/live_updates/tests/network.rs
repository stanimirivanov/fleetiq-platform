//! Exercise HTTP/1 SSE bytes over a real listener, not only an in-memory stream.
use super::*;
use fleetiq_projection::ProjectionMaintenance;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn connect(address: std::net::SocketAddr, after: Option<&str>) -> BufReader<TcpStream> {
    let mut socket = TcpStream::connect(address).await.unwrap();
    let resume = after.map_or_else(String::new, |cursor| format!("Last-Event-ID: {cursor}\r\n"));
    socket.write_all(format!("GET {ROOT} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {TOKEN}\r\n{resume}\r\n").as_bytes()).await.unwrap();
    let mut socket = BufReader::new(socket);
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
        headers.push_str(&line);
        assert!(headers.len() < 8192);
        if line == "\r\n" {
            break;
        }
    }
    assert!(headers.starts_with("HTTP/1.1 200"));
    assert!(headers.contains("text/event-stream"));
    assert!(headers.contains("cache-control: no-store"));
    assert!(headers.contains("x-accel-buffering: no"));
    socket
}
async fn event(socket: &mut BufReader<TcpStream>) -> (String, Option<String>, serde_json::Value) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut name = String::new();
        let mut id = None;
        let mut data = None;
        let mut bytes = 0;
        loop {
            let mut line = String::new();
            assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
            bytes += line.len();
            assert!(bytes < 65536);
            if let Some(value) = line.strip_prefix("event: ") {
                name = value.trim().to_owned();
            }
            if let Some(value) = line.strip_prefix("id: ") {
                id = Some(value.trim().to_owned());
            }
            if let Some(value) = line.strip_prefix("data: ") {
                data = Some(serde_json::from_str(value.trim()).unwrap());
            }
            if line.trim().is_empty()
                && let Some(data) = data.take()
            {
                return (name, id, data);
            }
        }
    })
    .await
    .unwrap()
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn tcp_reconnect_replaces_committed_state_and_revocation_stops_delivery(pool: PgPool) {
    let app = setup(&pool).await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let _server = Server(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    let mut first = connect(address, None).await;
    let (name, cursor, snapshot) = event(&mut first).await;
    assert_eq!(name, "snapshot");
    assert_eq!(snapshot["revision"], "0");
    drop(first);
    PostgresCurrentStateRepository::new(pool.clone())
        .tick()
        .await
        .unwrap();
    let mut resumed = connect(address, cursor.as_deref()).await;
    let (name, _, update) = event(&mut resumed).await;
    assert_eq!(name, "replace");
    assert_eq!(update["base_revision"], "0");
    assert_eq!(update["snapshot"]["revision"], "1");
    assert_eq!(
        update["snapshot"]["items"][0]["change"]["value"]["value"],
        "9223372036854775807"
    );
    sqlx::query("DELETE FROM identity.tenant_action_grants WHERE tenant_id = 'tenant-a' AND actor_id = 'operator-a' AND action = 'telemetry.read'").execute(&pool).await.unwrap();
    // A previously authorized heartbeat may already be in the socket buffer.
    // It cannot be recalled; authority must still terminate the demanded stream.
    let mut ended = false;
    for _ in 0..3 {
        let (name, cursor, control) = event(&mut resumed).await;
        assert_eq!(cursor, None);
        if name == "stream-ended" {
            assert_eq!(control["reason"], "access_denied");
            ended = true;
            break;
        }
        assert_eq!(name, "heartbeat");
    }
    assert!(ended);
}
