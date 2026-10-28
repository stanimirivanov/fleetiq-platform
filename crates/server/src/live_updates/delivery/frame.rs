//! Bounded SSE encoding is separate from demand, authority and lifetime control.
use crate::{current_state::dto::Body, live_updates::cursor, protected_request};
use axum::response::sse::Event;
use fleetiq_projection::{CurrentPropertySnapshot, DeliveryPosition};
use serde::Serialize;
pub(super) const MAX_JSON_BYTES: usize = 60 * 1024;
pub(super) fn state_event(
    snapshot: &CurrentPropertySnapshot,
    previous: Option<DeliveryPosition>,
    scope: &str,
) -> Result<Event, &'static str> {
    let at = protected_request::now().map_err(|_| "unavailable")?;
    let current = DeliveryPosition {
        generation: snapshot.generation(),
        revision: snapshot.state().revision().get(),
    };
    let event = if previous == Some(current) {
        frame(
            "heartbeat",
            &Heartbeat {
                generation: current.generation.to_string(),
                revision: current.revision.to_string(),
                evaluated_at_ms: at.get().to_string(),
                pending: snapshot.pending(),
            },
            None,
        )
    } else {
        let body = Body::new(snapshot, at).map_err(|_| "unavailable")?;
        let id = cursor::encode(scope, current);
        if let Some(previous) = previous {
            frame(
                "replace",
                &Replacement {
                    base_revision: previous.revision.to_string(),
                    snapshot: body,
                },
                Some(&id),
            )
        } else {
            frame("snapshot", &body, Some(&id))
        }
    };
    event.ok_or("payload_limit")
}
#[derive(Serialize)]
struct Replacement {
    base_revision: String,
    snapshot: Body,
}
#[derive(Serialize)]
struct Heartbeat {
    generation: String,
    revision: String,
    evaluated_at_ms: String,
    pending: bool,
}
pub(super) fn frame<T: Serialize>(name: &str, value: &T, id: Option<&str>) -> Option<Event> {
    let mut buffer = JsonBuffer(Vec::with_capacity(1024));
    serde_json::to_writer(&mut buffer, value).ok()?;
    let json = String::from_utf8(buffer.0).ok()?;
    let event = Event::default().event(name).data(json);
    Some(if let Some(id) = id {
        event.id(id)
    } else {
        event
    })
}
// Stop serialization at the byte boundary; do not allocate an oversized JSON
// String and only discover its size afterwards.
struct JsonBuffer(Vec<u8>);
impl std::io::Write for JsonBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_JSON_BYTES - self.0.len() {
            return Err(std::io::Error::other("live JSON limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
