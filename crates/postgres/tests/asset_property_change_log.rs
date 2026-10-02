//! Integration coverage for durable semantic property interpretations.

use std::error::Error;

use fleetiq_domain::{
    AssetId, DefinitionVersion, DeviceEndpointId, EffectiveInterval, PropertyDefinitionId,
    PropertyDefinitionRef, PropertyValue, RecordRevision, RecordedRevision, SignalBinding,
    SignalBindingId, SignalSource, SignalTarget, SourceSignalId, TenantId, UnitId, UnixTimeMillis,
};
use fleetiq_history::{AssetPropertyChangeLog, PropertyChangeAppendOutcome};
use fleetiq_metadata::SignalBindingRepository;
use fleetiq_postgres::{
    PostgresAssetPropertyChangeLog, PostgresAssetPropertyChangeLogError,
    PostgresDefinitionRepository, PostgresSignalBindingRepository,
};
use fleetiq_semantics::{
    AssetPropertyChange, EventTimeBasis, ResolutionContext, ResolutionOutcome,
    SignalBindingResolver,
};
use fleetiq_telemetry::{
    AcceptedMessageId, DecodedMeasurement, DecodedObservation, DecodedObservationId,
    ObservationQuality, ObservationTimes, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion,
};
use sqlx::PgPool;

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn first_append_exact_retry_and_later_knowledge_keep_one_fact(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let original = setup(&pool).await?;
    let log = PostgresAssetPropertyChangeLog::new(pool.clone());
    let first = change(&pool, original.clone(), 150, EventTimeBasis::DeviceRecorded).await?;
    let later = change(&pool, original, 250, EventTimeBasis::DeviceRecorded).await?;

    assert_eq!(
        log.append(&first).await?,
        PropertyChangeAppendOutcome::Appended
    );
    assert_eq!(
        log.append(&first).await?,
        PropertyChangeAppendOutcome::AlreadyPresent
    );
    assert_eq!(
        log.append(&later).await?,
        PropertyChangeAppendOutcome::AlreadyPresent
    );
    let stored: (i64, i64) = sqlx::query_as(
        "SELECT count(*)::bigint, min(resolution_known_at_ms) FROM history.asset_property_changes",
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(stored, (1, 150));
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn correction_and_event_time_basis_create_distinct_interpretations(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let observation = setup(&pool).await?;
    let log = PostgresAssetPropertyChangeLog::new(pool.clone());
    let original = change(
        &pool,
        observation.clone(),
        150,
        EventTimeBasis::DeviceRecorded,
    )
    .await?;
    let receipt = change(&pool, observation.clone(), 150, EventTimeBasis::ReceivedAt).await?;
    assert_eq!(
        log.append(&original).await?,
        PropertyChangeAppendOutcome::Appended
    );
    assert_eq!(
        log.append(&receipt).await?,
        PropertyChangeAppendOutcome::Appended
    );

    PostgresSignalBindingRepository::new(pool.clone())
        .insert_revision(&binding(2, 200, "power-system-2")?)
        .await?;
    let corrected = change(&pool, observation, 250, EventTimeBasis::DeviceRecorded).await?;
    assert_eq!(corrected.target().asset_id().as_str(), "power-system-2");
    assert_eq!(
        log.append(&corrected).await?,
        PropertyChangeAppendOutcome::Appended
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM history.asset_property_changes")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 3);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn forged_value_and_missing_durable_observation_are_rejected(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let original = setup(&pool).await?;
    let log = PostgresAssetPropertyChangeLog::new(pool.clone());
    let forged = observation(original.id().clone(), 99)?;
    let candidate = change(&pool, forged, 150, EventTimeBasis::DeviceRecorded).await?;
    assert!(matches!(
        log.append(&candidate).await,
        Err(PostgresAssetPropertyChangeLogError::SourceMismatch)
    ));
    let missing = observation(
        DecodedObservationId::new(AcceptedMessageId::new("missing-message")?, package()?, 0),
        48,
    )?;
    let candidate = change(&pool, missing, 150, EventTimeBasis::DeviceRecorded).await?;
    assert!(matches!(
        log.append(&candidate).await,
        Err(PostgresAssetPropertyChangeLogError::SourceMismatch)
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM history.asset_property_changes")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 0);
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn concurrent_exact_appends_have_one_winner(pool: PgPool) -> Result<(), Box<dyn Error>> {
    let observation = setup(&pool).await?;
    let candidate = change(&pool, observation, 150, EventTimeBasis::DeviceRecorded).await?;
    let log = PostgresAssetPropertyChangeLog::new(pool);
    let (first, second) = tokio::join!(log.append(&candidate), log.append(&candidate));
    let outcomes = [first?, second?];
    assert_eq!(
        outcomes
            .iter()
            .filter(|x| **x == PropertyChangeAppendOutcome::Appended)
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|x| **x == PropertyChangeAppendOutcome::AlreadyPresent)
            .count(),
        1
    );
    Ok(())
}

#[sqlx::test(migrator = "fleetiq_postgres::MIGRATOR")]
async fn direct_sql_cannot_forge_lineage_or_mutate_history(
    pool: PgPool,
) -> Result<(), Box<dyn Error>> {
    let observation = setup(&pool).await?;
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-b')")
        .execute(&pool)
        .await?;
    let cross_tenant = sqlx::query(
        r"
        INSERT INTO history.asset_property_changes
            (tenant_id, message_id, protocol_package_id, protocol_package_version,
             output_index, binding_id, binding_revision, event_time_basis,
             event_at_ms, resolution_effective_at_ms, resolution_known_at_ms)
        VALUES ('tenant-b', 'message-1', 'vendor.protocol', '1', 0,
                'binding-1', 1, 'device_recorded', 1000, 1000, 150)
        ",
    )
    .execute(&pool)
    .await
    .expect_err("another tenant cannot reference the source fact");
    assert!(matches!(
        cross_tenant
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("23503" | "23514")
    ));
    let bad = sqlx::query(
        r"
        INSERT INTO history.asset_property_changes
            (tenant_id, message_id, protocol_package_id, protocol_package_version,
             output_index, binding_id, binding_revision, event_time_basis,
             event_at_ms, resolution_effective_at_ms, resolution_known_at_ms)
        VALUES ('tenant-a', 'message-1', 'vendor.protocol', '1', 0,
                'binding-1', 1, 'device_recorded', 999, 100, 150)
        ",
    )
    .execute(&pool)
    .await
    .expect_err("invented event time must fail the lineage trigger");
    assert_eq!(
        bad.as_database_error()
            .and_then(sqlx::error::DatabaseError::constraint),
        Some("ck_asset_property_change_lineage")
    );

    let candidate = change(&pool, observation, 150, EventTimeBasis::DeviceRecorded).await?;
    PostgresAssetPropertyChangeLog::new(pool.clone())
        .append(&candidate)
        .await?;
    let update = sqlx::query("UPDATE history.asset_property_changes SET event_at_ms = 999")
        .execute(&pool)
        .await
        .expect_err("history must be immutable");
    assert_eq!(
        update
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("55000")
    );
    let deletion = sqlx::query("DELETE FROM history.asset_property_changes")
        .execute(&pool)
        .await
        .expect_err("history must not be deleted row by row");
    assert_eq!(
        deletion
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("55000")
    );
    Ok(())
}

async fn setup(pool: &PgPool) -> Result<DecodedObservation, Box<dyn Error>> {
    sqlx::query("INSERT INTO metadata.tenants (tenant_id) VALUES ('tenant-a')")
        .execute(pool)
        .await?;
    let mut definitions = pool.begin().await?;
    sqlx::query(
        r"INSERT INTO metadata.property_definitions
           (tenant_id, property_id, version, display_name, value_kind, canonical_unit)
           VALUES ('tenant-a', 'electrical.voltage', 1, 'Voltage', 'integer', 'volt')",
    )
    .execute(&mut *definitions)
    .await?;
    sqlx::query(
        r"INSERT INTO metadata.asset_type_definitions
           (tenant_id, type_id, version, display_name, property_count)
           VALUES ('tenant-a', 'communication.gateway', 1, 'Gateway', 0),
                  ('tenant-a', 'generic.power-system', 1, 'Power system', 1)",
    )
    .execute(&mut *definitions)
    .await?;
    sqlx::query(
        r"INSERT INTO metadata.asset_type_properties
           (tenant_id, type_id, type_version, property_id, property_version, property_ordinal)
           VALUES ('tenant-a', 'generic.power-system', 1, 'electrical.voltage', 1, 1)",
    )
    .execute(&mut *definitions)
    .await?;
    definitions.commit().await?;
    sqlx::query(
        r"INSERT INTO metadata.assets
           (tenant_id, asset_id, type_id, type_version, display_name)
           VALUES ('tenant-a', 'gateway-1', 'communication.gateway', 1, 'Gateway'),
                  ('tenant-a', 'power-system-1', 'generic.power-system', 1, 'Power system one'),
                  ('tenant-a', 'power-system-2', 'generic.power-system', 1, 'Power system two')",
    )
    .execute(pool)
    .await?;
    sqlx::query("INSERT INTO metadata.communication_devices (tenant_id, device_asset_id) VALUES ('tenant-a', 'gateway-1')")
        .execute(pool).await?;
    sqlx::query(
        r"INSERT INTO metadata.device_endpoints
           (tenant_id, device_asset_id, endpoint_id, endpoint_kind_id, display_name, endpoint_ordinal)
           VALUES ('tenant-a', 'gateway-1', 'can-primary', 'can', 'Primary CAN', 1)",
    )
    .execute(pool).await?;
    PostgresSignalBindingRepository::new(pool.clone())
        .insert_revision(&binding(1, 100, "power-system-1")?)
        .await?;
    sqlx::query(
        r"INSERT INTO ingestion.accepted_messages
           (tenant_id, message_id, device_asset_id, protocol_package_id,
            protocol_package_version, received_at_ms, payload)
           VALUES ('tenant-a', 'message-1', 'gateway-1', 'vendor.protocol', '1',
                   1100, '\x01'::bytea)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r"INSERT INTO ingestion.decoded_message_results
           (tenant_id, message_id, protocol_package_id, protocol_package_version, observation_count)
           VALUES ('tenant-a', 'message-1', 'vendor.protocol', '1', 1)",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r"INSERT INTO ingestion.decoded_observations
           (tenant_id, message_id, protocol_package_id, protocol_package_version,
            output_index, endpoint_id, signal_id, device_recorded_at_ms,
            value_kind, value_integer, source_unit, quality)
           VALUES ('tenant-a', 'message-1', 'vendor.protocol', '1', 0,
                   'can-primary', 'bus.voltage', 1000, 'integer', 48, 'volt', 'good')",
    )
    .execute(pool)
    .await?;
    observation(
        DecodedObservationId::new(AcceptedMessageId::new("message-1")?, package()?, 0),
        48,
    )
}

fn binding(revision: u64, recorded_at: i64, target: &str) -> Result<SignalBinding, Box<dyn Error>> {
    Ok(SignalBinding::new(
        TenantId::new("tenant-a")?,
        SignalBindingId::new("binding-1")?,
        RecordedRevision::new(
            RecordRevision::new(revision)?,
            UnixTimeMillis::new(recorded_at),
        ),
        SignalSource::new(
            AssetId::new("gateway-1")?,
            DeviceEndpointId::new("can-primary")?,
            SourceSignalId::new("bus.voltage")?,
        ),
        SignalTarget::new(
            AssetId::new(target)?,
            PropertyDefinitionRef::new(
                PropertyDefinitionId::new("electrical.voltage")?,
                DefinitionVersion::new(1)?,
            ),
        ),
        EffectiveInterval::new(UnixTimeMillis::new(0), None)?,
    ))
}

fn package() -> Result<ProtocolPackageRef, Box<dyn Error>> {
    Ok(ProtocolPackageRef::new(
        ProtocolPackageId::new("vendor.protocol")?,
        ProtocolPackageVersion::new("1")?,
    ))
}

fn observation(id: DecodedObservationId, value: i64) -> Result<DecodedObservation, Box<dyn Error>> {
    Ok(DecodedObservation::new(
        id,
        TenantId::new("tenant-a")?,
        SignalSource::new(
            AssetId::new("gateway-1")?,
            DeviceEndpointId::new("can-primary")?,
            SourceSignalId::new("bus.voltage")?,
        ),
        DecodedMeasurement::new(
            PropertyValue::Integer(value),
            Some(UnitId::new("volt")?),
            ObservationQuality::Good,
        )?,
        ObservationTimes::new(Some(UnixTimeMillis::new(1000)), UnixTimeMillis::new(1100)),
    ))
}

async fn change(
    pool: &PgPool,
    observation: DecodedObservation,
    known_at: i64,
    basis: EventTimeBasis,
) -> Result<AssetPropertyChange, Box<dyn Error>> {
    let resolver = SignalBindingResolver::new(
        PostgresSignalBindingRepository::new(pool.clone()),
        PostgresDefinitionRepository::new(pool.clone()),
    );
    let outcome = resolver
        .resolve(
            observation,
            ResolutionContext::new(UnixTimeMillis::new(1000), UnixTimeMillis::new(known_at)),
        )
        .await?;
    let ResolutionOutcome::Bound(bound) = outcome else {
        return Err("test observation should resolve".into());
    };
    Ok(AssetPropertyChange::new(bound, basis)?)
}
