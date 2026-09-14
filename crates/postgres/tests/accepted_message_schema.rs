//! Integration tests for accepted-message storage invariants.

use std::error::Error;

use sqlx::{PgPool, Row};

async fn seed_communication_device(
    pool: &PgPool,
    tenant_id: &str,
    device_asset_id: &str,
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
        VALUES ($1, $2, 'communication.gateway', 1, 'Test gateway')
        ",
    )
    .bind(tenant_id)
    .bind(device_asset_id)
    .execute(pool)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.communication_devices (tenant_id, device_asset_id)
        VALUES ($1, $2)
        ",
    )
    .bind(tenant_id)
    .bind(device_asset_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_message(
    pool: &PgPool,
    tenant_id: &str,
    message_id: &str,
    device_asset_id: &str,
    payload: &[u8],
) -> sqlx::Result<()> {
    sqlx::query(
        r"
        INSERT INTO ingestion.accepted_messages
            (tenant_id, message_id, device_asset_id, protocol_package_id,
             protocol_package_version, received_at_ms, payload)
        VALUES ($1, $2, $3, 'vendor.protocol', '2026.09.1', 1000, $4)
        ",
    )
    .bind(tenant_id)
    .bind(message_id)
    .bind(device_asset_id)
    .bind(payload)
    .execute(pool)
    .await?;
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn accepted_message_preserves_exact_replay_fields(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_communication_device(&pool, "tenant-a", "device-a").await?;
    insert_message(&pool, "tenant-a", "message-1", "device-a", b"\0wire\xff").await?;

    let row = sqlx::query(
        r"
        SELECT tenant_id, message_id, device_asset_id, protocol_package_id,
               protocol_package_version, received_at_ms, payload
        FROM ingestion.accepted_messages
        WHERE tenant_id = 'tenant-a' AND message_id = 'message-1'
        ",
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(row.try_get::<String, _>("tenant_id")?, "tenant-a");
    assert_eq!(row.try_get::<String, _>("message_id")?, "message-1");
    assert_eq!(row.try_get::<String, _>("device_asset_id")?, "device-a");
    assert_eq!(
        row.try_get::<String, _>("protocol_package_id")?,
        "vendor.protocol"
    );
    assert_eq!(
        row.try_get::<String, _>("protocol_package_version")?,
        "2026.09.1"
    );
    assert_eq!(row.try_get::<i64, _>("received_at_ms")?, 1_000);
    assert_eq!(row.try_get::<Vec<u8>, _>("payload")?, b"\0wire\xff");
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn message_identity_is_tenant_scoped(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_communication_device(&pool, "tenant-a", "device-a").await?;
    seed_communication_device(&pool, "tenant-b", "device-b").await?;

    insert_message(&pool, "tenant-a", "shared-message", "device-a", b"a").await?;
    insert_message(&pool, "tenant-b", "shared-message", "device-b", b"b").await?;

    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ingestion.accepted_messages WHERE message_id = 'shared-message'",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(count, 2);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn message_requires_a_device_in_the_same_tenant(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_communication_device(&pool, "tenant-b", "device-a").await?;
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a')")
        .execute(&pool)
        .await?;

    let error = insert_message(&pool, "tenant-a", "message-1", "device-a", b"wire")
        .await
        .expect_err("another tenant's communication device must not be referenced");

    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some("fk_accepted_messages_device")
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn duplicate_identity_cannot_create_another_row(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_communication_device(&pool, "tenant-a", "device-a").await?;
    insert_message(&pool, "tenant-a", "message-1", "device-a", b"first").await?;

    let error = insert_message(&pool, "tenant-a", "message-1", "device-a", b"second")
        .await
        .expect_err("one tenant-scoped message identity must name only one row");

    assert_eq!(
        error
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some("pk_accepted_messages")
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn accepted_messages_cannot_be_updated_or_deleted(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_communication_device(&pool, "tenant-a", "device-a").await?;
    insert_message(&pool, "tenant-a", "message-1", "device-a", b"wire").await?;

    for statement in [
        "UPDATE ingestion.accepted_messages SET payload = 'changed' WHERE tenant_id = 'tenant-a'",
        "DELETE FROM ingestion.accepted_messages WHERE tenant_id = 'tenant-a'",
    ] {
        let error = sqlx::query(statement)
            .execute(&pool)
            .await
            .expect_err("accepted-message evidence must be append-only");
        assert_eq!(
            error
                .as_database_error()
                .and_then(sqlx::error::DatabaseError::code),
            Some("55000".into())
        );
    }

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ingestion.accepted_messages")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 1);
    Ok(())
}
