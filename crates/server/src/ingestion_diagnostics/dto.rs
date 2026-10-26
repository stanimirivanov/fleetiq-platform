//! Closed response vocabulary. Safe DTOs cannot carry raw payload/state/value bytes.
use fleetiq_ingestion::{IngestionDiagnostics, ProcessingEvidence};
use serde::Serialize;
#[derive(Serialize)]
pub(super) struct SnapshotBody {
    observed_at_ms: i64,
    connection_liveness: &'static str,
    worker_liveness: &'static str,
    coverage: Coverage,
    frames: Vec<Frame>,
    sources: Vec<Source>,
}
#[derive(Serialize)]
struct Coverage {
    frame_limit: usize,
    output_limit_per_frame: usize,
    source_limit: usize,
    frames_truncated: bool,
    observations_truncated: bool,
    sources_truncated: bool,
}
#[derive(Serialize)]
struct Frame {
    message_id: String,
    package_id: String,
    package_version: String,
    received_at_ms: i64,
    processing: &'static str,
    observation_count: Option<i64>,
}
#[derive(Serialize)]
struct Source {
    package_id: String,
    package_version: String,
    endpoint_id: String,
    signal_id: String,
}
impl From<IngestionDiagnostics> for SnapshotBody {
    fn from(s: IngestionDiagnostics) -> Self {
        Self {
            observed_at_ms: s.observed_at_ms,
            connection_liveness: "not_observed",
            worker_liveness: "not_observed",
            coverage: Coverage {
                frame_limit: fleetiq_ingestion::DIAGNOSTIC_FRAME_LIMIT,
                output_limit_per_frame: fleetiq_ingestion::DIAGNOSTIC_OUTPUT_LIMIT,
                source_limit: fleetiq_ingestion::DIAGNOSTIC_SOURCE_LIMIT,
                frames_truncated: s.frames_truncated,
                observations_truncated: s.observations_truncated,
                sources_truncated: s.sources_truncated,
            },
            frames: s
                .frames
                .into_iter()
                .map(|r| Frame {
                    message_id: r.message_id,
                    package_id: r.package_id,
                    package_version: r.package_version,
                    received_at_ms: r.received_at_ms,
                    observation_count: r.observation_count,
                    processing: match r.processing {
                        ProcessingEvidence::Decoded => "decoded",
                        ProcessingEvidence::Accepted => "accepted",
                        ProcessingEvidence::Queued => "queued",
                        ProcessingEvidence::Leased => "leased",
                        ProcessingEvidence::Expired => "expired",
                        ProcessingEvidence::Poisoned => "poisoned",
                    },
                })
                .collect(),
            sources: s
                .sources
                .into_iter()
                .map(|r| Source {
                    package_id: r.package_id,
                    package_version: r.package_version,
                    endpoint_id: r.endpoint_id,
                    signal_id: r.signal_id,
                })
                .collect(),
        }
    }
}
