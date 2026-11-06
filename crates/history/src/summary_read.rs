//! Authorized reads of immutable derived snapshots; no production or raw fallback.
use crate::downsampling::{NumericBucket, NumericRequest, SummaryFailure, SummaryReceipt};
use fleetiq_access::{AuthorizedTenant, ReadTelemetry};
use fleetiq_domain::{PropertyValueKind, UnitId};
use std::future::Future;
/// One bounded persisted numeric snapshot; mean is the exact bucket sum/count pair.
pub struct NumericSummary {
    /// Original publication identity.
    pub receipt: SummaryReceipt,
    /// Captured observation time, never a completeness watermark.
    pub observed_at_ms: i64,
    /// Exact numeric definition kind.
    pub kind: PropertyValueKind,
    /// Original canonical unit, without conversion.
    pub unit: Option<UnitId>,
    /// All complete buckets, bounded by the validated numeric specification to sixty-four.
    pub buckets: Vec<NumericBucket>,
}
/// Consumer-owned persistence port; implementations preserve exact revision and scope.
pub trait NumericSummaryView {
    /// Reads the current head or one explicitly pinned immutable publication.
    /// # Errors
    /// Storage/reconstruction failures must never become empty success.
    fn read_summary(
        &self,
        request: &NumericRequest,
        pinned: Option<SummaryReceipt>,
    ) -> impl Future<Output = Result<Option<NumericSummary>, SummaryFailure>> + Send;
}
/// Enforces telemetry authority before invoking the summary read port.
pub struct NumericSummaryService<V>(V);
impl<V: NumericSummaryView> NumericSummaryService<V> {
    /// Composes an existing persistence adapter; starts no production.
    pub const fn new(view: V) -> Self {
        Self(view)
    }
    /// Reads only the exact authorized tenant and requested immutable scope.
    /// # Errors
    /// Rejects mismatched authority or preserves storage/reconstruction failure.
    pub async fn read(
        &self,
        grant: AuthorizedTenant<ReadTelemetry>,
        request: &NumericRequest,
        pinned: Option<SummaryReceipt>,
    ) -> Result<Option<NumericSummary>, SummaryFailure> {
        if grant.tenant_id() != request.tenant() {
            return Err(SummaryFailure::InvalidEvidence);
        }
        self.0.read_summary(request, pinned).await
    }
}
