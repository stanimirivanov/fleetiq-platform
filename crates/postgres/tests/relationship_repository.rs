//! Integration tests for the PostgreSQL asset-relationship repository.

use std::error::Error;

use fleetiq_application::{
    AssetRepository, DefinitionRepository, RelationshipRepository, RelationshipSnapshotQuery,
};
use fleetiq_domain::{
    Asset, AssetId, AssetName, AssetRelationship, AssetRelationshipId, AssetTypeDefinition,
    AssetTypeId, AssetTypeRef, DefinitionName, DefinitionVersion, EffectiveInterval,
    RecordRevision, RecordedRevision, RelationshipEndpoints, RelationshipTypeDefinition,
    RelationshipTypeId, RelationshipTypeRef, TenantId, UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresAssetRepository, PostgresDefinitionRepository, PostgresRelationshipRepository,
    PostgresRepositoryError,
};
use sqlx::PgPool;

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("test tenant ID should be valid")
}

fn asset_id(value: &str) -> AssetId {
    AssetId::new(value).expect("test asset ID should be valid")
}

fn definition_version(value: u32) -> DefinitionVersion {
    DefinitionVersion::new(value).expect("test definition version should be valid")
}

fn asset_type_reference() -> AssetTypeRef {
    AssetTypeRef::new(
        AssetTypeId::new("generic.machine").expect("test asset type ID should be valid"),
        definition_version(1),
    )
}

fn relationship_type_reference() -> RelationshipTypeRef {
    RelationshipTypeRef::new(
        RelationshipTypeId::new("physical.contains")
            .expect("test relationship type ID should be valid"),
        definition_version(1),
    )
}

fn revision(value: u64) -> RecordRevision {
    RecordRevision::new(value).expect("test record revision should be valid")
}

fn relationship(
    tenant: &TenantId,
    revision_number: u64,
    recorded_at_ms: i64,
    source: &str,
    target: &str,
    effective_start_ms: i64,
    effective_end_ms: Option<i64>,
) -> AssetRelationship {
    AssetRelationship::new(
        tenant.clone(),
        AssetRelationshipId::new("relationship-1").expect("test relationship ID should be valid"),
        RecordedRevision::new(
            revision(revision_number),
            UnixTimeMillis::new(recorded_at_ms),
        ),
        relationship_type_reference(),
        RelationshipEndpoints::new(asset_id(source), asset_id(target)),
        EffectiveInterval::new(
            UnixTimeMillis::new(effective_start_ms),
            effective_end_ms.map(UnixTimeMillis::new),
        )
        .expect("test effective interval should be valid"),
    )
}

async fn seed_tenant_graph(pool: &PgPool, tenant: &TenantId) -> Result<(), Box<dyn Error>> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
        .bind(tenant.as_str())
        .execute(pool)
        .await?;

    let definitions = PostgresDefinitionRepository::new(pool.clone());
    definitions
        .insert_asset_type(
            tenant,
            &AssetTypeDefinition::new(
                asset_type_reference(),
                DefinitionName::new("Machine").expect("test name should be valid"),
            ),
        )
        .await?;
    definitions
        .insert_relationship_type(
            tenant,
            &RelationshipTypeDefinition::new(
                relationship_type_reference(),
                DefinitionName::new("Contains").expect("test name should be valid"),
            ),
        )
        .await?;

    let assets = PostgresAssetRepository::new(pool.clone());
    for id in ["assembly-a", "component-b", "component-c"] {
        assets
            .insert(&Asset::new(
                tenant.clone(),
                asset_id(id),
                asset_type_reference(),
                AssetName::new(id).expect("test asset name should be valid"),
            ))
            .await?;
    }

    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn relationship_revision_round_trip_preserves_domain_values(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant_graph(&pool, &tenant).await?;
    let repository = PostgresRelationshipRepository::new(pool.clone());
    let expected = relationship(
        &tenant,
        1,
        2_000,
        "assembly-a",
        "component-b",
        1_000,
        Some(3_000),
    );

    repository.insert_revision(&expected).await?;
    let actual = repository
        .find_revision(&tenant, expected.id(), revision(1))
        .await?;

    assert_eq!(actual, Some(expected));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn revisions_must_be_contiguous_and_start_at_one(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant_graph(&pool, &tenant).await?;
    let repository = PostgresRelationshipRepository::new(pool);
    let second = relationship(&tenant, 2, 2_000, "assembly-a", "component-b", 1_000, None);

    assert!(matches!(
        repository
            .insert_revision(&second)
            .await
            .expect_err("a new relationship must start at revision one"),
        PostgresRepositoryError::UnexpectedRevision {
            expected: 1,
            actual: 2
        }
    ));

    let first = relationship(&tenant, 1, 1_000, "assembly-a", "component-b", 1_000, None);
    repository.insert_revision(&first).await?;
    let third = relationship(&tenant, 3, 3_000, "assembly-a", "component-b", 1_000, None);

    assert!(matches!(
        repository
            .insert_revision(&third)
            .await
            .expect_err("a relationship revision cannot skip its predecessor"),
        PostgresRepositoryError::UnexpectedRevision {
            expected: 2,
            actual: 3
        }
    ));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn recorded_time_cannot_move_backwards(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant_graph(&pool, &tenant).await?;
    let repository = PostgresRelationshipRepository::new(pool);
    repository
        .insert_revision(&relationship(
            &tenant,
            1,
            2_000,
            "assembly-a",
            "component-b",
            1_000,
            None,
        ))
        .await?;

    let error = repository
        .insert_revision(&relationship(
            &tenant,
            2,
            1_999,
            "assembly-a",
            "component-b",
            1_000,
            None,
        ))
        .await
        .expect_err("recorded time must be monotonic across revisions");

    assert!(matches!(
        error,
        PostgresRepositoryError::RecordedTimeRegression {
            previous_ms: 2_000,
            actual_ms: 1_999
        }
    ));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn endpoint_and_type_references_are_tenant_scoped(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let first_tenant = tenant_id("tenant-a");
    let second_tenant = tenant_id("tenant-b");
    seed_tenant_graph(&pool, &first_tenant).await?;
    seed_tenant_graph(&pool, &second_tenant).await?;
    sqlx::query("DELETE FROM metadata.assets WHERE tenant_id = $1 AND asset_id = 'component-b'")
        .bind(first_tenant.as_str())
        .execute(&pool)
        .await?;
    let repository = PostgresRelationshipRepository::new(pool.clone());

    let error = repository
        .insert_revision(&relationship(
            &first_tenant,
            1,
            2_000,
            "assembly-a",
            "component-b",
            1_000,
            None,
        ))
        .await
        .expect_err("an endpoint existing only in another tenant is invalid");

    assert!(matches!(
        error,
        PostgresRepositoryError::InvalidReference {
            constraint: Some(constraint),
            ..
        } if constraint == "fk_asset_relationship_revisions_target"
    ));
    let identity_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM metadata.asset_relationships WHERE tenant_id = $1",
    )
    .bind(first_tenant.as_str())
    .fetch_one(&pool)
    .await?;
    assert_eq!(
        identity_count, 0,
        "the failed append must not leak its head"
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn snapshot_respects_recorded_and_effective_time(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant_graph(&pool, &tenant).await?;
    let repository = PostgresRelationshipRepository::new(pool);
    let original = relationship(&tenant, 1, 100, "assembly-a", "component-b", 0, Some(1_000));
    let correction = relationship(&tenant, 2, 200, "assembly-a", "component-c", 0, Some(500));
    repository.insert_revision(&original).await?;
    repository.insert_revision(&correction).await?;

    assert_eq!(
        repository
            .find_for_asset(
                &tenant,
                &asset_id("component-b"),
                RelationshipSnapshotQuery::new(UnixTimeMillis::new(750), UnixTimeMillis::new(150),),
            )
            .await?,
        vec![original]
    );
    assert!(
        repository
            .find_for_asset(
                &tenant,
                &asset_id("component-b"),
                RelationshipSnapshotQuery::new(UnixTimeMillis::new(250), UnixTimeMillis::new(250),),
            )
            .await?
            .is_empty(),
        "the corrected latest revision no longer involves component-b"
    );
    assert_eq!(
        repository
            .find_for_asset(
                &tenant,
                &asset_id("component-c"),
                RelationshipSnapshotQuery::new(UnixTimeMillis::new(250), UnixTimeMillis::new(250),),
            )
            .await?,
        vec![correction]
    );
    assert!(
        repository
            .find_for_asset(
                &tenant,
                &asset_id("component-c"),
                RelationshipSnapshotQuery::new(UnixTimeMillis::new(750), UnixTimeMillis::new(250),),
            )
            .await?
            .is_empty(),
        "the corrected relationship is no longer physically effective at 750"
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_appends_are_serialized_per_relationship(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant_graph(&pool, &tenant).await?;
    let repository = PostgresRelationshipRepository::new(pool);
    repository
        .insert_revision(&relationship(
            &tenant,
            1,
            1_000,
            "assembly-a",
            "component-b",
            0,
            None,
        ))
        .await?;
    let first_attempt = relationship(&tenant, 2, 2_000, "assembly-a", "component-b", 0, None);
    let second_attempt = first_attempt.clone();
    let first_repository = repository.clone();
    let second_repository = repository.clone();

    let (first_result, second_result) = tokio::join!(
        first_repository.insert_revision(&first_attempt),
        second_repository.insert_revision(&second_attempt)
    );
    let results = [first_result, second_result];

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(PostgresRepositoryError::UnexpectedRevision {
                    expected: 3,
                    actual: 2
                })
            ))
            .count(),
        1
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn stored_relationship_revisions_are_immutable(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant_graph(&pool, &tenant).await?;
    let repository = PostgresRelationshipRepository::new(pool.clone());
    repository
        .insert_revision(&relationship(
            &tenant,
            1,
            1_000,
            "assembly-a",
            "component-b",
            0,
            None,
        ))
        .await?;

    let error = sqlx::query(
        r"
        UPDATE metadata.asset_relationship_revisions
        SET effective_start_ms = 1
        WHERE tenant_id = $1 AND relationship_id = 'relationship-1'
        ",
    )
    .bind(tenant.as_str())
    .execute(&pool)
    .await
    .expect_err("stored relationship revisions must not be rewritten");

    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code),
        Some("55000".into())
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn database_rejects_a_non_contiguous_revision_history(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_tenant_graph(&pool, &tenant).await?;
    let mut transaction = pool.begin().await?;

    sqlx::query(
        r"
        INSERT INTO metadata.asset_relationships
            (tenant_id, relationship_id, latest_revision)
        VALUES ($1, 'relationship-gap', 2)
        ",
    )
    .bind(tenant.as_str())
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.asset_relationship_revisions
            (tenant_id, relationship_id, revision, recorded_at_ms,
             relationship_type_id, relationship_type_version,
             source_asset_id, target_asset_id,
             effective_start_ms, effective_end_ms)
        VALUES (
            $1, 'relationship-gap', 2, 2_000,
            'physical.contains', 1,
            'assembly-a', 'component-b',
            1_000, NULL
        )
        ",
    )
    .bind(tenant.as_str())
    .execute(&mut *transaction)
    .await?;

    let error = transaction
        .commit()
        .await
        .expect_err("deferred history validation must reject a missing first revision");
    let database_error = error
        .as_database_error()
        .expect("constraint failure should be a PostgreSQL database error");
    assert_eq!(database_error.code().as_deref(), Some("23514"));
    assert_eq!(
        database_error.constraint(),
        Some("ck_asset_relationship_revision_sequence")
    );
    Ok(())
}
