//! Bounded original evidence retrieval; never ingestion replay or hot-source reconstruction.
use crate::objects::{ArchiveObjectRef, ImmutableArchiveObjects};
use fleetiq_domain::TenantId;
use fleetiq_history::{
    archive_work::ArchiveObject,
    restore::{ArchiveRestoreSource, RestoreFailure, RestoredSegment},
};
impl ArchiveRestoreSource for ImmutableArchiveObjects {
    async fn load(
        &self,
        tenant: &TenantId,
        object: ArchiveObject,
    ) -> Result<RestoredSegment, RestoreFailure> {
        let reference = ArchiveObjectRef::new(
            tenant.clone(),
            object.digest(),
            u64::try_from(object.bytes()).map_err(|_| RestoreFailure)?,
        )
        .map_err(|_| RestoreFailure)?;
        let bytes = self
            .get(tenant, &reference)
            .await
            .map_err(|_| RestoreFailure)?;
        let changes = crate::decode(tenant, &bytes).map_err(|_| RestoreFailure)?;
        RestoredSegment::new(bytes, changes)
    }
}
