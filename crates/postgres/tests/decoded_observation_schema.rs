//! Integration tests for decoded-message result storage invariants.

use std::error::Error;

use sqlx::{PgPool, Row};

async fn seed_accepted_message(
    pool: &PgPool,
    tenant_id: &str,
    message_id: &str,
) -> sqlx::Result<()> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ($1)")
        .bind(tenant_id)
        .execute(pool)
        .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.asset_type_definitions
            (tenant_id, type_id, version, display_name, property_count)
        VALUES ($1, 'communication.gateway', 1, 'Communication gateway', 0)
        ",
    )
    .bind(tenant_id)
    .execute(pool)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.assets
            (tenant_id, asset_id, type_id, type_version, display_name)
        VALUES ($1, 'device-a', 'communication.gateway', 1, 'Test gateway')
        ",
    )
    .bind(tenant_id)
    .execute(pool)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.communication_devices (tenant_id, device_asset_id)
        VALUES ($1, 'device-a')
        ",
    )
    .bind(tenant_id)
    .execute(pool)
    .await?;
    sqlx::query(
        r"
        INSERT INTO ingestion.accepted_messages
            (tenant_id, message_id, device_asset_id, protocol_package_id,
             protocol_package_version, received_at_ms, payload)
        VALUES ($1, $2, 'device-a', 'vendor.protocol', '1', 1000, 'wire')
        ",
    )
    .bind(tenant_id)
    .bind(message_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_result(pool: &PgPool, observation_count: i64) -> sqlx::Result<()> {
    sqlx::query(
        r"
        INSERT INTO ingestion.decoded_message_results
            (tenant_id, message_id, protocol_package_id,
             protocol_package_version, observation_count)
        VALUES ('tenant-a', 'message-1', 'vendor.protocol', '1', $1)
        ",
    )
    .bind(observation_count)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_text_observation<'e, E>(executor: E, output_index: i64) -> sqlx::Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(
        r"
        INSERT INTO ingestion.decoded_observations
            (tenant_id, message_id, protocol_package_id,
             protocol_package_version, output_index, endpoint_id, signal_id,
             device_recorded_at_ms, value_kind, value_text, source_unit, quality)
        VALUES
            ('tenant-a', 'message-1', 'vendor.protocol', '1', $1,
             'can-1', 'freezer.temperature', NULL, 'text', 'cold', NULL, 'good')
        ",
    )
    .bind(output_index)
    .execute(executor)
    .await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn empty_result_is_explicit_completion_evidence(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_accepted_message(&pool, "tenant-a", "message-1").await?;
    insert_result(&pool, 0).await?;

    let count: i64 =
        sqlx::query_scalar("SELECT observation_count FROM ingestion.decoded_message_results")
            .fetch_one(&pool)
            .await?;
    let observations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ingestion.decoded_observations")
            .fetch_one(&pool)
            .await?;

    assert_eq!(count, 0);
    assert_eq!(observations, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn observations_preserve_ordered_typed_decoder_output(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_accepted_message(&pool, "tenant-a", "message-1").await?;
    insert_result(&pool, 4).await?;

    sqlx::query(
        r"
        INSERT INTO ingestion.decoded_observations
            (tenant_id, message_id, protocol_package_id,
             protocol_package_version, output_index, endpoint_id, signal_id,
             device_recorded_at_ms, value_kind, value_boolean, value_integer,
             value_decimal, value_text, source_unit, quality)
        VALUES
            ('tenant-a', 'message-1', 'vendor.protocol', '1', 0,
             'internal', 'door.open', NULL, 'boolean', TRUE, NULL, NULL, NULL,
             NULL, 'good'),
            ('tenant-a', 'message-1', 'vendor.protocol', '1', 1,
             'can-1', 'engine.speed', 900, 'integer', NULL, 1200, NULL, NULL,
             'rpm', 'uncertain'),
            ('tenant-a', 'message-1', 'vendor.protocol', '1', 2,
             'can-1', 'freezer.temperature', 950, 'decimal', NULL, NULL,
             2.50, NULL, 'deg_c', 'invalid'),
            ('tenant-a', 'message-1', 'vendor.protocol', '1', 3,
             'serial-1', 'status', NULL, 'text', NULL, NULL, NULL, 'ready',
             NULL, 'good')
        ",
    )
    .execute(&pool)
    .await?;

    let rows = sqlx::query(
        r"
        SELECT output_index, endpoint_id, signal_id, device_recorded_at_ms,
               value_kind, value_boolean, value_integer, value_decimal::TEXT,
               value_text, source_unit, quality
        FROM ingestion.decoded_observations
        ORDER BY output_index
        ",
    )
    .fetch_all(&pool)
    .await?;

    assert_eq!(rows.len(), 4);
    assert!(rows[0].try_get::<bool, _>("value_boolean")?);
    assert_eq!(rows[1].try_get::<i64, _>("value_integer")?, 1_200);
    assert_eq!(rows[2].try_get::<String, _>("value_decimal")?, "2.50");
    assert_eq!(rows[2].try_get::<String, _>("source_unit")?, "deg_c");
    assert_eq!(rows[2].try_get::<String, _>("quality")?, "invalid");
    assert_eq!(rows[3].try_get::<String, _>("value_text")?, "ready");
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn result_must_name_the_package_pinned_to_the_message(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_accepted_message(&pool, "tenant-a", "message-1").await?;

    let error = sqlx::query(
        r"
        INSERT INTO ingestion.decoded_message_results
            (tenant_id, message_id, protocol_package_id,
             protocol_package_version, observation_count)
        VALUES ('tenant-a', 'message-1', 'vendor.protocol', '2', 0)
        ",
    )
    .execute(&pool)
    .await
    .expect_err("result must use the accepted message's exact package release");

    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some("fk_decoded_message_results_message")
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn observation_requires_its_complete_result_marker(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_accepted_message(&pool, "tenant-a", "message-1").await?;

    let error = insert_text_observation(&pool, 0)
        .await
        .expect_err("observation must belong to a result marker");

    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some("fk_decoded_observations_result")
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn typed_value_unit_quality_and_position_are_constrained(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_accepted_message(&pool, "tenant-a", "message-1").await?;
    insert_result(&pool, 1).await?;

    for (statement, constraint) in [
        (
            "INSERT INTO ingestion.decoded_observations
             (tenant_id, message_id, protocol_package_id, protocol_package_version,
              output_index, endpoint_id, signal_id, value_kind,
              value_boolean, value_text, quality)
             VALUES ('tenant-a', 'message-1', 'vendor.protocol', '1', 0,
                     'can-1', 'signal-1', 'boolean', TRUE, 'also-set', 'good')",
            "ck_decoded_observations_typed_value",
        ),
        (
            "INSERT INTO ingestion.decoded_observations
             (tenant_id, message_id, protocol_package_id, protocol_package_version,
              output_index, endpoint_id, signal_id, value_kind,
              value_text, source_unit, quality)
             VALUES ('tenant-a', 'message-1', 'vendor.protocol', '1', 0,
                     'can-1', 'signal-1', 'text', 'ready', 'volts', 'good')",
            "ck_decoded_observations_unit",
        ),
        (
            "INSERT INTO ingestion.decoded_observations
             (tenant_id, message_id, protocol_package_id, protocol_package_version,
              output_index, endpoint_id, signal_id, value_kind, value_text, quality)
             VALUES ('tenant-a', 'message-1', 'vendor.protocol', '1', 0,
                     'can-1', 'signal-1', 'text', 'ready', 'unknown')",
            "ck_decoded_observations_quality",
        ),
        (
            "INSERT INTO ingestion.decoded_observations
             (tenant_id, message_id, protocol_package_id, protocol_package_version,
              output_index, endpoint_id, signal_id, value_kind, value_text, quality)
             VALUES ('tenant-a', 'message-1', 'vendor.protocol', '1', -1,
                     'can-1', 'signal-1', 'text', 'ready', 'good')",
            "ck_decoded_observations_output_index",
        ),
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("invalid decoded observation must be rejected");
        assert_eq!(
            error
                .as_database_error()
                .and_then(sqlx::error::DatabaseError::constraint),
            Some(constraint)
        );
    }
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn failed_batch_transaction_leaves_no_partial_result(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_accepted_message(&pool, "tenant-a", "message-1").await?;
    let mut transaction = pool.begin().await?;

    sqlx::query(
        r"
        INSERT INTO ingestion.decoded_message_results
            (tenant_id, message_id, protocol_package_id,
             protocol_package_version, observation_count)
        VALUES ('tenant-a', 'message-1', 'vendor.protocol', '1', 2)
        ",
    )
    .execute(&mut *transaction)
    .await?;
    insert_text_observation(&mut *transaction, 0).await?;
    let error = insert_text_observation(&mut *transaction, 0)
        .await
        .expect_err("duplicate output identity must abort the batch");
    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some("pk_decoded_observations")
    );
    transaction.rollback().await?;

    let results: i64 = sqlx::query_scalar("SELECT count(*) FROM ingestion.decoded_message_results")
        .fetch_one(&pool)
        .await?;
    let observations: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ingestion.decoded_observations")
            .fetch_one(&pool)
            .await?;
    assert_eq!((results, observations), (0, 0));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn decoded_results_and_observations_are_immutable(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_accepted_message(&pool, "tenant-a", "message-1").await?;
    insert_result(&pool, 1).await?;
    insert_text_observation(&pool, 0).await?;

    for statement in [
        "UPDATE ingestion.decoded_message_results SET observation_count = 2",
        "DELETE FROM ingestion.decoded_message_results",
        "UPDATE ingestion.decoded_observations SET value_text = 'changed'",
        "DELETE FROM ingestion.decoded_observations",
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("decoded ingestion evidence must be append-only");
        assert_eq!(
            error
                .as_database_error()
                .and_then(sqlx::error::DatabaseError::code),
            Some("55000".into())
        );
    }
    Ok(())
}
