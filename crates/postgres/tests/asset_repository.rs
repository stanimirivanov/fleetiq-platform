//! Integration tests for the PostgreSQL asset repository.

use std::error::Error;

use fleetiq_domain::{
    Asset, AssetId, AssetName, AssetTypeId, AssetTypeRef, DefinitionVersion, ExternalIdentifier,
    ExternalIdentifierAuthority, ExternalIdentifierKind, ExternalIdentifierValue, TenantId,
};
use fleetiq_metadata::{AssetCatalogueEntry, AssetPageQuery, AssetRepository};
use fleetiq_postgres::{PostgresAssetRepository, PostgresAssetRepositoryError};
use sqlx::PgPool;

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("test tenant ID should be valid")
}

fn asset(tenant: &str, id: &str, name: &str) -> Asset {
    Asset::new(
        tenant_id(tenant),
        AssetId::new(id).expect("test asset ID should be valid"),
        AssetTypeRef::new(
            AssetTypeId::new("generic.machine").expect("test asset type ID should be valid"),
            DefinitionVersion::new(1).expect("test definition version should be valid"),
        ),
        AssetName::new(name).expect("test asset name should be valid"),
    )
}

fn summary(asset: &Asset) -> AssetCatalogueEntry {
    AssetCatalogueEntry::new(
        asset.tenant_id().clone(),
        asset.id().clone(),
        asset.name().clone(),
        asset.asset_type().clone(),
    )
}

fn serial_number(value: &str) -> ExternalIdentifier {
    ExternalIdentifier::new(
        ExternalIdentifierKind::new("manufacturer.serial-number")
            .expect("test identifier kind should be valid"),
        Some(
            ExternalIdentifierAuthority::new("manufacturer-a")
                .expect("test identifier authority should be valid"),
        ),
        ExternalIdentifierValue::new(value).expect("test identifier value should be valid"),
    )
}

fn imei(value: &str) -> ExternalIdentifier {
    ExternalIdentifier::new(
        ExternalIdentifierKind::new("imei").expect("test identifier kind should be valid"),
        None,
        ExternalIdentifierValue::new(value).expect("test identifier value should be valid"),
    )
}

async fn seed_tenant_and_asset_type(pool: &PgPool, tenant: &str) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
        .bind(tenant)
        .execute(pool)
        .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.asset_type_definitions
            (tenant_id, type_id, version, display_name, property_count)
        VALUES ($1, 'generic.machine', 1, 'Machine', 0)
        ",
    )
    .bind(tenant)
    .execute(pool)
    .await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn asset_round_trip_preserves_domain_values(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant_and_asset_type(&pool, "tenant-a").await?;
    let repository = PostgresAssetRepository::new(pool);
    let mut expected = asset("tenant-a", "asset-1", "Primary machine");
    expected.add_external_identifier(imei("123456789012345"))?;
    expected.add_external_identifier(serial_number("SN-100"))?;

    repository.insert(&expected).await?;
    let actual = repository
        .find_by_id(expected.tenant_id(), expected.id())
        .await?;

    assert_eq!(actual, Some(expected));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn tenant_scope_allows_equal_asset_ids_without_leaking(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant_and_asset_type(&pool, "tenant-a").await?;
    seed_tenant_and_asset_type(&pool, "tenant-b").await?;
    let repository = PostgresAssetRepository::new(pool);
    let first = asset("tenant-a", "shared-id", "Tenant A machine");
    let second = asset("tenant-b", "shared-id", "Tenant B machine");

    repository.insert(&first).await?;
    repository.insert(&second).await?;

    assert_eq!(
        repository.find_by_id(first.tenant_id(), first.id()).await?,
        Some(first)
    );
    assert_eq!(
        repository
            .find_by_id(second.tenant_id(), second.id())
            .await?,
        Some(second)
    );
    assert_eq!(
        repository
            .find_by_id(&tenant_id("tenant-c"), &AssetId::new("shared-id")?)
            .await?,
        None
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn catalogue_pages_asset_summaries_in_tenant_id_order(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_tenant_and_asset_type(&pool, "tenant-a").await?;
    seed_tenant_and_asset_type(&pool, "tenant-b").await?;
    let repository = PostgresAssetRepository::new(pool);
    let mut first = asset("tenant-a", "asset-a", "First");
    first.add_external_identifier(imei("111111111111111"))?;
    first.add_external_identifier(serial_number("SN-A"))?;
    let second = asset("tenant-a", "asset-b", "Second");
    let mut third = asset("tenant-a", "asset-c", "Third");
    third.add_external_identifier(serial_number("SN-C"))?;
    let other = asset("tenant-b", "asset-b", "Other tenant");
    for value in [&third, &other, &first, &second] {
        repository.insert(value).await?;
    }

    let first_page = repository
        .list_page(&tenant_id("tenant-a"), &AssetPageQuery::new(2, None)?)
        .await?;
    assert_eq!(first_page.assets(), &[summary(&first), summary(&second)]);
    assert_eq!(first_page.next_after(), Some(first_page.assets()[1].id()));
    let second_page = repository
        .list_page(
            &tenant_id("tenant-a"),
            &AssetPageQuery::new(2, first_page.next_after().cloned())?,
        )
        .await?;
    assert_eq!(second_page.assets(), &[summary(&third)]);
    assert_eq!(second_page.next_after(), None);
    let other_page = repository
        .list_page(&tenant_id("tenant-b"), &AssetPageQuery::new(2, None)?)
        .await?;
    assert_eq!(other_page.assets(), &[summary(&other)]);
    assert_eq!(other_page.next_after(), None);
    let empty = repository
        .list_page(
            &tenant_id("tenant-a"),
            &AssetPageQuery::new(2, Some(AssetId::new("asset-z")?))?,
        )
        .await?;
    assert!(empty.assets().is_empty());
    assert_eq!(empty.next_after(), None);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_asset_type_version_is_an_invalid_reference(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a')")
        .execute(&pool)
        .await?;
    let repository = PostgresAssetRepository::new(pool);

    let error = repository
        .insert(&asset("tenant-a", "asset-1", "Primary machine"))
        .await
        .expect_err("missing asset type should violate its foreign key");

    assert!(matches!(
        error,
        PostgresAssetRepositoryError::InvalidReference {
            constraint: Some(constraint),
            ..
        } if constraint == "fk_assets_type"
    ));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn external_identifier_is_unique_within_a_tenant(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_tenant_and_asset_type(&pool, "tenant-a").await?;
    let repository = PostgresAssetRepository::new(pool);
    let mut first = asset("tenant-a", "asset-1", "First machine");
    let mut second = asset("tenant-a", "asset-2", "Second machine");
    first.add_external_identifier(serial_number("SN-100"))?;
    second.add_external_identifier(serial_number("SN-100"))?;

    repository.insert(&first).await?;
    let error = repository
        .insert(&second)
        .await
        .expect_err("duplicate external identifier should be rejected");

    assert!(matches!(
        error,
        PostgresAssetRepositoryError::Conflict {
            constraint: Some(constraint),
            ..
        } if constraint == "uq_asset_external_identifier"
    ));
    assert_eq!(
        repository
            .find_by_id(second.tenant_id(), second.id())
            .await?,
        None,
        "the failed aggregate insert must roll back its asset row"
    );
    Ok(())
}
