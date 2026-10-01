use std::future::Future;

use fleetiq_domain::{
    AssetId, CommunicationDevice, DeviceEndpoint, DeviceEndpointId, DeviceEndpointKindId,
    DeviceEndpointName, TenantId,
};
use fleetiq_metadata::DeviceRepository;
use sqlx::PgPool;

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

/// PostgreSQL implementation of the communication-device repository port.
#[derive(Clone, Debug)]
pub struct PostgresDeviceRepository {
    pool: PgPool,
}

impl PostgresDeviceRepository {
    /// Constructs a device repository using an existing connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Inserts device capability and all declared endpoints atomically.
    ///
    /// The data-modifying common table expression first attaches communication
    /// capability to the existing asset. One `UNNEST ... WITH ORDINALITY` then
    /// inserts all endpoints while retaining their declaration order. Empty
    /// arrays still register the device and produce no endpoint rows.
    async fn insert_device(
        &self,
        device: &CommunicationDevice,
    ) -> Result<(), PostgresRepositoryError> {
        let endpoints = device.endpoints();
        let endpoint_ids: Vec<_> = endpoints
            .iter()
            .map(|endpoint| endpoint.id().as_str())
            .collect();
        let endpoint_kind_ids: Vec<_> = endpoints
            .iter()
            .map(|endpoint| endpoint.kind().as_str())
            .collect();
        let endpoint_names: Vec<_> = endpoints
            .iter()
            .map(|endpoint| endpoint.name().as_str())
            .collect();

        sqlx::query!(
            r"
            WITH inserted_device AS (
                INSERT INTO metadata.communication_devices
                    (tenant_id, device_asset_id)
                VALUES ($1, $2)
                RETURNING tenant_id, device_asset_id
            )
            INSERT INTO metadata.device_endpoints
                (tenant_id, device_asset_id, endpoint_id, endpoint_kind_id,
                 display_name, endpoint_ordinal)
            SELECT
                inserted_device.tenant_id,
                inserted_device.device_asset_id,
                endpoint.endpoint_id,
                endpoint.endpoint_kind_id,
                endpoint.display_name,
                endpoint.endpoint_ordinal
            FROM inserted_device
            CROSS JOIN UNNEST($3::text[], $4::text[], $5::text[]) WITH ORDINALITY
                AS endpoint(endpoint_id, endpoint_kind_id, display_name, endpoint_ordinal)
            ",
            device.tenant_id().as_str(),
            device.asset_id().as_str(),
            &endpoint_ids as &[&str],
            &endpoint_kind_ids as &[&str],
            &endpoint_names as &[&str]
        )
        .execute(&self.pool)
        .await
        .map_err(classify_database_error)?;

        Ok(())
    }

    /// Reconstructs device capability and its ordered endpoint collection.
    ///
    /// The parent read establishes whether the requested asset is registered as
    /// a device. A separate indexed child read retains ordinary `query_as!`
    /// column checking and distinguishes absence from a device with no endpoints.
    async fn find_device(
        &self,
        tenant_id: &TenantId,
        asset_id: &AssetId,
    ) -> Result<Option<CommunicationDevice>, PostgresRepositoryError> {
        let row = sqlx::query_as!(
            CommunicationDeviceRow,
            r"
            SELECT device_asset_id
            FROM metadata.communication_devices
            WHERE tenant_id = $1 AND device_asset_id = $2
            ",
            tenant_id.as_str(),
            asset_id.as_str()
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let endpoint_rows = sqlx::query_as!(
            DeviceEndpointRow,
            r"
            SELECT endpoint_id, endpoint_kind_id, display_name
            FROM metadata.device_endpoints
            WHERE tenant_id = $1 AND device_asset_id = $2
            ORDER BY endpoint_ordinal
            ",
            tenant_id.as_str(),
            asset_id.as_str()
        )
        .fetch_all(&self.pool)
        .await
        .map_err(classify_database_error)?;

        row.into_domain(tenant_id.clone(), endpoint_rows).map(Some)
    }
}

impl DeviceRepository for PostgresDeviceRepository {
    type Error = PostgresRepositoryError;

    fn insert<'a>(
        &'a self,
        device: &'a CommunicationDevice,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'a {
        self.insert_device(device)
    }

    fn find_by_asset<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        asset_id: &'a AssetId,
    ) -> impl Future<Output = Result<Option<CommunicationDevice>, Self::Error>> + Send + 'a {
        self.find_device(tenant_id, asset_id)
    }
}

#[derive(Debug)]
struct CommunicationDeviceRow {
    device_asset_id: String,
}

impl CommunicationDeviceRow {
    fn into_domain(
        self,
        tenant_id: TenantId,
        endpoints: Vec<DeviceEndpointRow>,
    ) -> Result<CommunicationDevice, PostgresRepositoryError> {
        let mut device = CommunicationDevice::new(
            tenant_id,
            AssetId::new(self.device_asset_id)
                .map_err(|error| corrupt("communication_devices.device_asset_id", error))?,
        );

        for endpoint in endpoints {
            device
                .add_endpoint(endpoint.into_domain()?)
                .map_err(|error| corrupt("device_endpoints", error))?;
        }

        Ok(device)
    }
}

#[derive(Debug)]
struct DeviceEndpointRow {
    endpoint_id: String,
    endpoint_kind_id: String,
    display_name: String,
}

impl DeviceEndpointRow {
    fn into_domain(self) -> Result<DeviceEndpoint, PostgresRepositoryError> {
        Ok(DeviceEndpoint::new(
            DeviceEndpointId::new(self.endpoint_id)
                .map_err(|error| corrupt("device_endpoints.endpoint_id", error))?,
            DeviceEndpointKindId::new(self.endpoint_kind_id)
                .map_err(|error| corrupt("device_endpoints.endpoint_kind_id", error))?,
            DeviceEndpointName::new(self.display_name)
                .map_err(|error| corrupt("device_endpoints.display_name", error))?,
        ))
    }
}
