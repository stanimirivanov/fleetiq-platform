//! Bounded operational evidence, independent of transport, SQL and health inference.
use fleetiq_access::{AuthorizedTenant, ManageProtocols};
use fleetiq_domain::{AssetId, TenantId};
use std::future::Future;

/// Maximum accepted-message rows returned for one device.
pub const DIAGNOSTIC_FRAME_LIMIT: usize = 64;
/// Maximum decoded positions inspected per sampled message.
pub const DIAGNOSTIC_OUTPUT_LIMIT: usize = 64;
/// Maximum distinct package/endpoint/signal tuples returned.
pub const DIAGNOSTIC_SOURCE_LIMIT: usize = 128;

/// Durable scheduling evidence; none of these states proves process liveness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessingEvidence {
    /// Decoded marker exists, including empty output, without outstanding replay work.
    Decoded,
    /// Raw input exists without replay work or decoded completion evidence.
    Accepted,
    /// Pending work has never been claimed.
    Queued,
    /// An unexpired database lease exists; a claimant may nevertheless have died.
    Leased,
    /// Lease expired; recovery may retry or mark exhausted work poisoned.
    Expired,
    /// Scheduler recorded terminal rejection or exhaustion; intervention is required.
    Poisoned,
}
impl ProcessingEvidence {
    /// Classifies persisted facts using the database snapshot clock, never a device clock.
    #[must_use]
    pub fn classify(decoded: bool, status: Option<&str>, lease: Option<i64>, now: i64) -> Self {
        match status {
            Some("poisoned") => Self::Poisoned,
            Some("pending") => match lease {
                None => Self::Queued,
                Some(until) if until > now => Self::Leased,
                Some(_) => Self::Expired,
            },
            _ if decoded => Self::Decoded,
            _ => Self::Accepted,
        }
    }
}
/// Safe metadata for one accepted message. Opaque bytes and values are deliberately absent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticFrame {
    /// Stable accepted-message identity within the authorized tenant.
    pub message_id: String,
    /// Exact decoder identity, not a current/floating selection.
    pub package_id: String,
    /// Immutable decoder version.
    pub package_version: String,
    /// Trusted receipt timestamp; historical/replayed data does not imply a live socket.
    pub received_at_ms: i64,
    /// Durable processing evidence evaluated at the snapshot clock.
    pub processing: ProcessingEvidence,
    /// Number of decoded outputs; None means no completion marker.
    pub observation_count: Option<i64>,
}
/// Observed decoder-owned source identity, not physical equipment inventory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedSource {
    /// Decoder package identity.
    pub package_id: String,
    /// Exact decoder version.
    pub package_version: String,
    /// Source endpoint reported by the decoder.
    pub endpoint_id: String,
    /// Source signal reported by the decoder.
    pub signal_id: String,
}
/// Consistent bounded snapshot; truncation flags describe coverage, not health.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestionDiagnostics {
    /// Database transaction timestamp in Unix milliseconds.
    pub observed_at_ms: i64,
    /// Newest by receipt timestamp then message identity, at most 64 entries.
    pub frames: Vec<DiagnosticFrame>,
    /// More accepted messages exist outside this recent sample.
    pub frames_truncated: bool,
    /// First 64 output positions of each sampled completed frame were inspected.
    pub observations_truncated: bool,
    /// Lexicographically ordered distinct source tuples from that bounded prefix.
    pub sources: Vec<ObservedSource>,
    /// More distinct source tuples exist within the inspected output prefix.
    pub sources_truncated: bool,
}
/// Consumer-owned safe read port. Implementations must bound reads before aggregation.
pub trait IngestionDiagnosticsRepository: Send + Sync {
    /// Adapter failure; callers must redact underlying database diagnostics.
    type Error: std::error::Error + Send + Sync + 'static;
    /// Returns None for an absent communication device, Some(empty) for no activity.
    fn snapshot<'a>(
        &'a self,
        tenant: &'a TenantId,
        device: &'a AssetId,
    ) -> impl Future<Output = Result<Option<IngestionDiagnostics>, Self::Error>> + Send + 'a;
}
/// Authorized operator use case; authorization evidence supplies the tenant.
pub struct IngestionDiagnosticsService<R> {
    repository: R,
}
impl<R: IngestionDiagnosticsRepository> IngestionDiagnosticsService<R> {
    /// Injects the capability's persistence port.
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
    /// Reads safe evidence under current protocol-management authority.
    /// # Errors
    /// Returns persistence failures; absence is a separate outcome.
    pub async fn find(
        &self,
        authorization: AuthorizedTenant<ManageProtocols>,
        device: &AssetId,
    ) -> Result<Option<IngestionDiagnostics>, R::Error> {
        self.repository
            .snapshot(authorization.tenant_id(), device)
            .await
    }
}
#[cfg(test)]
mod tests {
    use super::ProcessingEvidence as E;
    #[test]
    fn silence_is_not_failure_and_expiry_is_not_poison_or_liveness() {
        assert_eq!(E::classify(false, None, None, 100), E::Accepted);
        // A legacy decode marker is independent of pending replay/checkpoint
        // work and must never hide a poisoned or still-queued session.
        assert_eq!(
            E::classify(true, Some("poisoned"), Some(0), 100),
            E::Poisoned
        );
        assert_eq!(E::classify(true, Some("pending"), None, 100), E::Queued);
        assert_eq!(E::classify(false, Some("pending"), None, 100), E::Queued);
        assert_eq!(
            E::classify(false, Some("pending"), Some(101), 100),
            E::Leased
        );
        assert_eq!(
            E::classify(false, Some("pending"), Some(100), 100),
            E::Expired
        );
        assert_eq!(
            E::classify(false, Some("poisoned"), Some(0), 100),
            E::Poisoned
        );
        assert_eq!(
            E::classify(true, Some("completed"), Some(0), 100),
            E::Decoded
        );
    }
}
