//! Real PostgreSQL proves atomicity, winner visibility and durable provenance.
mod metadata_registration {
    mod concurrency;
    mod invariants;
}
use fleetiq_domain::{
    ActorId, Asset, AssetId, AssetName, AssetTypeId, AssetTypeRef, CommunicationDevice,
    DefinitionVersion, DeviceEndpoint, DeviceEndpointId, DeviceEndpointKindId, DeviceEndpointName,
    ExternalIdentifier, ExternalIdentifierAuthority, ExternalIdentifierKind,
    ExternalIdentifierValue, TenantId, UnixTimeMillis,
};
use fleetiq_metadata::{
    AssetRepository, DeviceRepository, MetadataRegistration, RegistrationOutcome,
    RegistrationReceipt, RegistrationRepository,
};
use fleetiq_postgres::{
    PostgresAssetRepository, PostgresDeviceRepository, PostgresRegistrationRepository,
    PostgresRepositoryError,
};
use sqlx::PgPool;
use std::error::Error;
type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

async fn seed(pool: &PgPool) -> Result {
    sqlx::raw_sql("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a'),('tenant-b');
        INSERT INTO identity.actors (actor_id) VALUES ('operator-a'),('operator-b');
        INSERT INTO metadata.asset_type_definitions (tenant_id,type_id,version,display_name,property_count)
        VALUES ('tenant-a','equipment',1,'Equipment',0),('tenant-b','equipment',1,'Equipment',0);")
        .execute(pool).await?;
    Ok(())
}
fn asset(tenant: &str, id: &str) -> Asset {
    Asset::new(
        TenantId::new(tenant).unwrap(),
        AssetId::new(id).unwrap(),
        AssetTypeRef::new(
            AssetTypeId::new("equipment").unwrap(),
            DefinitionVersion::new(1).unwrap(),
        ),
        AssetName::new("Machine").unwrap(),
    )
}
fn identifier(kind: &str, authority: Option<&str>, value: &str) -> ExternalIdentifier {
    ExternalIdentifier::new(
        ExternalIdentifierKind::new(kind).unwrap(),
        authority.map(|a| ExternalIdentifierAuthority::new(a).unwrap()),
        ExternalIdentifierValue::new(value).unwrap(),
    )
}
fn device(tenant: &str, id: &str, endpoints: &[&str]) -> CommunicationDevice {
    let mut device =
        CommunicationDevice::new(TenantId::new(tenant).unwrap(), AssetId::new(id).unwrap());
    for id in endpoints {
        device
            .add_endpoint(DeviceEndpoint::new(
                DeviceEndpointId::new(*id).unwrap(),
                DeviceEndpointKindId::new("bus.can").unwrap(),
                DeviceEndpointName::new(*id).unwrap(),
            ))
            .unwrap();
    }
    device
}
fn actor() -> ActorId {
    ActorId::new("operator-a").unwrap()
}
fn receipt() -> RegistrationReceipt {
    RegistrationReceipt::new(actor(), UnixTimeMillis::new(123))
}
async fn register(pool: &PgPool, value: &MetadataRegistration) -> Result<RegistrationOutcome> {
    Ok(PostgresRegistrationRepository::new(pool.clone())
        .register(value, &actor(), UnixTimeMillis::new(123))
        .await?)
}
async fn counts(pool: &PgPool) -> Result<(i64, i64, i64, i64, i64)> {
    Ok(sqlx::query_as(
        "SELECT (SELECT count(*) FROM metadata.assets),
        (SELECT count(*) FROM metadata.asset_external_identifiers),
        (SELECT count(*) FROM metadata.communication_devices),
        (SELECT count(*) FROM metadata.device_endpoints),
        (SELECT count(*) FROM metadata.registration_records)",
    )
    .fetch_one(pool)
    .await?)
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn aggregate_round_trips_and_exact_retries_keep_first_attribution(pool: PgPool) -> Result {
    seed(&pool).await?;
    let mut expected = asset("tenant-a", "asset-a");
    expected.add_external_identifier(identifier("imei", None, "350000000000001"))?;
    expected.add_external_identifier(identifier("serial", Some("maker"), "SN-1"))?;
    let request = MetadataRegistration::Asset(expected.clone());
    assert_eq!(
        register(&pool, &request).await?,
        RegistrationOutcome::Registered(receipt())
    );
    assert_eq!(
        PostgresAssetRepository::new(pool.clone())
            .find_by_id(expected.tenant_id(), expected.id())
            .await?,
        Some(expected)
    );
    let later = PostgresRegistrationRepository::new(pool.clone())
        .register(
            &request,
            &ActorId::new("operator-b")?,
            UnixTimeMillis::new(999),
        )
        .await?;
    assert_eq!(later, RegistrationOutcome::AlreadyRegistered(receipt()));
    let expected_device = device("tenant-a", "asset-a", &["can-one", "can-two"]);
    let request = MetadataRegistration::Device(expected_device.clone());
    assert_eq!(
        register(&pool, &request).await?,
        RegistrationOutcome::Registered(receipt())
    );
    assert_eq!(
        register(&pool, &request).await?,
        RegistrationOutcome::AlreadyRegistered(receipt())
    );
    assert_eq!(
        PostgresDeviceRepository::new(pool.clone())
            .find_by_asset(expected_device.tenant_id(), expected_device.asset_id())
            .await?,
        Some(expected_device)
    );
    assert_eq!(counts(&pool).await?, (1, 2, 1, 2, 2));
    Ok(())
}
#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn empty_children_still_create_parents_and_registration_evidence(pool: PgPool) -> Result {
    seed(&pool).await?;
    register(
        &pool,
        &MetadataRegistration::Asset(asset("tenant-a", "asset-a")),
    )
    .await?;
    register(
        &pool,
        &MetadataRegistration::Device(device("tenant-a", "asset-a", &[])),
    )
    .await?;
    assert_eq!(counts(&pool).await?, (1, 0, 1, 0, 2));
    Ok(())
}
