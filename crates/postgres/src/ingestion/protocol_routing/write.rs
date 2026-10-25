use super::{
    PROTOCOL_ADMISSION_POLICY, PgConnection, PostgresProtocolRoutingRepository,
    PostgresRepositoryError, ProtocolDeviceRoute, ProtocolRouteCommand, ProtocolRouteWrite,
    classify_database_error, corrupt, read,
};
use crate::{
    PostgresProtocolArtifactStore, PostgresProtocolPackageCatalog,
    PostgresProtocolPackageCatalogError,
};
use fleetiq_ingestion::{ProtocolArtifactStore, ProtocolPackageCatalog};
impl PostgresProtocolRoutingRepository {
    pub(super) async fn replace(
        &self,
        command: &ProtocolRouteCommand,
    ) -> Result<ProtocolRouteWrite, PostgresRepositoryError> {
        let Some(next) = command.expected_revision.checked_add(1) else {
            return Ok(ProtocolRouteWrite::Conflict);
        };
        // Rehash immutable content before commissioning, outside the short lock
        // transaction. The admission FK remains the final durable prerequisite.
        if let Some(selection) = &command.selection {
            let release = PostgresProtocolPackageCatalog::new(self.pool.clone())
                .find(&command.tenant_id, &selection.package)
                .await
                .map_err(|error| match error {
                    PostgresProtocolPackageCatalogError::Repository(error) => error,
                    PostgresProtocolPackageCatalogError::IdentityCollision { .. } => {
                        corrupt("protocol_routes", "unexpected catalogue conflict")
                    }
                })?;
            let Some(release) = release else {
                return Ok(ProtocolRouteWrite::Unavailable);
            };
            let store = PostgresProtocolArtifactStore::new(self.pool.clone());
            if store.find(&release).await?.is_none()
                || store.admission(&release, selection.world).await?.is_none()
            {
                return Ok(ProtocolRouteWrite::Unavailable);
            }
        }
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
        let active = sqlx::query_scalar!(
            "SELECT is_active FROM metadata.tenants WHERE tenant_id=$1 FOR SHARE",
            command.tenant_id.as_str()
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(classify_database_error)?;
        if active != Some(true) {
            return Ok(ProtocolRouteWrite::Unavailable);
        }
        sqlx::query!(r"INSERT INTO ingestion.protocol_route_heads (tenant_id,device_asset_id,channel_id,revision)
            SELECT tenant_id,device_asset_id,$3,0 FROM metadata.communication_devices WHERE tenant_id=$1 AND device_asset_id=$2
            ON CONFLICT (tenant_id,device_asset_id,channel_id) DO NOTHING",command.tenant_id.as_str(),command.device_asset_id.as_str(),command.channel_id.as_str())
            .execute(&mut *transaction).await.map_err(classify_database_error)?;
        let current = sqlx::query_scalar!(
            r"SELECT revision FROM ingestion.protocol_route_heads
            WHERE tenant_id=$1 AND device_asset_id=$2 AND channel_id=$3 FOR UPDATE",
            command.tenant_id.as_str(),
            command.device_asset_id.as_str(),
            command.channel_id.as_str()
        )
        .fetch_optional(&mut *transaction)
        .await
        .map_err(classify_database_error)?;
        let Some(current) = current else {
            return Ok(ProtocolRouteWrite::Unavailable);
        };
        let existing = read::find(
            &mut transaction,
            &command.tenant_id,
            &command.device_asset_id,
            &command.channel_id,
            if current == i64::from(command.expected_revision) {
                None
            } else {
                Some(i64::from(next))
            },
        )
        .await?;
        if current != i64::from(command.expected_revision) {
            if let Some(route) = existing
                && route.selection == command.selection
                && route.actor_id == command.actor_id
                && route.policy_version
                    == command
                        .selection
                        .as_ref()
                        .map(|_| PROTOCOL_ADMISSION_POLICY)
            {
                transaction
                    .commit()
                    .await
                    .map_err(classify_database_error)?;
                return Ok(ProtocolRouteWrite::Committed(route));
            }
            return Ok(ProtocolRouteWrite::Conflict);
        }
        if existing
            .as_ref()
            .is_some_and(|route| command.recorded_at.get() < route.recorded_at.get())
        {
            return Ok(ProtocolRouteWrite::Conflict);
        }
        let route = append_generation(&mut transaction, command, next).await?;
        transaction
            .commit()
            .await
            .map_err(classify_database_error)?;
        Ok(ProtocolRouteWrite::Committed(route))
    }
}

// Persist the complete immutable generation before the caller commits the head.
async fn append_generation(
    connection: &mut PgConnection,
    command: &ProtocolRouteCommand,
    next: u32,
) -> Result<ProtocolDeviceRoute, PostgresRepositoryError> {
    let package = command
        .selection
        .as_ref()
        .map(|selection| selection.package.id().as_str());
    let version = command
        .selection
        .as_ref()
        .map(|selection| selection.package.version().as_str());
    let world = command
        .selection
        .as_ref()
        .map(|selection| selection.world.as_str());
    let policy = command
        .selection
        .as_ref()
        .map(|_| PROTOCOL_ADMISSION_POLICY);
    let next = i64::from(next);
    // The head is private until commit. Deferred referential integrity binds
    // its final value to the appended immutable generation, including on retry.
    sqlx::query!(r"UPDATE ingestion.protocol_route_heads SET revision=$4 WHERE tenant_id=$1 AND device_asset_id=$2 AND channel_id=$3",
            command.tenant_id.as_str(),command.device_asset_id.as_str(),command.channel_id.as_str(),next)
            .execute(&mut *connection).await.map_err(classify_database_error)?;
    let time = command.recorded_at.get();
    sqlx::query!(r"INSERT INTO ingestion.protocol_route_revisions
            (tenant_id,device_asset_id,channel_id,revision,protocol_package_id,protocol_package_version,wit_world,policy_version,actor_id,recorded_at_ms)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",command.tenant_id.as_str(),command.device_asset_id.as_str(),command.channel_id.as_str(),next,
            package,version,world,policy,command.actor_id.as_str(),time)
            .execute(&mut *connection).await.map_err(classify_database_error)?;
    let route = read::find(
        connection,
        &command.tenant_id,
        &command.device_asset_id,
        &command.channel_id,
        None,
    )
    .await?
    .ok_or_else(|| corrupt("protocol_routes", "committed route missing"))?;
    Ok(route)
}
