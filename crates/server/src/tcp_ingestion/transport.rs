//! Bounded socket admission and cancellation. No decoding policy lives here.
use fleetiq_domain::UnixTimeMillis;
use fleetiq_ingestion::{
    LiveExecutor, LiveReplayRepository, LiveReplySink, LiveRoutingRepository, LiveSession,
    LiveStep, RegisteredIdentityChannel,
};
use std::{
    collections::HashMap,
    future::Future,
    io,
    net::IpAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
    task::JoinSet,
};
pub(super) const MAX_CONNECTIONS: usize = 8;
const MAX_PER_PEER: usize = 2;
#[derive(Clone, Copy)]
pub(super) struct Deadlines {
    pub read: Duration,
    pub write: Duration,
    pub turn: Duration,
    pub lifetime: Duration,
}
impl Default for Deadlines {
    fn default() -> Self {
        Self {
            read: Duration::from_secs(15),
            write: Duration::from_secs(2),
            turn: Duration::from_secs(25),
            lifetime: Duration::from_secs(300),
        }
    }
}
struct Writer<'a, T>(&'a mut T, Duration);
impl<T: AsyncWrite + Unpin + Send> LiveReplySink for Writer<'_, T> {
    async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        write_with_deadline(self.0, bytes, self.1).await
    }
}
pub(super) async fn write_with_deadline(
    socket: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
    deadline: Duration,
) -> io::Result<()> {
    tokio::time::timeout(deadline, socket.write_all(bytes))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}
fn now() -> io::Result<UnixTimeMillis> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis();
    Ok(UnixTimeMillis::new(
        i64::try_from(millis).map_err(io::Error::other)?,
    ))
}
pub(super) async fn connection<T, R, Q, E>(
    mut socket: T,
    mut session: LiveSession<R, Q, E>,
    limits: Deadlines,
) where
    T: AsyncRead + AsyncWrite + Unpin + Send,
    R: LiveRoutingRepository,
    Q: LiveReplayRepository,
    E: LiveExecutor,
{
    let work = async {
        let mut chunk = [0_u8; 4096];
        loop {
            let available = session.available().min(chunk.len());
            if available == 0 {
                return;
            }
            let read =
                tokio::time::timeout(limits.read, socket.read(&mut chunk[..available])).await;
            let Ok(Ok(read)) = read else {
                return;
            };
            if read == 0 || session.push(&chunk[..read]).is_err() {
                return;
            }
            let Ok(received) = now() else {
                return;
            };
            while session.has_input() {
                let result = tokio::time::timeout(
                    limits.turn,
                    session.step(received, &mut Writer(&mut socket, limits.write)),
                )
                .await;
                match result {
                    Ok(Ok(LiveStep::Committed)) => {}
                    Ok(Ok(LiveStep::NeedMore)) => break,
                    _ => return,
                }
            }
        }
    };
    let _ = tokio::time::timeout(limits.lifetime, work).await;
    let _ = socket.shutdown().await;
}
pub(super) async fn listen<R, Q, E>(
    listener: TcpListener,
    channel: RegisteredIdentityChannel,
    routing: R,
    repository: Q,
    executor: E,
    shutdown: impl Future<Output = ()> + Send,
) -> io::Result<()>
where
    R: LiveRoutingRepository + Clone + 'static,
    Q: LiveReplayRepository + Clone + 'static,
    E: LiveExecutor + Clone + 'static,
{
    let mut tasks = JoinSet::new();
    let mut peers = HashMap::<IpAddr, usize>::new();
    tokio::pin!(shutdown);
    let result = loop {
        tokio::select! {
            biased;
            () = &mut shutdown => break Ok(()),
            Some(result) = tasks.join_next() => {
                match result {
                    Ok(peer) => {
                        if let Some(count) = peers.get_mut(&peer) {
                            *count -= 1;
                            if *count == 0 { peers.remove(&peer); }
                        }
                    }
                    Err(error) => break Err(io::Error::other(error)),
                }
            }
            accepted = listener.accept() => {
                let (socket, address) = match accepted {
                    Ok(value) => value,
                    Err(error) => break Err(error),
                };
                let peer = address.ip();
                if tasks.len() >= MAX_CONNECTIONS
                    || peers.get(&peer).copied().unwrap_or(0) >= MAX_PER_PEER
                {
                    drop(socket);
                    // Bound reject-loop CPU without delaying a ready shutdown signal.
                    tokio::select! {
                        biased;
                        () = &mut shutdown => break Ok(()),
                        () = tokio::time::sleep(Duration::from_millis(10)) => {}
                    }
                    continue;
                }
                let mut nonce = [0; 16];
                if getrandom::fill(&mut nonce).is_err() {
                    break Err(io::Error::other("session entropy unavailable"));
                }
                *peers.entry(peer).or_default() += 1;
                let session = LiveSession::new(
                    nonce, channel.clone(), routing.clone(), repository.clone(), executor.clone(),
                );
                tasks.spawn(async move {
                    connection(socket, session, Deadlines::default()).await;
                    peer
                });
            }
        }
    };
    // Cancel socket owners first. Their child guards initiate kill/reap; the
    // composition root then drains the shared child slot before closing the pool.
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    result
}
