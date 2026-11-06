//! Existing exact codec exposed through the numeric producer's consumer-owned boundary.
use fleetiq_domain::TenantId;
use fleetiq_history::{
    StoredPropertyChange,
    downsampling::{SummaryArchiveDecoder, SummaryFailure},
};
/// Version-one exact bounded archive decoder; no object-store scan or replay.
pub struct NumericArchiveDecoder;
impl SummaryArchiveDecoder for NumericArchiveDecoder {
    fn decode(
        &self,
        tenant: &TenantId,
        bytes: &[u8],
    ) -> Result<Vec<StoredPropertyChange>, SummaryFailure> {
        crate::decode(tenant, bytes).map_err(|_| SummaryFailure::CoverageUnavailable)
    }
}
