use fleetiq_domain::{
    AssetId, DefinitionVersion, DeviceEndpointId, EffectiveInterval, PropertyDefinitionId,
    PropertyDefinitionRef, PropertyValue, RecordRevision, RecordedRevision, SignalBinding,
    SignalBindingId, SignalSource, SignalTarget, SourceSignalId, TenantId, UnitId, UnixTimeMillis,
};
use fleetiq_metadata::SignalBindingRepository;
use fleetiq_postgres::{PostgresDefinitionRepository, PostgresSignalBindingRepository};
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
use std::error::Error;
pub(super) async fn setup(
    pool: &PgPool,
    with_binding: bool,
) -> Result<DecodedObservation, Box<dyn Error>> {
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
    if with_binding {
        PostgresSignalBindingRepository::new(pool.clone())
            .insert_revision(&binding(1, 100, "power-system-1")?)
            .await?;
    }
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

pub(super) fn binding(
    revision: u64,
    recorded_at: i64,
    target: &str,
) -> Result<SignalBinding, Box<dyn Error>> {
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

pub(super) async fn change(
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
