//! Existing exact Parquet profile reused for frozen export selections and artifacts.
use fleetiq_domain::TenantId;
use fleetiq_history::{
    StoredPropertyChange,
    exports::{ExportCodec, ExportFailure},
};
/// Trusted exact version-one codec; introduces no format or I/O protocol.
pub struct ExactExportCodec;
impl ExportCodec for ExactExportCodec {
    fn encode(
        &self,
        tenant: &TenantId,
        rows: &[StoredPropertyChange],
    ) -> Result<Vec<u8>, ExportFailure> {
        crate::encode(tenant, rows).map_err(|_| ExportFailure::Limit)
    }
    fn decode(
        &self,
        tenant: &TenantId,
        bytes: &[u8],
    ) -> Result<Vec<StoredPropertyChange>, ExportFailure> {
        crate::decode(tenant, bytes).map_err(|_| ExportFailure::Coverage)
    }
}
