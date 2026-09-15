//! Integration tests for the PostgreSQL decoded-observation log.

use std::{error::Error, str::FromStr};

use bigdecimal::BigDecimal;
use fleetiq_application::{
    AcceptedMessageLog, DecodedObservationAppendOutcome, DecodedObservationBatch,
    DecodedObservationLog, PendingAcceptedMessage,
};
use fleetiq_domain::{
    AssetId, DeviceEndpointId, PropertyValue, SignalSource, SourceSignalId, TenantId, UnitId,
    UnixTimeMillis,
};
use fleetiq_postgres::{
    PostgresAcceptedMessageLog, PostgresDecodedObservationLog, PostgresDecodedObservationLogError,
};
use fleetiq_telemetry::{
    AcceptedMessage, AcceptedMessageId, AcceptedPayload, DecodedMeasurement, DecodedObservation,
    ObservationQuality, ObservationTimes, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
use sqlx::PgPool;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn first_append_batches_all_typed_observations_and_exact_retry_succeeds(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let message = seed_accepted_message(&pool).await?;
    let batch = complete_batch(message)?;
    let log = PostgresDecodedObservationLog::new(pool.clone());

    assert_eq!(
        log.append(&batch).await?,
        DecodedObservationAppendOutcome::Appended
    );
    assert_eq!(
        log.append(&batch).await?,
        DecodedObservationAppendOutcome::AlreadyPresent
    );

    let marker_count: i64 =
        sqlx::query_scalar("SELECT observation_count FROM ingestion.decoded_message_results")
            .fetch_one(&pool)
            .await?;
    let observation_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ingestion.decoded_observations")
            .fetch_one(&pool)
            .await?;
    assert_eq!((marker_count, observation_count), (4, 4));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn empty_batch_is_durable_and_idempotent(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let message = seed_accepted_message(&pool).await?;
    let batch = DecodedObservationBatch::new(message, Vec::new())?;
    let log = PostgresDecodedObservationLog::new(pool.clone());

    assert!(log.append(&batch).await?.was_appended());
    assert!(!log.append(&batch).await?.was_appended());

    let counts = sqlx::query_as::<_, (i64, i64)>(
        r"
        SELECT result.observation_count, count(observation.output_index)
        FROM ingestion.decoded_message_results AS result
        LEFT JOIN ingestion.decoded_observations AS observation
          USING (tenant_id, message_id, protocol_package_id,
                 protocol_package_version)
        GROUP BY result.observation_count
        ",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(counts, (0, 0));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_exact_appends_have_one_winner(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let batch = complete_batch(seed_accepted_message(&pool).await?)?;
    let log = PostgresDecodedObservationLog::new(pool);

    let (first, second) = tokio::join!(log.append(&batch), log.append(&batch));
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
            .filter(|outcome| matches!(outcome, DecodedObservationAppendOutcome::AlreadyPresent))
            .count(),
        1
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn different_immutable_output_is_an_identity_collision(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let message = seed_accepted_message(&pool).await?;
    let original = single_text_batch(message.clone(), "ready", ObservationQuality::Good)?;
    let log = PostgresDecodedObservationLog::new(pool);
    log.append(&original).await?;

    for conflicting in [
        single_text_batch(message.clone(), "changed", ObservationQuality::Good)?,
        single_text_batch(message.clone(), "ready", ObservationQuality::Invalid)?,
        DecodedObservationBatch::new(message.clone(), Vec::new())?,
    ] {
        let error = log
            .append(&conflicting)
            .await
            .expect_err("different output must not be accepted as an exact retry");
        assert!(matches!(
            error,
            PostgresDecodedObservationLogError::IdentityCollision {
                ref tenant_id,
                ref message_id,
                ref protocol_package,
            } if tenant_id.as_str() == "tenant-a"
                && message_id.as_str() == "message-1"
                && protocol_package.version().as_str() == "1"
        ));
    }
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_different_appends_resolve_winner_and_collision(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let message = seed_accepted_message(&pool).await?;
    let first_batch = single_text_batch(message.clone(), "first", ObservationQuality::Good)?;
    let second_batch = single_text_batch(message, "second", ObservationQuality::Good)?;
    let log = PostgresDecodedObservationLog::new(pool);

    let outcomes = tokio::join!(log.append(&first_batch), log.append(&second_batch));
    let results = [outcomes.0, outcomes.1];

    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(DecodedObservationAppendOutcome::Appended)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(PostgresDecodedObservationLogError::IdentityCollision { .. })
            ))
            .count(),
        1
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn missing_or_different_accepted_message_is_rejected(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let log = PostgresDecodedObservationLog::new(pool.clone());
    let missing = DecodedObservationBatch::new(accepted_message(), Vec::new())?;

    let error = log
        .append(&missing)
        .await
        .expect_err("a decoded result requires its accepted message");
    assert!(matches!(
        error,
        PostgresDecodedObservationLogError::AcceptedMessageMismatch { .. }
    ));

    let stored = seed_accepted_message(&pool).await?;
    let different = AcceptedMessage::new(
        stored.id().clone(),
        stored.tenant_id().clone(),
        stored.device_asset_id().clone(),
        stored.protocol_package().clone(),
        UnixTimeMillis::new(2_000),
        AcceptedPayload::new(b"different-wire-frame".as_slice()),
    );
    let batch = DecodedObservationBatch::new(different, Vec::new())?;
    let error = log
        .append(&batch)
        .await
        .expect_err("different accepted-message content must be rejected");
    assert!(matches!(
        error,
        PostgresDecodedObservationLogError::AcceptedMessageMismatch { .. }
    ));

    let result_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ingestion.decoded_message_results")
            .fetch_one(&pool)
            .await?;
    assert_eq!(result_count, 0);
    Ok(())
}

async fn seed_accepted_message(pool: &PgPool) -> Result<AcceptedMessage, Box<dyn Error>> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a')")
        .execute(pool)
        .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.asset_type_definitions
            (tenant_id, type_id, version, display_name, property_count)
        VALUES ('tenant-a', 'communication.gateway', 1, 'Communication gateway', 0)
        ",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.assets
            (tenant_id, asset_id, type_id, type_version, display_name)
        VALUES
            ('tenant-a', 'device-a', 'communication.gateway', 1, 'Test gateway')
        ",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r"
        INSERT INTO metadata.communication_devices (tenant_id, device_asset_id)
        VALUES ('tenant-a', 'device-a')
        ",
    )
    .execute(pool)
    .await?;

    let pending = PendingAcceptedMessage::new(
        accepted_message().id().clone(),
        TenantId::new("tenant-a")?,
        AssetId::new("device-a")?,
        package(),
        UnixTimeMillis::new(1_000),
        AcceptedPayload::new(b"private-wire-frame".as_slice()),
    );
    Ok(PostgresAcceptedMessageLog::new(pool.clone())
        .append(&pending)
        .await?
        .into_message())
}

fn accepted_message() -> AcceptedMessage {
    AcceptedMessage::new(
        AcceptedMessageId::new("message-1").expect("valid message ID"),
        TenantId::new("tenant-a").expect("valid tenant ID"),
        AssetId::new("device-a").expect("valid asset ID"),
        package(),
        UnixTimeMillis::new(1_000),
        AcceptedPayload::new(b"private-wire-frame".as_slice()),
    )
}

fn complete_batch(message: AcceptedMessage) -> Result<DecodedObservationBatch, Box<dyn Error>> {
    let observations = vec![
        observation(
            &message,
            0,
            PropertyValue::Boolean(true),
            None,
            None,
            ObservationQuality::Good,
        )?,
        observation(
            &message,
            1,
            PropertyValue::Integer(1_200),
            Some("rpm"),
            Some(900),
            ObservationQuality::Uncertain,
        )?,
        observation(
            &message,
            2,
            PropertyValue::Decimal(BigDecimal::from_str("2.50")?),
            Some("deg_c"),
            Some(950),
            ObservationQuality::Invalid,
        )?,
        observation(
            &message,
            3,
            PropertyValue::Text("ready".to_owned()),
            None,
            None,
            ObservationQuality::Good,
        )?,
    ];
    Ok(DecodedObservationBatch::new(message, observations)?)
}

fn single_text_batch(
    message: AcceptedMessage,
    value: &str,
    quality: ObservationQuality,
) -> Result<DecodedObservationBatch, Box<dyn Error>> {
    let output = observation(
        &message,
        0,
        PropertyValue::Text(value.to_owned()),
        None,
        None,
        quality,
    )?;
    Ok(DecodedObservationBatch::new(message, vec![output])?)
}

fn observation(
    message: &AcceptedMessage,
    output_index: u32,
    value: PropertyValue,
    source_unit: Option<&str>,
    device_recorded_at_ms: Option<i64>,
    quality: ObservationQuality,
) -> Result<DecodedObservation, Box<dyn Error>> {
    Ok(DecodedObservation::new(
        message.decoded_observation_id(output_index),
        message.tenant_id().clone(),
        SignalSource::new(
            message.device_asset_id().clone(),
            DeviceEndpointId::new("can-1")?,
            SourceSignalId::new(format!("signal-{output_index}"))?,
        ),
        DecodedMeasurement::new(value, source_unit.map(UnitId::new).transpose()?, quality)?,
        ObservationTimes::new(
            device_recorded_at_ms.map(UnixTimeMillis::new),
            message.received_at(),
        ),
    ))
}

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef::new(
        ProtocolPackageId::new("vendor.protocol").expect("valid package ID"),
        ProtocolPackageVersion::new("1").expect("valid package version"),
    )
}
