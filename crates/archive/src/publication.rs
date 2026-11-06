//! History-owned publication port implemented by the verifying object adapter.
use crate::objects::ImmutableArchiveObjects;
use fleetiq_domain::TenantId;
use fleetiq_history::{
    StoredPropertyChange,
    archive_work::{ArchiveFailure, ArchiveObject, ArchiveSegmentStore},
};
impl ArchiveSegmentStore for ImmutableArchiveObjects {
    async fn store(
        &self,
        tenant: &TenantId,
        changes: &[StoredPropertyChange],
    ) -> Result<ArchiveObject, ArchiveFailure> {
        let bytes = crate::encode(tenant, changes).map_err(|_| ArchiveFailure)?;
        let reference = self.put(tenant, &bytes).await.map_err(|_| ArchiveFailure)?;
        ArchiveObject::new(
            reference.sha256(),
            i64::try_from(reference.byte_length()).map_err(|_| ArchiveFailure)?,
        )
    }
}
