//! Exact tenant-scoped durable decoded-observation read for semantic processing.

use std::future::Future;

use bigdecimal::BigDecimal;
use fleetiq_domain::{
    AssetId, DeviceEndpointId, PropertyValue, SignalSource, SourceSignalId, TenantId, UnitId,
    UnixTimeMillis,
};
use fleetiq_history::DecodedObservationView;
use fleetiq_telemetry::{
    DecodedMeasurement, DecodedObservation, DecodedObservationId, ObservationQuality,
    ObservationTimes,
};
use sqlx::PgPool;

use crate::error::{PostgresRepositoryError, classify_database_error, corrupt};

/// Reads exact immutable decoded output without fetching the raw wire payload.
#[derive(Clone, Debug)]
pub struct PostgresDecodedObservationView {
    pool: PgPool,
}

impl PostgresDecodedObservationView {
    /// Uses a pool connected to the fully migrated `FleetIQ` database.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Selects one joined source row; an absent identity never falls back to
    /// another tenant, package release, or output index.
    async fn lookup(
        &self,
        tenant_id: &TenantId,
        id: &DecodedObservationId,
    ) -> Result<Option<DecodedObservation>, PostgresRepositoryError> {
        let package = id.decoded_by();
        let row = sqlx::query_as!(
            StoredObservationRow,
            r"
            SELECT message.device_asset_id, message.received_at_ms,
                   observation.endpoint_id, observation.signal_id,
                   observation.device_recorded_at_ms, observation.value_kind,
                   observation.value_boolean, observation.value_integer,
                   observation.value_decimal, observation.value_text,
                   observation.source_unit, observation.quality
            FROM ingestion.decoded_observations AS observation
            JOIN ingestion.accepted_messages AS message
              USING (tenant_id, message_id, protocol_package_id,
                     protocol_package_version)
            WHERE observation.tenant_id = $1
              AND observation.message_id = $2
              AND observation.protocol_package_id = $3
              AND observation.protocol_package_version = $4
              AND observation.output_index = $5
            ",
            tenant_id.as_str(),
            id.message_id().as_str(),
            package.id().as_str(),
            package.version().as_str(),
            i64::from(id.output_index()),
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(classify_database_error)?;
        row.map(|row| row.into_domain(tenant_id, id)).transpose()
    }
}

impl DecodedObservationView for PostgresDecodedObservationView {
    type Error = PostgresRepositoryError;

    fn find<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        id: &'a DecodedObservationId,
    ) -> impl Future<Output = Result<Option<DecodedObservation>, Self::Error>> + Send + 'a {
        self.lookup(tenant_id, id)
    }
}

/// Persistence representation joined from the observation and its parent
/// message. The selected columns deliberately omit the raw wire payload.
struct StoredObservationRow {
    device_asset_id: String,
    received_at_ms: i64,
    endpoint_id: String,
    signal_id: String,
    device_recorded_at_ms: Option<i64>,
    value_kind: String,
    value_boolean: Option<bool>,
    value_integer: Option<i64>,
    value_decimal: Option<BigDecimal>,
    value_text: Option<String>,
    source_unit: Option<String>,
    quality: String,
}

impl StoredObservationRow {
    fn into_domain(
        self,
        tenant_id: &TenantId,
        id: &DecodedObservationId,
    ) -> Result<DecodedObservation, PostgresRepositoryError> {
        let value = match (
            self.value_kind.as_str(),
            self.value_boolean,
            self.value_integer,
            self.value_decimal,
            self.value_text,
        ) {
            ("boolean", Some(value), None, None, None) => PropertyValue::Boolean(value),
            ("integer", None, Some(value), None, None) => PropertyValue::Integer(value),
            ("decimal", None, None, Some(value), None) => PropertyValue::Decimal(value),
            ("text", None, None, None, Some(value)) => PropertyValue::Text(value),
            _ => {
                return Err(corrupt(
                    "decoded_observations.value_kind",
                    "typed value columns disagree",
                ));
            }
        };
        let quality = match self.quality.as_str() {
            "good" => ObservationQuality::Good,
            "uncertain" => ObservationQuality::Uncertain,
            "invalid" => ObservationQuality::Invalid,
            _ => return Err(corrupt("decoded_observations.quality", "unknown quality")),
        };
        let measurement = DecodedMeasurement::new(
            value,
            self.source_unit
                .map(UnitId::new)
                .transpose()
                .map_err(|error| corrupt("decoded_observations.source_unit", error))?,
            quality,
        )
        .map_err(|error| corrupt("decoded_observations", error))?;
        Ok(DecodedObservation::new(
            id.clone(),
            tenant_id.clone(),
            SignalSource::new(
                AssetId::new(self.device_asset_id)
                    .map_err(|error| corrupt("accepted_messages.device_asset_id", error))?,
                DeviceEndpointId::new(self.endpoint_id)
                    .map_err(|error| corrupt("decoded_observations.endpoint_id", error))?,
                SourceSignalId::new(self.signal_id)
                    .map_err(|error| corrupt("decoded_observations.signal_id", error))?,
            ),
            measurement,
            ObservationTimes::new(
                self.device_recorded_at_ms.map(UnixTimeMillis::new),
                UnixTimeMillis::new(self.received_at_ms),
            ),
        ))
    }
}
