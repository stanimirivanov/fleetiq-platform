//! Actual SSE framing, controlled time and observable read/capacity ownership.
use super::frame::{MAX_JSON_BYTES, frame};
use super::*;
use axum::{
    body::to_bytes,
    response::{IntoResponse, Sse},
};
use fleetiq_projection::CurrentPropertySnapshot;
use futures_util::StreamExt;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Semaphore;
#[derive(Clone)]
struct Source {
    value: Arc<Mutex<Result<CurrentPropertySnapshot, ReadFailure>>>,
    reads: Arc<AtomicUsize>,
}
impl ReadSource for Source {
    fn read(
        &self,
    ) -> impl std::future::Future<Output = Result<CurrentPropertySnapshot, ReadFailure>> + Send
    {
        self.reads.fetch_add(1, Ordering::SeqCst);
        std::future::ready(self.value.lock().unwrap().clone())
    }
}
fn source() -> Source {
    Source {
        value: Arc::new(Mutex::new(Ok(snapshot(1, 1)))),
        reads: Arc::new(AtomicUsize::new(0)),
    }
}
fn snapshot(generation: u64, revision: u64) -> CurrentPropertySnapshot {
    CurrentPropertySnapshot::new(generation, revision, 0, false, None)
}
async fn text(event: Event) -> String {
    let response =
        Sse::new(futures_util::stream::iter([Ok::<_, Infallible>(event)])).into_response();
    String::from_utf8(
        to_bytes(response.into_body(), 65536)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
#[tokio::test(start_paused = true)]
async fn committed_replacements_coalesce_from_the_last_sent_revision() {
    let source = source();
    let capacity = Arc::new(Semaphore::new(1));
    let mut events = Box::pin(stream(
        source.clone(),
        "a".repeat(64),
        None,
        capacity.clone().acquire_owned().await.unwrap(),
    ));
    let first = text(events.next().await.unwrap().unwrap()).await;
    assert!(first.contains("event: snapshot"));
    assert!(first.contains("id: v1."));
    *source.value.lock().unwrap() = Ok(snapshot(1, 4));
    let changed = text(events.next().await.unwrap().unwrap()).await;
    assert!(changed.contains("event: replace"));
    assert!(changed.contains("\"base_revision\":\"1\""));
    assert!(changed.contains("\"revision\":\"4\""));
    drop(events);
    assert_eq!(capacity.available_permits(), 1);
}
#[tokio::test(start_paused = true)]
async fn recovery_and_authority_controls_never_advance_a_cursor() {
    for (next_value, expected) in [
        (Ok(snapshot(2, 2)), "projection_reset"),
        (Ok(snapshot(1, 0)), "revision_mismatch"),
        (Ok(snapshot(1, 258)), "resume_window_exceeded"),
        (
            Err(ReadFailure::AuthenticationRequired),
            "authentication_required",
        ),
        (Err(ReadFailure::AccessDenied), "access_denied"),
        (Err(ReadFailure::ScopeMissing), "scope_missing"),
        (Err(ReadFailure::Unavailable), "unavailable"),
    ] {
        let source = source();
        *source.value.lock().unwrap() = next_value;
        let capacity = Arc::new(Semaphore::new(1));
        let mut events = Box::pin(stream(
            source,
            "a".repeat(64),
            Some(DeliveryPosition {
                generation: 1,
                revision: 1,
            }),
            capacity.clone().acquire_owned().await.unwrap(),
        ));
        let control = text(events.next().await.unwrap().unwrap()).await;
        assert!(control.contains(expected));
        assert!(!control.contains("id:"));
        assert!(!control.contains("\"revision\""));
        assert!(events.next().await.is_none());
        assert_eq!(capacity.available_permits(), 1);
    }
}
#[tokio::test(start_paused = true)]
async fn no_detached_reads_or_unbounded_queue_exist_for_a_slow_consumer() {
    let source = source();
    let capacity = Arc::new(Semaphore::new(1));
    let mut events = Box::pin(stream(
        source.clone(),
        "a".repeat(64),
        None,
        capacity.clone().acquire_owned().await.unwrap(),
    ));
    let _initial_event = events.next().await.unwrap().unwrap();
    tokio::time::advance(Duration::from_secs(31)).await;
    assert_eq!(source.reads.load(Ordering::SeqCst), 1);
    let control = text(events.next().await.unwrap().unwrap()).await;
    assert!(control.contains("slow_consumer"));
    assert_eq!(capacity.available_permits(), 1);
}
#[tokio::test(start_paused = true)]
async fn heartbeats_keep_the_cursor_unchanged_and_active_leases_end() {
    let source = source();
    let capacity = Arc::new(Semaphore::new(1));
    let mut events = Box::pin(stream(
        source,
        "a".repeat(64),
        Some(DeliveryPosition {
            generation: 1,
            revision: 1,
        }),
        capacity.acquire_owned().await.unwrap(),
    ));
    let first = text(events.next().await.unwrap().unwrap()).await;
    assert!(first.contains("heartbeat"));
    assert!(!first.contains("id:"));
    let mut ended = false;
    for _ in 0..5 {
        let event = text(events.next().await.unwrap().unwrap()).await;
        if event.contains("lease_expired") {
            ended = true;
            break;
        }
        assert!(event.contains("heartbeat"));
    }
    assert!(ended);
    assert!(events.next().await.is_none());
}
struct Blocking(Arc<AtomicUsize>);
struct ActiveRead(Arc<AtomicUsize>);
impl Drop for ActiveRead {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl ReadSource for Blocking {
    async fn read(&self) -> Result<CurrentPropertySnapshot, ReadFailure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let _guard = ActiveRead(self.0.clone());
        std::future::pending().await
    }
}
#[tokio::test(start_paused = true)]
async fn read_deadline_cancels_work_and_releases_the_capacity_guard() {
    let active = Arc::new(AtomicUsize::new(0));
    let capacity = Arc::new(Semaphore::new(1));
    let mut events = Box::pin(stream(
        Blocking(active.clone()),
        "a".repeat(64),
        None,
        capacity.clone().acquire_owned().await.unwrap(),
    ));
    let event = text(events.next().await.unwrap().unwrap()).await;
    assert!(event.contains("unavailable"));
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert_eq!(capacity.available_permits(), 1);
}
#[tokio::test(start_paused = true)]
async fn dropping_a_pending_body_cancels_the_owned_read() {
    let active = Arc::new(AtomicUsize::new(0));
    let capacity = Arc::new(Semaphore::new(1));
    let mut events = Box::pin(stream(
        Blocking(active.clone()),
        "a".repeat(64),
        None,
        capacity.clone().acquire_owned().await.unwrap(),
    ));
    tokio::select! {
        _ = events.next() => panic!("blocked read unexpectedly returned"),
        () = tokio::time::sleep(Duration::from_secs(1)) => {}
    }
    assert_eq!(active.load(Ordering::SeqCst), 1);
    drop(events);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert_eq!(capacity.available_permits(), 1);
}
#[test]
fn oversized_json_cannot_enter_a_data_frame() {
    assert!(frame("snapshot", &"x".repeat(MAX_JSON_BYTES), None).is_none());
    assert!(frame("snapshot", &"safe", None).is_some());
}
