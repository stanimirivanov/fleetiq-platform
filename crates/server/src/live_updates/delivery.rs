//! Demand-driven delivery: no producer task, history queue or socket-owned SQL transaction.
use super::{
    IO_TIMEOUT,
    source::{ReadFailure, ReadSource},
};
use axum::response::sse::Event;
use fleetiq_projection::{DeliveryDecision, DeliveryPosition, delivery_decision};
use futures_util::{Stream, stream::unfold};
use std::{convert::Infallible, time::Duration};
use tokio::{sync::OwnedSemaphorePermit, time::Instant};

const READ_INTERVAL: Duration = Duration::from_secs(2);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const DEMAND_TIMEOUT: Duration = Duration::from_secs(30);
const LEASE: Duration = Duration::from_secs(60);
mod frame;
struct State<R> {
    source: R,
    scope: String,
    previous: Option<DeliveryPosition>,
    permit: Option<OwnedSemaphorePermit>,
    started: Instant,
    last_emit: Instant,
    next_read: Instant,
    first: bool,
    done: bool,
}
impl<R> State<R> {
    fn lease_expired(&self) -> bool {
        Instant::now().duration_since(self.started) >= LEASE
    }
    fn terminal(&mut self, name: &'static str, reason: &'static str) -> Event {
        self.done = true;
        self.permit.take();
        // Fixed tokens only; controls never disclose an unauthorized revision.
        Event::default()
            .event(name)
            .data(format!("{{\"reason\":\"{reason}\"}}"))
    }
}
pub(super) fn stream<R: ReadSource>(
    source: R,
    scope: String,
    previous: Option<DeliveryPosition>,
    permit: OwnedSemaphorePermit,
) -> impl Stream<Item = Result<Event, Infallible>> + Send {
    let now = Instant::now();
    unfold(
        State {
            source,
            scope,
            previous,
            permit: Some(permit),
            started: now,
            last_emit: now,
            next_read: now,
            first: true,
            done: false,
        },
        next,
    )
}
async fn next<R: ReadSource>(mut state: State<R>) -> Option<(Result<Event, Infallible>, State<R>)> {
    if state.done {
        return None;
    }
    // Only compare demand at entry. An actively awaited read may legitimately
    // span many idle poll intervals; that is not a slow consumer.
    if !state.first && Instant::now().duration_since(state.last_emit) > DEMAND_TIMEOUT {
        let event = state.terminal("resnapshot-required", "slow_consumer");
        return Some((Ok(event), state));
    }
    loop {
        if state.lease_expired() {
            let event = state.terminal("stream-ended", "lease_expired");
            return Some((Ok(event), state));
        }
        tokio::time::sleep_until(state.next_read).await;
        if state.lease_expired() {
            let event = state.terminal("stream-ended", "lease_expired");
            return Some((Ok(event), state));
        }
        let snapshot = match tokio::time::timeout(IO_TIMEOUT, state.source.read()).await {
            Ok(Ok(snapshot)) => snapshot,
            result => {
                let reason = match result {
                    Ok(Err(ReadFailure::AuthenticationRequired)) => "authentication_required",
                    Ok(Err(ReadFailure::AccessDenied)) => "access_denied",
                    Ok(Err(ReadFailure::ScopeMissing)) => "scope_missing",
                    _ => "unavailable",
                };
                let event = state.terminal("stream-ended", reason);
                return Some((Ok(event), state));
            }
        };
        // A read can finish after the lease; never stamp another data frame.
        if state.lease_expired() {
            let event = state.terminal("stream-ended", "lease_expired");
            return Some((Ok(event), state));
        }
        let current = DeliveryPosition {
            generation: snapshot.generation(),
            revision: snapshot.state().revision().get(),
        };
        let decision = delivery_decision(state.previous, current);
        let control = match decision {
            DeliveryDecision::ProjectionReset => Some("projection_reset"),
            DeliveryDecision::RevisionMismatch => Some("revision_mismatch"),
            DeliveryDecision::ResumeWindowExceeded => Some("resume_window_exceeded"),
            _ => None,
        };
        if let Some(reason) = control {
            let event = state.terminal("resnapshot-required", reason);
            return Some((Ok(event), state));
        }
        let now = Instant::now();
        let first = state.first;
        state.first = false;
        state.next_read = now + READ_INTERVAL;
        if decision == DeliveryDecision::Unchanged
            && !first
            && now.duration_since(state.last_emit) < HEARTBEAT_INTERVAL
        {
            continue;
        }
        let event = match frame::state_event(&snapshot, state.previous, &state.scope) {
            Ok(event) => event,
            Err(reason) => {
                let event = state.terminal("stream-ended", reason);
                return Some((Ok(event), state));
            }
        };
        if state.lease_expired() {
            let event = state.terminal("stream-ended", "lease_expired");
            return Some((Ok(event), state));
        }
        state.previous = Some(current);
        state.last_emit = Instant::now();
        return Some((Ok(event), state));
    }
}
#[cfg(test)]
mod tests;
