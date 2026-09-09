//! Integration tests for the PostgreSQL semantic-definition repository.

use std::error::Error;

use fleetiq_application::DefinitionRepository;
use fleetiq_domain::{
    AssetTypeDefinition, AssetTypeId, AssetTypeRef, DefinitionName, DefinitionVersion,
    PropertyDefinition, PropertyDefinitionId, PropertyDefinitionRef, PropertyValueKind,
    RelationshipTypeDefinition, RelationshipTypeId, RelationshipTypeRef, TenantId, UnitId,
};
use fleetiq_postgres::{PostgresDefinitionRepository, PostgresRepositoryError};
use sqlx::PgPool;

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("test tenant ID should be valid")
}

fn version(value: u32) -> DefinitionVersion {
    DefinitionVersion::new(value).expect("test definition version should be valid")
}

fn property_reference(id: &str, version_number: u32) -> PropertyDefinitionRef {
    PropertyDefinitionRef::new(
        PropertyDefinitionId::new(id).expect("test property ID should be valid"),
        version(version_number),
    )
}

fn property_definition(id: &str, name: &str, canonical_unit: Option<&str>) -> PropertyDefinition {
    PropertyDefinition::new(
        property_reference(id, 1),
        DefinitionName::new(name).expect("test definition name should be valid"),
        PropertyValueKind::Decimal,
        canonical_unit.map(|unit| UnitId::new(unit).expect("test unit ID should be valid")),
    )
    .expect("test property definition should be valid")
}

fn asset_type_reference(version_number: u32) -> AssetTypeRef {
    AssetTypeRef::new(
        AssetTypeId::new("generic.power-system").expect("test asset type ID should be valid"),
        version(version_number),
    )
}

fn relationship_type_reference(version_number: u32) -> RelationshipTypeRef {
    RelationshipTypeRef::new(
        RelationshipTypeId::new("physical.contains")
            .expect("test relationship type ID should be valid"),
        version(version_number),
    )
}

async fn seed_tenant(pool: &PgPool, tenant: &TenantId) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
        .bind(tenant.as_str())
        .execute(pool)
        .await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn property_definition_round_trip_preserves_domain_values(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant(&pool, &tenant).await?;
    let repository = PostgresDefinitionRepository::new(pool);
    let expected = property_definition("electrical.voltage", "Voltage", Some("V"));

    repository.insert_property(&tenant, &expected).await?;
    let actual = repository
        .find_property(&tenant, expected.reference())
        .await?;

    assert_eq!(actual, Some(expected));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn asset_type_round_trip_preserves_property_declaration_order(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant(&pool, &tenant).await?;
    let repository = PostgresDefinitionRepository::new(pool);
    let voltage = property_definition("electrical.voltage", "Voltage", Some("V"));
    let temperature = property_definition("thermal.temperature", "Temperature", Some("Cel"));
    repository.insert_property(&tenant, &voltage).await?;
    repository.insert_property(&tenant, &temperature).await?;

    let mut expected = AssetTypeDefinition::new(
        asset_type_reference(1),
        DefinitionName::new("Power system").expect("test name should be valid"),
    );
    expected.add_property(temperature.reference().clone())?;
    expected.add_property(voltage.reference().clone())?;

    repository.insert_asset_type(&tenant, &expected).await?;
    let actual = repository
        .find_asset_type(&tenant, expected.reference())
        .await?;

    assert_eq!(actual, Some(expected));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_property_rolls_back_asset_type_definition(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant(&pool, &tenant).await?;
    let repository = PostgresDefinitionRepository::new(pool);
    let mut definition = AssetTypeDefinition::new(
        asset_type_reference(1),
        DefinitionName::new("Power system").expect("test name should be valid"),
    );
    definition.add_property(property_reference("missing.property", 1))?;

    let error = repository
        .insert_asset_type(&tenant, &definition)
        .await
        .expect_err("missing property should violate its foreign key");

    assert!(matches!(
        error,
        PostgresRepositoryError::InvalidReference {
            constraint: Some(constraint),
            ..
        } if constraint == "fk_asset_type_properties_property"
    ));
    assert_eq!(
        repository
            .find_asset_type(&tenant, definition.reference())
            .await?,
        None,
        "the failed aggregate insert must roll back its asset-type row"
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn relationship_type_versions_are_exact_and_tenant_scoped(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let first_tenant = tenant_id("tenant-a");
    let second_tenant = tenant_id("tenant-b");
    seed_tenant(&pool, &first_tenant).await?;
    seed_tenant(&pool, &second_tenant).await?;
    let repository = PostgresDefinitionRepository::new(pool);
    let reference = relationship_type_reference(1);
    let first = RelationshipTypeDefinition::new(
        reference.clone(),
        DefinitionName::new("Contains").expect("test name should be valid"),
    );
    let second = RelationshipTypeDefinition::new(
        reference.clone(),
        DefinitionName::new("Includes").expect("test name should be valid"),
    );
    let next_reference = relationship_type_reference(2);
    let next = RelationshipTypeDefinition::new(
        next_reference.clone(),
        DefinitionName::new("Contains, version 2").expect("test name should be valid"),
    );

    repository
        .insert_relationship_type(&first_tenant, &first)
        .await?;
    repository
        .insert_relationship_type(&second_tenant, &second)
        .await?;
    repository
        .insert_relationship_type(&first_tenant, &next)
        .await?;

    assert_eq!(
        repository
            .find_relationship_type(&first_tenant, &reference)
            .await?,
        Some(first)
    );
    assert_eq!(
        repository
            .find_relationship_type(&second_tenant, &reference)
            .await?,
        Some(second)
    );
    assert_eq!(
        repository
            .find_relationship_type(&first_tenant, &next_reference)
            .await?,
        Some(next)
    );
    assert!(matches!(
        repository
            .insert_relationship_type(
                &first_tenant,
                &RelationshipTypeDefinition::new(
                    reference,
                    DefinitionName::new("Duplicate").expect("test name should be valid"),
                ),
            )
            .await
            .expect_err("an exact definition version cannot be inserted twice"),
        PostgresRepositoryError::Conflict {
            constraint: Some(constraint),
            ..
        } if constraint == "pk_relationship_type_definitions"
    ));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn published_definitions_cannot_be_updated(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant(&pool, &tenant).await?;
    let repository = PostgresDefinitionRepository::new(pool.clone());
    let definition = property_definition("electrical.voltage", "Voltage", Some("V"));
    repository.insert_property(&tenant, &definition).await?;

    let error = sqlx::query(
        "UPDATE metadata.property_definitions SET display_name = 'Changed' WHERE tenant_id = $1",
    )
    .bind(tenant.as_str())
    .execute(&pool)
    .await
    .expect_err("published definition update should be rejected");

    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code),
        Some("55000".into())
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn published_asset_type_cannot_gain_another_property(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant(&pool, &tenant).await?;
    let repository = PostgresDefinitionRepository::new(pool.clone());
    let property = property_definition("electrical.voltage", "Voltage", Some("V"));
    repository.insert_property(&tenant, &property).await?;
    let asset_type = AssetTypeDefinition::new(
        asset_type_reference(1),
        DefinitionName::new("Power system").expect("test name should be valid"),
    );
    repository.insert_asset_type(&tenant, &asset_type).await?;

    let error = sqlx::query(
        r"
        INSERT INTO metadata.asset_type_properties
            (tenant_id, type_id, type_version, property_id,
             property_version, property_ordinal)
        VALUES ($1, 'generic.power-system', 1, 'electrical.voltage', 1, 1)
        ",
    )
    .bind(tenant.as_str())
    .execute(&pool)
    .await
    .expect_err("a published asset type must retain its complete property set");

    let database_error = error
        .as_database_error()
        .expect("constraint failure should be a PostgreSQL database error");
    assert_eq!(database_error.code().as_deref(), Some("23514"));
    assert_eq!(
        database_error.constraint(),
        Some("ck_asset_type_definition_property_membership")
    );
    Ok(())
}
