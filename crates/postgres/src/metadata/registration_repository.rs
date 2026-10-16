//! Atomic creation reuses the strict aggregate SQL with attributed evidence.
use super::{PostgresAssetRepository, PostgresDeviceRepository};
use crate::{
    PostgresRepositoryError,
    error::{classify_database_error, corrupt},
};
use fleetiq_domain::{ActorId, UnixTimeMillis};
use fleetiq_metadata::{
    AssetRepository, DeviceRepository, MetadataRegistration, RegistrationOutcome,
    RegistrationReceipt, RegistrationRepository,
};
use sqlx::PgPool;

/// PostgreSQL adapter for attributed asset and communication-device creation.
#[derive(Clone, Debug)]
pub struct PostgresRegistrationRepository {
    pool: PgPool,
}
impl PostgresRegistrationRepository {
    /// Shares the configured pool without changing strict insert consumers.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Reads committed content only after rolling back the losing transaction.
    ///
    /// The primary-key collision waits for the complete winner. Separate fresh
    /// READ COMMITTED reads avoid the stale snapshot of an ON CONFLICT CTE.
    /// Registered aggregates have no supported update/delete operation. If a
    /// maintenance writer changes them, comparison fails closed on a retry.
    async fn resolve(
        &self,
        requested: &MetadataRegistration,
    ) -> Result<RegistrationOutcome, PostgresRepositoryError> {
        let tenant = requested.tenant_id();
        let asset = requested.asset_id();
        let stored = match requested {
            MetadataRegistration::Asset(_) => PostgresAssetRepository::new(self.pool.clone())
                .find_by_id(tenant, asset)
                .await?
                .map(MetadataRegistration::Asset),
            MetadataRegistration::Device(_) => PostgresDeviceRepository::new(self.pool.clone())
                .find_by_asset(tenant, asset)
                .await?
                .map(MetadataRegistration::Device),
        };
        if !stored
            .as_ref()
            .is_some_and(|stored| requested.same_content(stored))
        {
            return Ok(RegistrationOutcome::Conflict);
        }
        let row = sqlx::query!(
            "SELECT actor_id, recorded_at_ms FROM metadata.registration_records
             WHERE tenant_id = $1 AND asset_id = $2 AND registration_kind = $3",
            tenant.as_str(),
            asset.as_str(),
            kind(requested),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;
        let Some(row) = row else {
            // Legacy strict inserts have no actor/time: do not invent history.
            return Ok(RegistrationOutcome::Conflict);
        };
        Ok(RegistrationOutcome::AlreadyRegistered(
            RegistrationReceipt::new(
                ActorId::new(row.actor_id)
                    .map_err(|e| corrupt("registration_records.actor_id", e))?,
                UnixTimeMillis::new(row.recorded_at_ms),
            ),
        ))
    }
}
impl RegistrationRepository for PostgresRegistrationRepository {
    type Error = PostgresRepositoryError;

    /// One transaction owns parent, batched children and immutable evidence.
    ///
    /// Preserve the existing aggregate INSERT algorithms. Only known parent
    /// identity collisions are candidates for exact retry resolution; external
    /// identifier ownership conflicts are terminal collisions. Other constraints
    /// and operational failures remain errors. A commit error is ambiguous and
    /// must not be advertised as success; an exact retry resolves durable state.
    async fn register(
        &self,
        registration: &MetadataRegistration,
        actor: &ActorId,
        received_at: UnixTimeMillis,
    ) -> Result<RegistrationOutcome, Self::Error> {
        let mut transaction = self.pool.begin().await.map_err(classify_database_error)?;
        let attempt = match registration {
            MetadataRegistration::Asset(asset) => {
                PostgresAssetRepository::insert_on(&mut *transaction, asset).await
            }
            MetadataRegistration::Device(device) => {
                PostgresDeviceRepository::insert_on(&mut *transaction, device).await
            }
        };
        if let Err(error) = attempt {
            transaction
                .rollback()
                .await
                .map_err(classify_database_error)?;
            return match &error {
                PostgresRepositoryError::Conflict {
                    constraint: Some(name),
                    ..
                } if name
                    == match registration {
                        MetadataRegistration::Asset(_) => "pk_assets",
                        MetadataRegistration::Device(_) => "pk_communication_devices",
                    } =>
                {
                    self.resolve(registration).await
                }
                PostgresRepositoryError::Conflict {
                    constraint: Some(name),
                    ..
                } if name == "uq_asset_external_identifier" => Ok(RegistrationOutcome::Conflict),
                _ => Err(error),
            };
        }
        sqlx::query!(
            "INSERT INTO metadata.registration_records
             (tenant_id, asset_id, registration_kind, actor_id, recorded_at_ms)
             VALUES ($1, $2, $3, $4, $5)",
            registration.tenant_id().as_str(),
            registration.asset_id().as_str(),
            kind(registration),
            actor.as_str(),
            received_at.get(),
        )
        .execute(&mut *transaction)
        .await
        .map_err(classify_database_error)?;
        transaction
            .commit()
            .await
            .map_err(classify_database_error)?;
        Ok(RegistrationOutcome::Registered(RegistrationReceipt::new(
            actor.clone(),
            received_at,
        )))
    }
}
fn kind(registration: &MetadataRegistration) -> &'static str {
    match registration {
        MetadataRegistration::Asset(_) => "asset",
        MetadataRegistration::Device(_) => "device",
    }
}
