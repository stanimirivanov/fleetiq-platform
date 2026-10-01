//! Integration tests for the PostgreSQL accepted-message log.

use std::error::Error;

use fleetiq_domain::{AssetId, TenantId, UnixTimeMillis};
use fleetiq_ingestion::{AcceptedMessageAppendOutcome, AcceptedMessageLog, PendingAcceptedMessage};
use fleetiq_postgres::{PostgresAcceptedMessageLog, PostgresAcceptedMessageLogError};
use fleetiq_telemetry::{
    AcceptedMessageId, AcceptedPayload, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
use sqlx::PgPool;

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

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn first_append_returns_the_durable_domain_message(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_communication_device(&pool, "tenant-a", "device-a").await?;
    let log = PostgresAcceptedMessageLog::new(pool);
    let candidate = pending_message(b"\0private\xff", "2026.09.1", 1_000);

    let outcome = log.append(&candidate).await?;

    assert!(outcome.was_appended());
    assert_eq!(
        outcome.message(),
        &candidate.clone().into_accepted_message()
    );
    assert_eq!(outcome.message().payload().as_bytes(), b"\0private\xff");
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn exact_retry_returns_the_existing_message(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_communication_device(&pool, "tenant-a", "device-a").await?;
    let log = PostgresAcceptedMessageLog::new(pool);
    let candidate = pending_message(b"wire", "2026.09.1", 1_000);

    assert!(matches!(
        log.append(&candidate).await?,
        AcceptedMessageAppendOutcome::Appended(_)
    ));
    let retry = log.append(&candidate).await?;

    assert!(matches!(
        retry,
        AcceptedMessageAppendOutcome::AlreadyPresent(ref message)
            if message == &candidate.clone().into_accepted_message()
    ));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_exact_appends_have_one_winner(pool: PgPool) -> Result<(), Box<dyn Error>> {
    seed_communication_device(&pool, "tenant-a", "device-a").await?;
    let log = PostgresAcceptedMessageLog::new(pool);
    let candidate = pending_message(b"wire", "2026.09.1", 1_000);

    let (first, second) = tokio::join!(log.append(&candidate), log.append(&candidate));
    let outcomes = [first?, second?];

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.was_appended())
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, AcceptedMessageAppendOutcome::AlreadyPresent(_)))
            .count(),
        1
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn reused_identity_with_different_content_is_a_collision(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    seed_communication_device(&pool, "tenant-a", "device-a").await?;
    let log = PostgresAcceptedMessageLog::new(pool);
    let original = pending_message(b"wire", "2026.09.1", 1_000);
    log.append(&original).await?;

    for conflicting in [
        pending_message(b"changed", "2026.09.1", 1_000),
        pending_message(b"wire", "2026.09.2", 1_000),
        pending_message(b"wire", "2026.09.1", 2_000),
    ] {
        let error = log
            .append(&conflicting)
            .await
            .expect_err("different immutable content must not be an exact retry");
        assert!(matches!(
            error,
            PostgresAcceptedMessageLogError::IdentityCollision {
                ref tenant_id,
                ref message_id,
            } if tenant_id.as_str() == "tenant-a" && message_id.as_str() == "message-1"
        ));
    }
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_admitted_device_is_an_invalid_reference(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a')")
        .execute(&pool)
        .await?;
    let log = PostgresAcceptedMessageLog::new(pool);
    let candidate = pending_message(b"wire", "2026.09.1", 1_000);

    let error = log
        .append(&candidate)
        .await
        .expect_err("accepted messages require an admitted communication device");

    assert!(matches!(
        error,
        PostgresAcceptedMessageLogError::Repository(
            fleetiq_postgres::PostgresRepositoryError::InvalidReference {
                constraint: Some(constraint),
                ..
            }
        ) if constraint == "fk_accepted_messages_device"
    ));
    Ok(())
}

fn pending_message(
    payload: &[u8],
    package_version: &str,
    received_at_ms: i64,
) -> PendingAcceptedMessage {
    PendingAcceptedMessage::new(
        AcceptedMessageId::new("message-1").unwrap(),
        TenantId::new("tenant-a").unwrap(),
        AssetId::new("device-a").unwrap(),
        ProtocolPackageRef::new(
            ProtocolPackageId::new("vendor.protocol").unwrap(),
            ProtocolPackageVersion::new(package_version).unwrap(),
        ),
        UnixTimeMillis::new(received_at_ms),
        AcceptedPayload::new(payload),
    )
}
