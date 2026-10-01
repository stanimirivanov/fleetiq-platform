//! Integration tests for the PostgreSQL communication-device repository.

use std::error::Error;

use fleetiq_domain::{
    AssetId, CommunicationDevice, DeviceEndpoint, DeviceEndpointId, DeviceEndpointKindId,
    DeviceEndpointName, TenantId,
};
use fleetiq_metadata::DeviceRepository;
use fleetiq_postgres::{PostgresDeviceRepository, PostgresDeviceRepositoryError};
use sqlx::PgPool;

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("test tenant ID should be valid")
}

fn endpoint(id: &str, kind: &str, name: &str) -> DeviceEndpoint {
    DeviceEndpoint::new(
        DeviceEndpointId::new(id).expect("test endpoint ID should be valid"),
        DeviceEndpointKindId::new(kind).expect("test endpoint kind should be valid"),
        DeviceEndpointName::new(name).expect("test endpoint name should be valid"),
    )
}

fn device(tenant: &str, asset: &str) -> CommunicationDevice {
    CommunicationDevice::new(
        tenant_id(tenant),
        AssetId::new(asset).expect("test asset ID should be valid"),
    )
}

async fn seed_tenant_with_assets(
    pool: &PgPool,
    tenant: &str,
    asset_ids: &[&str],
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
        .bind(tenant)
        .execute(pool)
        .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.asset_type_definitions
            (tenant_id, type_id, version, display_name, property_count)
        VALUES ($1, 'communication.gateway', 1, 'Communication gateway', 0)
        ",
    )
    .bind(tenant)
    .execute(pool)
    .await?;

    for asset_id in asset_ids {
        sqlx::query(
            r"
            INSERT INTO metadata.assets
                (tenant_id, asset_id, type_id, type_version, display_name)
            VALUES ($1, $2, 'communication.gateway', 1, $2)
            ",
        )
        .bind(tenant)
        .bind(asset_id)
        .execute(pool)
        .await?;
    }

    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn device_round_trip_preserves_ordered_endpoints(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant_with_assets(&pool, "tenant-a", &["gateway-1"]).await?;
    let repository = PostgresDeviceRepository::new(pool);
    let mut expected = device("tenant-a", "gateway-1");
    expected.add_endpoint(endpoint(
        "onboard",
        "device.onboard",
        "Onboard measurements",
    ))?;
    expected.add_endpoint(endpoint("can-primary", "bus.can", "Primary CAN interface"))?;

    repository.insert(&expected).await?;

    assert_eq!(
        repository
            .find_by_asset(expected.tenant_id(), expected.asset_id())
            .await?,
        Some(expected)
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn device_without_endpoints_is_persisted(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant_with_assets(&pool, "tenant-a", &["gateway-1"]).await?;
    let repository = PostgresDeviceRepository::new(pool);
    let expected = device("tenant-a", "gateway-1");

    repository.insert(&expected).await?;

    assert_eq!(
        repository
            .find_by_asset(expected.tenant_id(), expected.asset_id())
            .await?,
        Some(expected)
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn communication_capability_requires_an_asset_in_the_same_tenant(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant_with_assets(&pool, "tenant-a", &[]).await?;
    seed_tenant_with_assets(&pool, "tenant-b", &["gateway-1"]).await?;
    let repository = PostgresDeviceRepository::new(pool);

    let error = repository
        .insert(&device("tenant-a", "gateway-1"))
        .await
        .expect_err("another tenant's asset must not acquire device capability");

    assert!(matches!(
        error,
        PostgresDeviceRepositoryError::InvalidReference {
            constraint: Some(constraint),
            ..
        } if constraint == "fk_communication_devices_asset"
    ));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn endpoint_identity_is_scoped_by_device(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant_with_assets(&pool, "tenant-a", &["gateway-1", "gateway-2"]).await?;
    let repository = PostgresDeviceRepository::new(pool);
    let mut first = device("tenant-a", "gateway-1");
    let mut second = device("tenant-a", "gateway-2");
    first.add_endpoint(endpoint("can-primary", "bus.can", "First CAN interface"))?;
    second.add_endpoint(endpoint("can-primary", "bus.can", "Second CAN interface"))?;

    repository.insert(&first).await?;
    repository.insert(&second).await?;

    assert_eq!(
        repository
            .find_by_asset(first.tenant_id(), first.asset_id())
            .await?,
        Some(first)
    );
    assert_eq!(
        repository
            .find_by_asset(second.tenant_id(), second.asset_id())
            .await?,
        Some(second)
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn device_lookup_is_tenant_scoped(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant_with_assets(&pool, "tenant-a", &["gateway-1"]).await?;
    seed_tenant_with_assets(&pool, "tenant-b", &["gateway-1"]).await?;
    let repository = PostgresDeviceRepository::new(pool);
    let expected = device("tenant-a", "gateway-1");
    repository.insert(&expected).await?;

    assert_eq!(
        repository
            .find_by_asset(&tenant_id("tenant-b"), expected.asset_id())
            .await?,
        None
    );
    Ok(())
}
