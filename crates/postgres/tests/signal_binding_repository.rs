//! Integration tests for PostgreSQL signal-binding persistence and resolution.

use std::error::Error;

use fleetiq_application::{SignalBindingRepository, SignalBindingSnapshotQuery};
use fleetiq_domain::{
    AssetId, DefinitionVersion, DeviceEndpointId, EffectiveInterval, PropertyDefinitionId,
    PropertyDefinitionRef, RecordRevision, RecordedRevision, SignalBinding, SignalBindingId,
    SignalSource, SignalTarget, SourceSignalId, TenantId, UnixTimeMillis,
};
use fleetiq_postgres::{PostgresRepositoryError, PostgresSignalBindingRepository};
use sqlx::PgPool;

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).expect("test tenant ID should be valid")
}

fn asset_id(value: &str) -> AssetId {
    AssetId::new(value).expect("test asset ID should be valid")
}

fn revision(value: u64) -> RecordRevision {
    RecordRevision::new(value).expect("test record revision should be valid")
}

fn source(device: &str, endpoint: &str, signal: &str) -> SignalSource {
    SignalSource::new(
        asset_id(device),
        DeviceEndpointId::new(endpoint).expect("test endpoint ID should be valid"),
        SourceSignalId::new(signal).expect("test source signal ID should be valid"),
    )
}

#[allow(clippy::too_many_arguments)]
fn binding(
    tenant: &TenantId,
    revision_number: u64,
    recorded_at_ms: i64,
    endpoint: &str,
    signal: &str,
    target_asset: &str,
    target_property: &str,
    effective_start_ms: i64,
    effective_end_ms: Option<i64>,
) -> SignalBinding {
    SignalBinding::new(
        tenant.clone(),
        SignalBindingId::new("binding-1").expect("test binding ID should be valid"),
        RecordedRevision::new(
            revision(revision_number),
            UnixTimeMillis::new(recorded_at_ms),
        ),
        source("gateway-1", endpoint, signal),
        SignalTarget::new(
            asset_id(target_asset),
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new(target_property)
                    .expect("test property ID should be valid"),
                DefinitionVersion::new(1).expect("test definition version should be valid"),
            ),
        ),
        EffectiveInterval::new(
            UnixTimeMillis::new(effective_start_ms),
            effective_end_ms.map(UnixTimeMillis::new),
        )
        .expect("test effective interval should be valid"),
    )
}

/// Seeds a small protocol-neutral topology directly because these tests exercise
/// the binding repository rather than the already-covered aggregate adapters.
async fn seed_topology(pool: &PgPool, tenant: &TenantId) -> Result<(), Box<dyn Error>> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
        .bind(tenant.as_str())
        .execute(pool)
        .await?;
    let mut definition_transaction = pool.begin().await?;
    sqlx::query(
        r"
        INSERT INTO metadata.property_definitions
            (tenant_id, property_id, version, display_name, value_kind, canonical_unit)
        VALUES
            ($1, 'electrical.voltage', 1, 'Electrical voltage', 'decimal', 'volt'),
            ($1, 'thermal.temperature', 1, 'Temperature', 'decimal', 'celsius')
        ",
    )
    .bind(tenant.as_str())
    .execute(&mut *definition_transaction)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.asset_type_definitions
            (tenant_id, type_id, version, display_name, property_count)
        VALUES
            ($1, 'communication.gateway', 1, 'Communication gateway', 0),
            ($1, 'generic.power-system', 1, 'Power system', 1)
        ",
    )
    .bind(tenant.as_str())
    .execute(&mut *definition_transaction)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.asset_type_properties
            (tenant_id, type_id, type_version, property_id, property_version,
             property_ordinal)
        VALUES ($1, 'generic.power-system', 1, 'electrical.voltage', 1, 1)
        ",
    )
    .bind(tenant.as_str())
    .execute(&mut *definition_transaction)
    .await?;
    definition_transaction.commit().await?;
    sqlx::query(
        r"
        INSERT INTO metadata.assets
            (tenant_id, asset_id, type_id, type_version, display_name)
        VALUES
            ($1, 'gateway-1', 'communication.gateway', 1, 'Gateway'),
            ($1, 'power-system-1', 'generic.power-system', 1, 'Power system one'),
            ($1, 'power-system-2', 'generic.power-system', 1, 'Power system two')
        ",
    )
    .bind(tenant.as_str())
    .execute(pool)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.communication_devices (tenant_id, device_asset_id)
        VALUES ($1, 'gateway-1')
        ",
    )
    .bind(tenant.as_str())
    .execute(pool)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.device_endpoints
            (tenant_id, device_asset_id, endpoint_id, endpoint_kind_id,
             display_name, endpoint_ordinal)
        VALUES
            ($1, 'gateway-1', 'can-primary', 'can', 'Primary CAN', 1),
            ($1, 'gateway-1', 'can-secondary', 'can', 'Secondary CAN', 2)
        ",
    )
    .bind(tenant.as_str())
    .execute(pool)
    .await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn binding_revision_round_trip_preserves_domain_values(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_topology(&pool, &tenant).await?;
    let repository = PostgresSignalBindingRepository::new(pool);
    let expected = binding(
        &tenant,
        1,
        2_000,
        "can-primary",
        "bus.voltage",
        "power-system-1",
        "electrical.voltage",
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
async fn revisions_are_contiguous_with_monotonic_recorded_time(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_topology(&pool, &tenant).await?;
    let repository = PostgresSignalBindingRepository::new(pool);

    let second = binding(
        &tenant,
        2,
        2_000,
        "can-primary",
        "bus.voltage",
        "power-system-1",
        "electrical.voltage",
        0,
        None,
    );
    assert!(matches!(
        repository.insert_revision(&second).await,
        Err(PostgresRepositoryError::UnexpectedRevision {
            expected: 1,
            actual: 2
        })
    ));

    let first = binding(
        &tenant,
        1,
        2_000,
        "can-primary",
        "bus.voltage",
        "power-system-1",
        "electrical.voltage",
        0,
        None,
    );
    repository.insert_revision(&first).await?;
    let third = binding(
        &tenant,
        3,
        3_000,
        "can-primary",
        "bus.voltage",
        "power-system-1",
        "electrical.voltage",
        0,
        None,
    );
    assert!(matches!(
        repository.insert_revision(&third).await,
        Err(PostgresRepositoryError::UnexpectedRevision {
            expected: 2,
            actual: 3
        })
    ));
    let regressed = binding(
        &tenant,
        2,
        1_999,
        "can-primary",
        "bus.voltage",
        "power-system-1",
        "electrical.voltage",
        0,
        None,
    );
    assert!(matches!(
        repository.insert_revision(&regressed).await,
        Err(PostgresRepositoryError::RecordedTimeRegression {
            previous_ms: 2_000,
            actual_ms: 1_999
        })
    ));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn source_endpoint_and_target_property_are_validated(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_topology(&pool, &tenant).await?;
    let repository = PostgresSignalBindingRepository::new(pool.clone());

    let missing_endpoint = binding(
        &tenant,
        1,
        100,
        "missing",
        "bus.voltage",
        "power-system-1",
        "electrical.voltage",
        0,
        None,
    );
    let error = repository
        .insert_revision(&missing_endpoint)
        .await
        .expect_err("endpoint must exist beneath this device and tenant");
    assert!(matches!(
        error,
        PostgresRepositoryError::InvalidReference { constraint: Some(constraint), .. }
            if constraint == "fk_signal_binding_revisions_source_endpoint"
    ));

    let unsupported_property = binding(
        &tenant,
        1,
        100,
        "can-primary",
        "bus.temperature",
        "power-system-1",
        "thermal.temperature",
        0,
        None,
    );
    let error = repository
        .insert_revision(&unsupported_property)
        .await
        .expect_err("target asset type must support the exact property version");
    assert!(matches!(
        error,
        PostgresRepositoryError::InvalidReference { constraint: Some(constraint), .. }
            if constraint == "fk_signal_binding_target_property_support"
    ));

    let identity_count: i64 = sqlx::query_scalar("SELECT count(*) FROM metadata.signal_bindings")
        .fetch_one(&pool)
        .await?;
    assert_eq!(
        identity_count, 0,
        "failed appends must not leak identity heads"
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn source_snapshot_respects_recorded_and_effective_time(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_topology(&pool, &tenant).await?;
    let repository = PostgresSignalBindingRepository::new(pool);
    let original = binding(
        &tenant,
        1,
        100,
        "can-primary",
        "bus.voltage",
        "power-system-1",
        "electrical.voltage",
        0,
        Some(1_000),
    );
    let correction = binding(
        &tenant,
        2,
        200,
        "can-secondary",
        "bus.voltage",
        "power-system-2",
        "electrical.voltage",
        0,
        Some(500),
    );
    repository.insert_revision(&original).await?;
    repository.insert_revision(&correction).await?;

    assert_eq!(
        repository
            .find_for_source(
                &tenant,
                &source("gateway-1", "can-primary", "bus.voltage"),
                SignalBindingSnapshotQuery::new(UnixTimeMillis::new(750), UnixTimeMillis::new(150)),
            )
            .await?,
        vec![original]
    );
    assert!(
        repository
            .find_for_source(
                &tenant,
                &source("gateway-1", "can-primary", "bus.voltage"),
                SignalBindingSnapshotQuery::new(UnixTimeMillis::new(250), UnixTimeMillis::new(250)),
            )
            .await?
            .is_empty(),
        "the corrected revision no longer uses the original source"
    );
    assert_eq!(
        repository
            .find_for_source(
                &tenant,
                &source("gateway-1", "can-secondary", "bus.voltage"),
                SignalBindingSnapshotQuery::new(UnixTimeMillis::new(250), UnixTimeMillis::new(250)),
            )
            .await?,
        vec![correction.clone()]
    );
    assert!(
        repository
            .find_for_source(
                &tenant,
                correction.source(),
                SignalBindingSnapshotQuery::new(UnixTimeMillis::new(750), UnixTimeMillis::new(250)),
            )
            .await?
            .is_empty(),
        "the corrected binding has ended by effective time 750"
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_appends_are_serialized_per_binding(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_topology(&pool, &tenant).await?;
    let repository = PostgresSignalBindingRepository::new(pool);
    repository
        .insert_revision(&binding(
            &tenant,
            1,
            1_000,
            "can-primary",
            "bus.voltage",
            "power-system-1",
            "electrical.voltage",
            0,
            None,
        ))
        .await?;
    let attempt = binding(
        &tenant,
        2,
        2_000,
        "can-primary",
        "bus.voltage",
        "power-system-1",
        "electrical.voltage",
        0,
        None,
    );
    let other_attempt = attempt.clone();
    let first_repository = repository.clone();
    let second_repository = repository.clone();

    let (first_result, second_result) = tokio::join!(
        first_repository.insert_revision(&attempt),
        second_repository.insert_revision(&other_attempt)
    );
    let results = [first_result, second_result];
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "concurrent append results: {results:?}"
    );
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
        1,
        "concurrent append results: {results:?}"
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn stored_binding_revisions_are_immutable(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_topology(&pool, &tenant).await?;
    let repository = PostgresSignalBindingRepository::new(pool.clone());
    repository
        .insert_revision(&binding(
            &tenant,
            1,
            1_000,
            "can-primary",
            "bus.voltage",
            "power-system-1",
            "electrical.voltage",
            0,
            None,
        ))
        .await?;

    let error = sqlx::query("UPDATE metadata.signal_binding_revisions SET effective_start_ms = 1")
        .execute(&pool)
        .await
        .expect_err("recorded binding revisions must not be rewritten");
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code),
        Some("55000".into())
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn database_rejects_a_non_contiguous_binding_history(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let tenant = tenant_id("tenant-a");
    seed_topology(&pool, &tenant).await?;
    let mut transaction = pool.begin().await?;

    sqlx::query(
        r"
        INSERT INTO metadata.signal_bindings
            (tenant_id, binding_id, latest_revision)
        VALUES ($1, 'binding-gap', 2)
        ",
    )
    .bind(tenant.as_str())
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.signal_binding_revisions
            (tenant_id, binding_id, revision, recorded_at_ms,
             source_device_asset_id, source_endpoint_id, source_signal_id,
             target_asset_id, target_property_id, target_property_version,
             effective_start_ms, effective_end_ms)
        VALUES (
            $1, 'binding-gap', 2, 2_000,
            'gateway-1', 'can-primary', 'bus.voltage',
            'power-system-1', 'electrical.voltage', 1,
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
        .expect_err("deferred validation must reject a missing first revision");
    let database_error = error
        .as_database_error()
        .expect("constraint failure should be a PostgreSQL database error");
    assert_eq!(database_error.code().as_deref(), Some("23514"));
    assert_eq!(
        database_error.constraint(),
        Some("ck_signal_binding_revision_sequence")
    );
    Ok(())
}
