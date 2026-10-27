//! Current and history reads share exact value and immutable provenance encoding.
use crate::property_history::dto::ChangeBody;
use fleetiq_domain::UnixTimeMillis;
use fleetiq_projection::{CurrentPropertySnapshot, Freshness, freshness};
use serde::Serialize;
#[derive(Serialize)]
pub(super) struct Body {
    generation: String,
    revision: String,
    processed_count: String,
    pending: bool,
    evaluated_at_ms: String,
    stale_after_ms: u32,
    items: Vec<Item>,
}
#[derive(Serialize)]
struct Item {
    freshness: &'static str,
    change: ChangeBody,
}
impl Body {
    pub(super) fn new(
        snapshot: &CurrentPropertySnapshot,
        at: UnixTimeMillis,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            generation: snapshot.generation().to_string(),
            revision: snapshot.state().revision().get().to_string(),
            processed_count: snapshot.processed_count().to_string(),
            pending: snapshot.pending(),
            evaluated_at_ms: at.get().to_string(),
            stale_after_ms: 30_000,
            items: snapshot
                .state()
                .items()
                .iter()
                .map(|change| {
                    Ok(Item {
                        freshness: match freshness(change.event_at(), at) {
                            Freshness::Fresh => "fresh",
                            Freshness::Stale => "stale",
                            Freshness::Future => "future",
                        },
                        change: ChangeBody::new(change)?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}
